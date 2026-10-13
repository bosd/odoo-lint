//! Find references: where an XML id, a model or a field is used, in the
//! modules next to the open one and on the addons path. A use is a mention
//! of the name whose go to definition leads to the same place. The language
//! server answers `textDocument/references` with it.

use crate::checker::ModuleInfo;
use crate::definition::{python_target_in, resolve, xml_target_in, Definition, Target};
use crate::index::{closure, closure_at, Closure};
use crate::manifest::MANIFEST_FILE_NAMES;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Folders of a module that hold no references worth showing.
const SKIPPED_DIRS: &[&str] = &["static", "i18n", "i18n_extra", "__pycache__", "node_modules"];

/// A use: a file and the byte range of the name in it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Usage {
    pub path: PathBuf,
    pub start: usize,
    pub end: usize,
}

/// The name to look for in the files: what a use spells out.
fn needle(target: &Target) -> Option<String> {
    let name = match target {
        Target::XmlId(id) => id.rsplit('.').next()?.to_string(),
        Target::Model(model) => model.clone(),
        Target::Field { path, segment, .. } => path.split('.').nth(*segment)?.to_string(),
        Target::ViewField { name, .. } => name.clone(),
    };
    (!name.is_empty()).then_some(name)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The offsets of `needle` in `text` as a whole name: not part of a longer
/// identifier (a model name such as `sale.order` may still be followed by
/// `.line`, which the definition then tells apart).
fn occurrences<'a>(text: &'a str, needle: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(needle).map(|(i, _)| i).filter(move |&i| {
        let before = text[..i].chars().next_back();
        let after = text[i + needle.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

fn same(a: &Definition, b: &Definition) -> bool {
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    a.line == b.line && canonical(&a.path) == canonical(&b.path)
}

/// The module folders of `dirs`, each once.
fn module_dirs(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if !MANIFEST_FILE_NAMES.iter().any(|m| path.join(m).is_file()) {
                continue;
            }
            let key = path.canonicalize().unwrap_or_else(|_| path.clone());
            if seen.insert(key) {
                found.push(path);
            }
        }
    }
    found
}

/// The Python and XML files of a module.
fn module_files(path: &Path) -> impl Iterator<Item = PathBuf> {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| !(e.file_type().is_dir() && SKIPPED_DIRS.contains(&e.file_name().to_string_lossy().as_ref())))
        .flatten()
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "py" || x == "xml"))
        .map(|e| e.into_path())
}

/// The uses of what is at byte `offset` of `text`, the content of `file` in
/// `module`; with `include_declaration`, its definition too.
pub fn references(
    module: &ModuleInfo,
    addons_path: &[PathBuf],
    file: &Path,
    text: &str,
    offset: usize,
    include_declaration: bool,
) -> Vec<Usage> {
    let mut dirs: Vec<PathBuf> = module.path.parent().map(PathBuf::from).into_iter().collect();
    dirs.extend(addons_path.iter().cloned());
    let target = match file.extension().and_then(|e| e.to_str()) {
        Some("xml") => roxmltree::Document::parse(text)
            .ok()
            .and_then(|doc| xml_target_in(&doc, offset, &module.name)),
        Some("py") => ruff_python_parser::parse_module(text)
            .ok()
            .and_then(|parsed| python_target_in(&parsed, text, offset, &module.name)),
        _ => None,
    };
    let Some((target, _)) = target else { return Vec::new() };
    let Some(needle) = needle(&target) else {
        return Vec::new();
    };
    let Some(definition) = resolve(closure(module, addons_path).as_ref(), &dirs, target) else {
        return Vec::new();
    };

    let mut closures: HashMap<PathBuf, Option<Closure>> = HashMap::new();
    let mut found = Vec::new();
    let open_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    for module_dir in module_dirs(&dirs) {
        let name = module_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        for path in module_files(&module_dir) {
            // The open file as the editor has it.
            let content = if path.canonicalize().ok().as_ref() == Some(&open_file) {
                text.to_string()
            } else {
                match std::fs::read_to_string(&path) {
                    Ok(content) => content,
                    Err(_) => continue,
                }
            };
            let offsets: Vec<usize> = occurrences(&content, &needle).collect();
            if offsets.is_empty() {
                continue;
            }
            let closure = closures
                .entry(module_dir.clone())
                .or_insert_with(|| closure_at(&module_dir, addons_path))
                .as_ref();
            let is_xml = path.extension().is_some_and(|x| x == "xml");
            let doc = if is_xml {
                roxmltree::Document::parse(&content).ok()
            } else {
                None
            };
            let parsed = if is_xml {
                None
            } else {
                ruff_python_parser::parse_module(&content).ok()
            };
            for start in offsets {
                // Inside the name, past a leading quote or dot.
                let at = start + 1;
                let target = match (&doc, &parsed) {
                    (Some(doc), _) => xml_target_in(doc, at, &name),
                    (_, Some(parsed)) => python_target_in(parsed, &content, at, &name),
                    _ => None,
                };
                let Some((target, _)) = target else { continue };
                // The definition itself (`partner_id = fields...`) only as the declaration.
                let line = content[..start].matches('\n').count() + 1;
                let declaration =
                    line == definition.line && path.canonicalize().ok() == definition.path.canonicalize().ok();
                if declaration && !include_declaration {
                    continue;
                }
                if resolve(closure, &dirs, target).is_some_and(|d| same(&d, &definition)) {
                    found.push(Usage {
                        path: path.clone(),
                        start,
                        end: start + needle.len(),
                    });
                }
            }
        }
    }
    if include_declaration {
        if let Ok(content) = std::fs::read_to_string(&definition.path) {
            // The name on the definition's line or just below it (a class's
            // `_name`); the line's start otherwise.
            let line_start: usize = content
                .split_inclusive('\n')
                .take(definition.line - 1)
                .map(str::len)
                .sum();
            let near: usize = content[line_start..].split_inclusive('\n').take(4).map(str::len).sum();
            let (start, end) = match occurrences(&content[line_start..line_start + near], &needle).next() {
                Some(i) => (line_start + i, line_start + i + needle.len()),
                None => (line_start, line_start),
            };
            found.push(Usage {
                path: definition.path.clone(),
                start,
                end,
            });
        }
    }
    // One spelling per file (the definition's is canonical: `/private/var`
    // on macOS), for the order and to drop doubles.
    for usage in &mut found {
        if let Ok(canonical) = usage.path.canonicalize() {
            usage.path = canonical;
        }
    }
    found.sort();
    found.dedup();
    found
}
