//! The XML data files of an Odoo module: which files its manifest loads, and
//! their parsed contents with byte positions.
//!
//! Which files count follows OCA's `oca-checks-odoo-module`: the files listed
//! under the manifest's data keys, `qweb` and `assets` (with glob patterns),
//! in installable modules that have an `__init__.py`.

use crate::checker::ModuleInfo;
use crate::manifest::Manifest;
use crate::odoo_version::OdooVersion;
use crate::pyliteral::is_truthy;
use crate::sources::Sources;
use globset::GlobBuilder;
use regex::Regex;
use ruff_python_ast::Expr;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Manifest keys whose files Odoo loads in order.
const DATA_KEYS: &[&str] = &["data", "demo", "demo_xml", "init_xml", "qweb", "test", "update_xml"];

/// An XML file the manifest loads, and the manifest key it is listed under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlEntry {
    pub path: PathBuf,
    pub section: String,
}

/// The string items of a list or tuple literal.
fn strings(expr: &Expr) -> Vec<&str> {
    let items: &[Expr] = match expr {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tuple) => &tuple.elts,
        _ => return Vec::new(),
    };
    items
        .iter()
        .filter_map(|item| item.as_string_literal_expr().map(|s| s.value.to_str()))
        .collect()
}

/// `relative` (with `/` separators) under `base`, with native separators.
fn join(base: &Path, relative: &str) -> PathBuf {
    let mut path = base.to_path_buf();
    path.extend(relative.split('/').filter(|part| !part.is_empty() && *part != "."));
    path
}

/// Python's `glob.glob(pattern)` relative to `base`, without `recursive`:
/// `**` matches like `*`, and wildcards skip hidden names. The matches are
/// `/`-separated paths relative to `base`, sorted.
pub fn py_glob(base: &Path, pattern: &str) -> Vec<String> {
    let mut found = vec![String::new()];
    for part in pattern.split('/').filter(|p| !p.is_empty()) {
        let wildcard = part.contains(['*', '?', '[']);
        let mut next = Vec::new();
        for prefix in &found {
            let dir = join(base, prefix);
            let child = |name: &str| {
                if prefix.is_empty() {
                    name.to_string()
                } else {
                    format!("{prefix}/{name}")
                }
            };
            if !wildcard {
                if dir.join(part).exists() {
                    next.push(child(part));
                }
                continue;
            }
            let Ok(matcher) = GlobBuilder::new(&part.replace("**", "*"))
                .literal_separator(true)
                .build()
                .map(|g| g.compile_matcher())
            else {
                continue;
            };
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if (name.starts_with('.') && !part.starts_with('.')) || !matcher.is_match(&name) {
                    continue;
                }
                next.push(child(&name));
            }
        }
        found = next;
    }
    found.retain(|path| !path.is_empty());
    found.sort();
    found
}

/// Files of `section` matched by the manifest's glob patterns (`qweb` and
/// `assets`), relative to the module.
fn glob_section(module: &ModuleInfo, manifest: &Manifest, section: &str) -> Vec<String> {
    let Some(value) = manifest.get(section) else {
        return Vec::new();
    };
    let lists: Vec<&Expr> = match value {
        Expr::Dict(dict) => dict.items.iter().map(|item| &item.value).collect(),
        other => vec![other],
    };
    let Some(addons) = module.path.parent() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for pattern in lists.into_iter().flat_map(strings) {
        let pattern = if section == "qweb" {
            format!("{}/{pattern}", module.name)
        } else {
            pattern.to_string()
        };
        for path in py_glob(addons, &pattern) {
            // Only the module's own files.
            if let Some(relative) = path.strip_prefix(&format!("{}/", module.name)) {
                found.push(relative.to_string());
            }
        }
    }
    found
}

/// Whether the module is loaded at all: installable, with an `__init__.py`.
pub fn is_installable(module: &ModuleInfo) -> bool {
    let Some(manifest) = &module.manifest else {
        return false;
    };
    module.path.join("__init__.py").is_file()
        && manifest
            .get("installable")
            .is_none_or(|value| is_truthy(value).unwrap_or(true))
}

