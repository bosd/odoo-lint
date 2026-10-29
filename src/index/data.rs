//! A module's data files: the XML ids they create, its access rights and
//! its views.

use super::python::strings;
use super::{indexed_files, text_size, Access, ModelClass, SKIPPED_DIRS};
use crate::manifest::{Manifest, LOADED_KEYS};
use ruff_source_file::LineIndex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Where Odoo creates an XML id: the rank of the data file in load order
/// (0 for ids Odoo creates before loading data) and the byte offset of the
/// defining element in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub rank: usize,
    pub offset: usize,
}

/// A data file the manifest loads.
#[derive(Debug, Clone)]
pub struct DataFile {
    pub path: PathBuf,
    /// Its place in Odoo's load order, from 1.
    pub rank: usize,
    /// Whether it is demo data (`demo`, `demo_xml`).
    pub demo: bool,
}

/// Elements that define an XML id.
const ID_ELEMENTS: &[&str] = &["record", "template", "menuitem", "report", "act_window", "url", "asset"];

pub(super) fn data_files(path: &Path, manifest: &Manifest) -> Vec<DataFile> {
    let mut files: Vec<DataFile> = Vec::new();
    for key in LOADED_KEYS {
        for name in manifest.get(key).map(strings).unwrap_or_default() {
            let file = path.join(&name);
            if files.iter().any(|f| f.path == file) {
                continue;
            }
            let rank = files.len() + 1;
            files.push(DataFile {
                path: file,
                rank,
                demo: key.starts_with("demo"),
            });
        }
    }
    files
}

static SQL_XML_ID: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)insert\s+into\s+ir_model_data\s*\(\s*name\s*,\s*module[^)]*\)\s*values\s*\(\s*'([^']+)'\s*,\s*'([^']+)'",
    )
    .unwrap()
});

/// Magic fields, which get `field_<model>__<name>` ids like the others.
const MAGIC_FIELD_IDS: &[&str] = &[
    "id",
    "display_name",
    "create_uid",
    "create_date",
    "write_uid",
    "write_date",
];

static PYTHON_XML_ID: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?:['"]xml_id['"]\s*:|\b(?:ref_name|xml_id|xmlid)\s*=)\s*['"]([\w.]+)['"]"#).unwrap()
});

/// The XML ids a module creates, by full name (`module.name`): its own and
/// those it creates in another module's namespace (`base.demo_company_ae`).
type XmlIdLines = HashMap<String, (PathBuf, usize)>;

pub(super) fn xml_ids(
    module: &str,
    path: &Path,
    files: &[DataFile],
    classes: &[ModelClass],
) -> (HashMap<String, Position>, XmlIdLines) {
    let mut ids: HashMap<String, Position> = HashMap::new();
    let mut lines: XmlIdLines = HashMap::new();
    let full = |id: &str| {
        if id.contains('.') {
            id.to_string()
        } else {
            format!("{module}.{id}")
        }
    };
    let generated = Position { rank: 0, offset: 0 };
    // ir.model and ir.model.fields records of the module's classes.
    for class in classes {
        for model in class.name.iter().chain(class.inherit.iter()) {
            let model = model.replace('.', "_");
            ids.insert(format!("{module}.model_{model}"), generated);
            let magic = class
                .name
                .iter()
                .flat_map(|_| MAGIC_FIELD_IDS.iter().map(|f| f.to_string()));
            for field in class.fields.iter().map(|f| f.name.clone()).chain(magic) {
                ids.insert(format!("{module}.field_{model}__{field}"), generated);
            }
        }
    }
    // Records `base_data.sql` creates before any data file.
    for entry in walkdir::WalkDir::new(path.join("data"))
        .max_depth(1)
        .into_iter()
        .flatten()
    {
        if entry.path().extension().is_none_or(|e| e != "sql") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for captures in SQL_XML_ID.captures_iter(&text) {
            ids.insert(format!("{}.{}", &captures[2], &captures[1]), generated);
        }
    }
    // Records Python code creates with an XML id (`_load_records`).
    for file in indexed_files(path) {
        let Ok(text) = std::fs::read_to_string(file.path()) else {
            continue;
        };
        for captures in PYTHON_XML_ID.captures_iter(&text) {
            ids.entry(full(&captures[1])).or_insert(generated);
        }
    }
    // The manifest's data files, in load order, then the module's other
    // data files, which code loads (`convert_file`) at a moment we cannot
    // know: those count as there from the start.
    let listed: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
    let others = walkdir::WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| !(e.file_type().is_dir() && SKIPPED_DIRS.contains(&e.file_name().to_string_lossy().as_ref())))
        .flatten()
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|x| matches!(x.to_str(), Some("xml" | "csv")))
                && !listed.contains(&e.path().to_path_buf())
        })
        .map(|e| DataFile {
            path: e.path().to_path_buf(),
            rank: 0,
            demo: false,
        })
        .collect::<Vec<_>>();
    for file in files.iter().chain(others.iter()) {
        let Ok(text) = std::fs::read_to_string(&file.path) else {
            continue;
        };
        let extension = file.path.extension().map(|e| e.to_string_lossy().to_lowercase());
        match extension.as_deref() {
            Some("xml") => {
                // `_load_records` in `<function>`/`eval` code.
                for captures in PYTHON_XML_ID.captures_iter(&text) {
                    let offset = captures.get(0).map_or(0, |m| m.start());
                    ids.entry(full(&captures[1])).or_insert(Position {
                        rank: file.rank,
                        offset,
                    });
                }
                let Ok(doc) = roxmltree::Document::parse(&text) else {
                    continue;
                };
                let line_index = LineIndex::from_source_text(&text);
                for node in doc.descendants().filter(|n| n.is_element()) {
                    if !ID_ELEMENTS.contains(&node.tag_name().name()) {
                        continue;
                    }
                    let Some(id) = node.attribute("id") else { continue };
                    let position = Position {
                        rank: file.rank,
                        offset: node.range().start,
                    };
                    ids.entry(full(id)).or_insert(position);
                    let line = line_index.line_index(text_size(node.range().start)).get();
                    lines.entry(full(id)).or_insert((file.path.clone(), line));
                }
            }
            Some("csv") => {
                let Ok(records) = crate::rules::module::read_csv(&text) else {
                    continue;
                };
                let mut rows = records.into_iter();
                let Some(header) = rows.next() else { continue };
                let Some(column) = header.fields.iter().position(|f| f == "id") else {
                    continue;
                };
                for row in rows {
                    let Some(id) = row.fields.get(column).filter(|id| !id.is_empty()) else {
                        continue;
                    };
                    let position = Position {
                        rank: file.rank,
                        offset: row.line,
                    };
                    ids.entry(full(id)).or_insert(position);
                    lines.entry(full(id)).or_insert((file.path.clone(), row.line));
                }
            }
            _ => {}
        }
    }
    (ids, lines)
}

static REF: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r#"ref\(\s*['"]([\w.]+)['"]\s*\)"#).unwrap());

/// The access rights and implied groups of a module's data files.
type Security = (Vec<Access>, Vec<(String, Vec<String>)>);

pub(super) fn security(module: &str, files: &[DataFile]) -> Security {
    let (mut access, mut implied) = (Vec::new(), Vec::new());
    let yes = |value: &str| matches!(value.trim(), "1" | "True" | "true");
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file.path) else {
            continue;
        };
        let name = file
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.ends_with(".csv") {
            if !name.starts_with("ir.model.access") && !name.starts_with("ir_model_access") {
                continue;
            }
            let Ok(records) = crate::rules::module::read_csv(&text) else {
                continue;
            };
            let mut rows = records.into_iter();
            let Some(header) = rows.next() else { continue };
            let column = |names: &[&str]| header.fields.iter().position(|f| names.contains(&f.trim()));
            let (Some(model), Some(read)) = (column(&["model_id:id", "model_id/id"]), column(&["perm_read"])) else {
                continue;
            };
            let group = column(&["group_id:id", "group_id/id"]);
            for row in rows {
                let Some(model) = row.fields.get(model).filter(|m| !m.trim().is_empty()) else {
                    continue;
                };
                access.push(Access {
                    model: qualify(module, model.trim()),
                    group: group
                        .and_then(|g| row.fields.get(g))
                        .map(|g| g.trim())
                        .filter(|g| !g.is_empty())
                        .map(|g| qualify(module, g)),
                    read: row.fields.get(read).is_some_and(|r| yes(r)),
                });
            }
            continue;
        }
        if !name.ends_with(".xml") {
            continue;
        }
        let Ok(doc) = roxmltree::Document::parse(&text) else {
            continue;
        };
        for record in doc.descendants().filter(|n| n.has_tag_name("record")) {
            let field = |name: &str| {
                record
                    .children()
                    .find(|c| c.has_tag_name("field") && c.attribute("name") == Some(name))
            };
            match record.attribute("model") {
                Some("ir.model.access") => {
                    let Some(model) = field("model_id").and_then(|f| f.attribute("ref")) else {
                        continue;
                    };
                    let read = field("perm_read").and_then(|f| f.attribute("eval").or_else(|| f.text()));
                    access.push(Access {
                        model: qualify(module, model),
                        group: field("group_id")
                            .and_then(|f| f.attribute("ref"))
                            .map(|g| qualify(module, g)),
                        read: read.is_some_and(yes),
                    });
                }
                Some("res.groups") => {
                    let (Some(id), Some(eval)) = (
                        record.attribute("id"),
                        field("implied_ids").and_then(|f| f.attribute("eval")),
                    ) else {
                        continue;
                    };
                    let groups = REF.captures_iter(eval).map(|c| qualify(module, &c[1])).collect();
                    implied.push((qualify(module, id), groups));
                }
                _ => {}
            }
        }
    }
    (access, implied)
}