/// The XML files the manifest loads, in manifest order. A file listed under
/// two keys appears twice; within one key, once.
pub fn manifest_xml_files(module: &ModuleInfo) -> Vec<XmlEntry> {
    let Some(manifest) = &module.manifest else {
        return Vec::new();
    };
    if !is_installable(module) {
        return Vec::new();
    }
    let mut entries: Vec<XmlEntry> = Vec::new();
    for section in DATA_KEYS.iter().copied().chain(["assets"]) {
        let files: Vec<String> = if section == "qweb" || section == "assets" {
            glob_section(module, manifest, section)
        } else {
            manifest
                .get(section)
                .map(|value| strings(value).into_iter().map(str::to_string).collect())
                .unwrap_or_default()
        };
        for file in files {
            if !file.to_lowercase().ends_with(".xml") {
                continue;
            }
            let entry = XmlEntry {
                path: join(&module.path, &file),
                section: section.to_string(),
            };
            if !entries.contains(&entry) {
                entries.push(entry);
            }
        }
    }
    entries
}

static MANIFEST_SERIES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)\.(\d+)\.\d+\.\d+\.\d+$").unwrap());
static NUMERIC_VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+(\.\d+)*$").unwrap());

/// The Odoo version a module is written for: the series in its manifest's
/// `version` (`18.0.1.0.0`). Odoo prefixes shorter versions, and a missing
/// one, with the current series, so those get `fallback`. `None` for a
/// malformed version (`8_0.1.0.0`): the version-gated checks then skip the
/// module, as OCA's do.
/// The Odoo series of a manifest version such as `17.0.1.0.0`.
pub fn manifest_series(version: &str) -> Option<OdooVersion> {
    let captures = MANIFEST_SERIES.captures(version.trim())?;
    Some(OdooVersion::new(captures[1].parse().ok()?, captures[2].parse().ok()?))
}

pub fn module_version(module: &ModuleInfo, fallback: OdooVersion) -> Option<OdooVersion> {
    let Some(version) = module.manifest.as_ref().and_then(|m| m.get_str("version")) else {
        return Some(fallback);
    };
    if MANIFEST_SERIES.is_match(version.trim()) {
        return manifest_series(version);
    }
    NUMERIC_VERSION.is_match(version.trim()).then_some(fallback)
}

/// A parsed XML file as the checks see it.
pub struct XmlFile<'a> {
    pub path: &'a Path,
    /// The path as reported.
    pub display: String,
    /// The manifest key the file is listed under (`data`, `demo`, ...).
    pub section: &'a str,
    pub source: &'a str,
    /// `None` when the file cannot be read or parsed; see `error`.
    pub doc: Option<roxmltree::Document<'a>>,
    pub error: Option<String>,
    /// Checks disabled for the whole file by comments.
    pub disabled: HashSet<String>,
    /// The first tag of the file and its line, read as text.
    pub first_tag: Option<(String, usize)>,
    line_starts: Vec<usize>,
}

static DISABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"oca-hooks:disable=([a-z0-9\-,]+)").unwrap());
static PYLINT_DISABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"pylint:disable=([a-zA-Z0-9\-,]+)").unwrap());

/// Check names (or codes) disabled by a comment such as
/// `<!-- oca-hooks:disable=xml-record-missing-id -->`.
fn disabled_in(comment: &str) -> Vec<String> {
    let compact: String = comment.chars().filter(|c| !matches!(c, '\n' | ' ' | '#')).collect();
    let found = DISABLE.captures(&compact).or_else(|| PYLINT_DISABLE.captures(&compact));
    found.map_or_else(Vec::new, |c| c[1].split(',').map(str::to_string).collect())
}

/// The first tag of the file as OCA reads it: from the first line starting
/// with `<` to the first line ending with `>`, joined with spaces.
fn first_tag(source: &str) -> (String, usize) {
    let mut buffer: Vec<&str> = Vec::new();
    let mut line_number = 0;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        let stripped = line.trim_matches([' ', '\n']);
        if buffer.is_empty() && stripped.starts_with('<') {
            buffer.push(stripped);
            line_number = index + 1;
        }
        if !buffer.is_empty() && stripped.ends_with('>') {
            if !buffer.contains(&stripped) {
                buffer.push(stripped);
            }
            return (buffer.join(" "), line_number);
        }
    }
    (String::new(), 0)
}