/// A view as a data file defines it.
#[derive(Debug, Clone)]
pub struct View {
    /// Full XML id (`module.name`).
    pub id: String,
    pub file: PathBuf,
    /// Where the view is loaded; also its line.
    pub position: Position,
    pub line: usize,
    /// Full XML id of the view it inherits from.
    pub inherit_id: Option<String>,
    /// The `model` field of a record view; `None` for a template.
    pub model: Option<String>,
    /// A primary view starts a new arch; an extension changes its parent's.
    pub primary: bool,
    pub priority: i64,
    pub active: bool,
    /// The arch as XML text: its root element (or `<data>` with the specs).
    pub arch: Option<String>,
    /// Byte offset in `file` of the arch text, and the length of what was
    /// added in front of it (a wrapper element), to find lines again.
    pub arch_offset: usize,
    pub arch_prefix: usize,
}

fn qualify(module: &str, id: &str) -> String {
    if id.contains('.') {
        id.to_string()
    } else {
        format!("{module}.{id}")
    }
}

fn truthy(value: &str) -> bool {
    !matches!(value.trim(), "" | "0" | "False" | "false")
}

/// The views of a module's data files, in load order.
pub(super) fn views(module: &str, files: &[DataFile]) -> Vec<View> {
    let mut out = Vec::new();
    for file in files {
        if file.path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("xml")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file.path) else {
            continue;
        };
        let Ok(doc) = roxmltree::Document::parse(&text) else {
            continue;
        };
        let line_index = LineIndex::from_source_text(&text);
        let line_of = |offset: usize| line_index.line_index(text_size(offset)).get();
        for node in doc.descendants().filter(|n| n.is_element()) {
            let Some(id) = node.attribute("id") else { continue };
            let position = Position {
                rank: file.rank,
                offset: node.range().start,
            };
            if node.has_tag_name("template") {
                let inherit_id = node.attribute("inherit_id").map(|i| qualify(module, i));
                // `_tag_template` takes exactly `primary="True"`.
                let primary = inherit_id.is_none() || node.attribute("primary") == Some("True");
                // Odoo's `_tag_template`: `<t t-name>` for a primary view,
                // `<data>` with the specs for an extension.
                let (inner_start, inner_end) = inner_range(&text, node);
                let full = qualify(module, id);
                let prefix = if primary && inherit_id.is_none() {
                    format!("<t t-name=\"{full}\">")
                } else {
                    "<data>".to_string()
                };
                let suffix = if primary && inherit_id.is_none() {
                    "</t>"
                } else {
                    "</data>"
                };
                let arch = format!("{prefix}{}{suffix}", &text[inner_start..inner_end]);
                out.push(View {
                    id: full,
                    file: file.path.clone(),
                    position,
                    line: line_of(node.range().start),
                    inherit_id,
                    model: None,
                    primary,
                    priority: node
                        .attribute("priority")
                        .and_then(|p| p.trim().parse().ok())
                        .unwrap_or(16),
                    active: node.attribute("active").is_none_or(truthy),
                    arch: Some(arch),
                    arch_offset: inner_start,
                    arch_prefix: prefix.len(),
                });
                continue;
            }
            if !(node.has_tag_name("record") && node.attribute("model") == Some("ir.ui.view")) {
                continue;
            }
            let field = |name: &str| {
                node.children()
                    .find(|c| c.has_tag_name("field") && c.attribute("name") == Some(name))
            };
            let value = |name: &str| {
                field(name).map(|f| {
                    f.attribute("eval")
                        .or_else(|| f.text())
                        .unwrap_or_default()
                        .trim()
                        .to_string()
                })
            };
            let inherit_id = field("inherit_id")
                .and_then(|f| f.attribute("ref"))
                .map(|r| qualify(module, r));
            let primary = inherit_id.is_none() || value("mode").as_deref() == Some("primary");
            let (arch, arch_offset) = match field("arch") {
                Some(arch) => {
                    let (start, end) = inner_range(&text, arch);
                    (Some(format!("<__arch__>{}</__arch__>", &text[start..end])), start)
                }
                None => (None, 0),
            };
            out.push(View {
                id: qualify(module, id),
                file: file.path.clone(),
                position,
                line: line_of(node.range().start),
                inherit_id,
                model: value("model").filter(|m| !m.is_empty()),
                primary,
                priority: value("priority").and_then(|p| p.parse().ok()).unwrap_or(16),
                active: value("active").is_none_or(|a| truthy(&a)),
                arch,
                arch_offset,
                arch_prefix: "<__arch__>".len(),
            });
        }
    }
    out
}

/// The byte range of an element's content, between its tags.
fn inner_range(text: &str, node: roxmltree::Node) -> (usize, usize) {
    let range = node.range();
    let source = &text[range.clone()];
    if source.ends_with("/>") {
        return (range.end, range.end);
    }
    let start = node
        .attributes()
        .map(|a| a.range().end)
        .max()
        .unwrap_or(range.start + 1);
    let start = text[start..range.end].find('>').map_or(range.end, |i| start + i + 1);
    let end = source.rfind("</").map_or(range.end, |i| range.start + i);
    (start, end.max(start))
}