impl<'a> XmlFile<'a> {
    /// Parses `source` (`None`: the file could not be read as UTF-8 text).
    pub fn parse(path: &'a Path, section: &'a str, source: Option<&'a str>, read_error: Option<String>) -> Self {
        let display = path.to_string_lossy().into_owned();
        let Some(source) = source else {
            return XmlFile {
                path,
                display,
                section,
                source: "",
                doc: None,
                error: read_error.or_else(|| Some("cannot read file".to_string())),
                disabled: HashSet::new(),
                first_tag: None,
                line_starts: vec![0],
            };
        };
        let mut line_starts = vec![0];
        line_starts.extend(source.match_indices('\n').map(|(i, _)| i + 1));
        let options = roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        };
        let (doc, error) = match roxmltree::Document::parse_with_options(source, options) {
            Ok(doc) => (Some(doc), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let disabled = doc
            .iter()
            .flat_map(|d| d.descendants())
            .filter(|n| n.is_comment())
            .flat_map(|n| disabled_in(n.text().unwrap_or_default()))
            .collect();
        let first_tag = doc.as_ref().map(|_| first_tag(source));
        XmlFile {
            path,
            display,
            section,
            source,
            doc,
            error,
            disabled,
            first_tag,
            line_starts,
        }
    }

    /// 1-based line and column of byte `offset`.
    pub fn line_column(&self, offset: usize) -> (usize, usize) {
        let line = self.line_starts.partition_point(|&start| start <= offset);
        let start = self.line_starts[line - 1];
        let column = self.source.get(start..offset).map_or(0, |s| s.chars().count());
        (line, column + 1)
    }

    /// Byte offset where 1-based `line` starts.
    pub fn line_start(&self, line: usize) -> usize {
        self.line_starts.get(line.saturating_sub(1)).copied().unwrap_or(0)
    }

    /// The root element, when the file parsed.
    pub fn root(&self) -> Option<roxmltree::Node<'_, 'a>> {
        self.doc.as_ref().map(|d| d.root_element())
    }

    /// All elements in document order.
    pub fn elements(&self) -> impl Iterator<Item = roxmltree::Node<'_, 'a>> {
        self.doc.iter().flat_map(|d| d.descendants()).filter(|n| n.is_element())
    }
}

/// Reads the files of `entries` from `sources`, keeping them alive for the
/// parsed documents that borrow them.
pub fn read_entries(entries: &[XmlEntry], sources: &Sources) -> Vec<Result<String, String>> {
    entries
        .iter()
        .map(|entry| match sources.read(&entry.path) {
            Ok(bytes) => String::from_utf8(bytes).map_err(|e| format!("not valid UTF-8: {e}")),
            Err(error) => Err(error.to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_tag_spans_lines() {
        assert_eq!(
            first_tag("\n<?xml version='1.0'\n   encoding='utf-8'?>\n<odoo/>"),
            ("<?xml version='1.0' encoding='utf-8'?>".to_string(), 2)
        );
        assert_eq!(first_tag("<odoo>\n</odoo>\n"), ("<odoo>".to_string(), 1));
    }

    #[test]
    fn disable_comments() {
        assert_eq!(
            disabled_in(" oca-hooks:disable=xml-record-missing-id, xml-tag-position "),
            vec!["xml-record-missing-id", "xml-tag-position"]
        );
        assert_eq!(disabled_in("pylint:disable=xml-syntax-error"), vec!["xml-syntax-error"]);
        assert!(disabled_in("just a comment").is_empty());
    }

    #[test]
    fn python_glob() {
        let dir = tempfile::tempdir().unwrap();
        let module = dir.path().join("acme");
        std::fs::create_dir_all(module.join("static/src/xml/sub")).unwrap();
        for file in [
            "static/src/xml/a.xml",
            "static/src/xml/b.js",
            "static/src/xml/sub/c.xml",
            "static/src/xml/.d.xml",
        ] {
            std::fs::write(module.join(file), "").unwrap();
        }
        assert_eq!(
            py_glob(dir.path(), "acme/static/src/xml/*.xml"),
            vec!["acme/static/src/xml/a.xml"]
        );
        // Without `recursive`, `**` is a single level.
        assert_eq!(
            py_glob(dir.path(), "acme/static/src/**/*.xml"),
            vec!["acme/static/src/xml/a.xml"]
        );
        assert_eq!(
            py_glob(dir.path(), "acme/static/src/xml/a.xml"),
            vec!["acme/static/src/xml/a.xml"]
        );
    }
}
