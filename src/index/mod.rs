//! A model of the addons path: per module, its dependencies and the models
//! and fields its Python code declares. Checks across modules (fields in
//! views, ...) ask it what a module can rely on: the fields of a model as
//! the modules its `depends` reach define them.
//!
//! Modules are parsed on first use and cached for the run, so only the
//! dependencies of the checked modules are read, not all of Odoo. A hint
//! about where a missing model or field lives searches the whole addons
//! path, but parses only the modules whose Python mentions it.

mod data;
mod python;
mod scope;

pub use data::{DataFile, Position, View};
pub(crate) use python::class_models;
pub use scope::{closure, closure_at, closure_of, dependencies, missing_dependencies, Closure, MethodSet};

use data::{data_files, security, views, xml_ids};
use python::{model_class, string, strings, Constants};

use crate::manifest::{find_module_dir, manifest_path, module_dirs, Manifest};
use crate::rules::python::classes;
use rayon::prelude::*;
use ruff_python_ast::{Expr, Stmt};
use ruff_python_parser::parse_module;
use ruff_source_file::LineIndex;
use ruff_text_size::{Ranged, TextSize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;

/// A field as a class declares it.
#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    /// `Many2one`, `One2many`, `Char`, ...
    pub kind: String,
    /// The comodel of a relational field, when it is a literal.
    pub comodel: Option<String>,
    /// Whether the definition sets a `domain`.
    pub has_domain: bool,
    /// The `groups` the field is restricted to, when it is a literal.
    pub groups: Option<String>,
    /// The line of its definition in its class's file (0 when unknown).
    pub line: usize,
    /// What the definition says about its label, for translation templates.
    pub label: FieldLabel,
}

/// The label attributes a field definition sets, each `None` when it leaves
/// it to an earlier definition (or to Odoo's default).
#[derive(Debug, Clone, Default)]
pub struct FieldLabel {
    /// `string`, positional or keyword.
    pub string: Option<String>,
    pub help: Option<String>,
    /// The dotted path of a related field.
    pub related: Option<String>,
    pub selection: Option<Selection>,
    /// `selection_add`: each value, with its label when it has one.
    pub selection_add: Vec<(String, Option<String>)>,
    pub export_string_translation: Option<bool>,
    /// `translate`: `None` when the definition does not set it.
    pub translate: Option<Translate>,
    /// `sanitize` of an HTML field: `None` when the definition does not set it.
    pub sanitize: Option<Sanitize>,
}

/// The `sanitize` of an HTML field, as far as it decides how its values are
/// translated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sanitize {
    Yes,
    No,
    /// `sanitize='email_outgoing'`: sanitized, yet translated as a whole.
    EmailOutgoing,
}

/// How a field's values are translated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Translate {
    /// Not translated (`translate=False`).
    No,
    /// The whole value is one term (`translate=True`).
    Whole,
    /// The terms of an XML value (`xml_translate`, views).
    Xml,
    /// The terms of an HTML value (`html_translate`).
    Html,
}

/// The `selection` of a field definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// A literal list of `(value, label)` pairs.
    List(Vec<(String, String)>),
    /// A method or a name: known only when Odoo runs.
    Dynamic,
}

/// A database constraint a class declares, with the message Odoo shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constraint {
    /// The key of `_sql_constraints`, or the attribute name without its `_`
    /// for `models.Constraint` (19.0).
    pub name: String,
    pub message: Option<String>,
    /// Whether it is a table object (`models.Constraint`, 19.0) rather than
    /// an entry of `_sql_constraints`.
    pub table_object: bool,
}

impl Field {
    pub fn is_x2many(&self) -> bool {
        matches!(self.kind.as_str(), "One2many" | "Many2many")
    }
}

/// The fields of a model, by name.
pub type FieldMap = HashMap<String, Field>;

/// A class with `_name` or `_inherit`.
#[derive(Debug)]
pub struct ModelClass {
    /// Whether the class is an `AbstractModel` (a mixin).
    pub abstract_model: bool,
    /// Where the class is: file and line of its `class` statement.
    pub file: PathBuf,
    pub line: usize,
    pub name: Option<String>,
    pub inherit: Vec<String>,
    /// `_inherits` parents.
    pub delegates: Vec<String>,
    pub fields: Vec<Field>,
    /// The methods defined in the class body, and other names it assigns.
    pub methods: Vec<String>,
    /// The line of each method (`def`) in the class's file.
    pub method_lines: Vec<(String, usize)>,
    /// Whether the class has bases other than Odoo's model classes (a
    /// Python mixin), whose methods the index does not know.
    pub foreign_bases: bool,
    /// Whether a base is one of Odoo's model classes: components
    /// (`AbstractComponent`) have a `_name` too.
    pub odoo_model: bool,
    /// The last name of each base that is not one of Odoo's model classes:
    /// a base that is a model class of the module makes this one a model too.
    pub python_bases: Vec<String>,
    /// `_description`, when it is a literal.
    pub description: Option<String>,
    /// `_table`, when it is a literal.
    pub table: Option<String>,
    /// `_auto` and `_log_access`, when they are literals.
    pub auto: Option<bool>,
    pub log_access: Option<bool>,
    pub constraints: Vec<Constraint>,
}

#[derive(Debug)]
pub struct ModuleIndex {
    /// The XML ids the module creates, by full name (`module.name`), and
    /// where, including the ones Odoo generates for its models and fields.
    pub xml_ids: HashMap<String, Position>,
    /// Where each XML id of a data file is: file and line.
    pub xml_id_lines: HashMap<String, (PathBuf, usize)>,
    /// The views (`ir.ui.view` records and `<template>`s) of its data files.
    pub views: Vec<View>,
    /// The data files the manifest loads, in Odoo's order.
    pub data_files: Vec<DataFile>,
    pub name: String,
    pub path: PathBuf,
    pub depends: Vec<String>,
    /// The dependencies whose installation installs the module
    /// (`auto_install`): all of `depends` for `True`, `None` when it is not
    /// installed automatically.
    pub auto_install: Option<Vec<String>>,
    pub classes: Vec<ModelClass>,
    /// The access rights (`ir.model.access`) of its data files.
    pub access: Vec<Access>,
    /// The groups each group of its data files implies (`implied_ids`), by
    /// full XML id.
    pub implied: Vec<(String, Vec<String>)>,
}

/// An access right: the model's XML id (`base.model_res_partner`), the
/// group's (none for everyone), and whether it grants reading.
#[derive(Debug, Clone)]
pub struct Access {
    pub model: String,
    pub group: Option<String>,
    pub read: bool,
}

/// Folders of a module that Odoo does not import as models.
const SKIPPED_DIRS: &[&str] = &[
    "tests",
    "migrations",
    "upgrades",
    "static",
    "i18n",
    "i18n_extra",
    "__pycache__",
    "node_modules",
];

/// Fields every model has.
const MAGIC_FIELDS: &[&str] = &[
    "id",
    "display_name",
    "create_uid",
    "create_date",
    "write_uid",
    "write_date",
    "__last_update",
];

/// A module's index, the fingerprint of the files it was built from, and
/// the generation (see [`revalidate_cache`]) it was last checked in.
type Cached = (Option<Arc<ModuleIndex>>, Fingerprint, u64);

static CACHE: LazyLock<Mutex<HashMap<PathBuf, Cached>>> = LazyLock::new(Default::default);

/// Whether cached modules are checked for changes: in a long-running
/// process (the language server) files change between runs.
static REVALIDATE: AtomicBool = AtomicBool::new(false);

/// Bumped by [`revalidate_cache`]: a module checked in the current
/// generation is not checked again.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Re-read a module whenever its Python files or manifest change, instead of
/// once per process. Files may have changed since the last call: each module
/// is checked again once, the next time it is used.
pub fn revalidate_cache() {
    REVALIDATE.store(true, Ordering::Relaxed);
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Whether caches must check their files for changes (see [`revalidate_cache`]).
pub(crate) fn revalidating() -> bool {
    REVALIDATE.load(Ordering::Relaxed)
}

/// The number of indexed files of a module and their newest change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Fingerprint {
    files: usize,
    newest: Option<SystemTime>,
}

/// A byte offset as ruff's text size; files over 4 GiB are not source.
fn text_size(offset: usize) -> TextSize {
    TextSize::new(u32::try_from(offset).unwrap_or(u32::MAX))
}

fn indexed_files(path: &Path) -> impl Iterator<Item = walkdir::DirEntry> {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| !(e.file_type().is_dir() && SKIPPED_DIRS.contains(&e.file_name().to_string_lossy().as_ref())))
        .flatten()
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "py"))
}

fn fingerprint(path: &Path) -> Fingerprint {
    let mut print = Fingerprint::default();
    // Python for models and fields, data files for XML ids.
    let files = walkdir::WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| !(e.file_type().is_dir() && SKIPPED_DIRS.contains(&e.file_name().to_string_lossy().as_ref())))
        .flatten()
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|x| matches!(x.to_str(), Some("py" | "xml" | "csv" | "sql")))
        });
    for file in files {
        print.files += 1;
        let modified = file.metadata().ok().and_then(|m| m.modified().ok());
        print.newest = print.newest.max(modified);
    }
    print
}

fn parse(path: &Path) -> Option<ModuleIndex> {
    let manifest = Manifest::parse(&std::fs::read_to_string(manifest_path(path)?).ok()?)?;
    let depends = manifest.get("depends").map(strings).unwrap_or_default();
    let auto_install = match manifest.get("auto_install") {
        Some(Expr::BooleanLiteral(b)) if b.value => Some(depends.clone()),
        Some(expr @ (Expr::List(_) | Expr::Tuple(_))) => Some(strings(expr)),
        _ => None,
    };
    let name = path.canonicalize().ok()?.file_name()?.to_string_lossy().into_owned();
    // All files first: a field may use a constant of another file.
    let files: Vec<PathBuf> = indexed_files(path).map(walkdir::DirEntry::into_path).collect();
    let parsed_files: Vec<(PathBuf, String, _)> = files
        .into_par_iter()
        .filter_map(|file| {
            let source = std::fs::read_to_string(&file).ok()?;
            let parsed = parse_module(&source).ok()?;
            Some((file, source, parsed))
        })
        .collect();
    let mut constants = Constants::default();
    for (file, _, parsed) in &parsed_files {
        constants.add(file, parsed.suite());
    }
    let per_file: Vec<Vec<(String, ModelClass)>> = parsed_files
        .par_iter()
        .map(|(file, source, parsed)| {
            let lines = LineIndex::from_source_text(source);
            let line_of = |offset: usize| lines.line_index(text_size(offset)).get();
            let mut found: Vec<(String, ModelClass)> = classes(parsed.suite())
                .into_iter()
                .filter_map(|class| {
                    let eval = |expr: &Expr| constants.eval(file, Some(class), expr, 0);
                    let mut model = model_class(class, &eval)?;
                    model.file = file.clone();
                    model.line = line_of(class.name.start().to_usize());
                    for field in &mut model.fields {
                        field.line = line_of(field.line);
                    }
                    for (_, line) in &mut model.method_lines {
                        *line = line_of(*line);
                    }
                    Some((class.name.to_string(), model))
                })
                .collect();
            // `setattr(IrRule, 'global', global_)`: a field whose name is a
            // Python keyword, added after the class.
            for stmt in parsed.suite() {
                let Stmt::Expr(expr) = stmt else { continue };
                let Expr::Call(call) = &*expr.value else { continue };
                if !matches!(&*call.func, Expr::Name(n) if n.id.as_str() == "setattr") {
                    continue;
                }
                let [Expr::Name(class), name, _] = call.arguments.args.as_ref() else {
                    continue;
                };
                let Some(name) = string(name) else { continue };
                if let Some((_, model)) = found.iter_mut().find(|(c, _)| c == class.id.as_str()) {
                    model.fields.push(Field {
                        name,
                        kind: String::new(),
                        comodel: None,
                        has_domain: false,
                        groups: None,
                        line: 0,
                        label: FieldLabel::default(),
                    });
                }
            }
            found
        })
        .collect();
    // The Python name of each class in `model_classes`.
    let (class_names, mut model_classes): (Vec<String>, Vec<ModelClass>) = per_file.into_iter().flatten().unzip();
    // `class Child(Parent)` where `Parent` is a model class of the module.
    loop {
        let models: HashSet<&str> = class_names
            .iter()
            .zip(&model_classes)
            .filter(|(_, m)| m.odoo_model)
            .map(|(n, _)| n.as_str())
            .collect();
        let children: Vec<usize> = (0..model_classes.len())
            .filter(|&i| {
                let m = &model_classes[i];
                !m.odoo_model && m.python_bases.iter().any(|b| models.contains(b.as_str()))
            })
            .collect();
        if children.is_empty() {
            break;
        }
        for i in children {
            model_classes[i].odoo_model = true;
        }
    }
    let data_files = data_files(path, &manifest);
    let (xml_ids, xml_id_lines) = xml_ids(&name, path, &data_files, &model_classes);
    let views = views(&name, &data_files);
    let (access, implied) = security(&name, &data_files);
    Some(ModuleIndex {
        xml_ids,
        xml_id_lines,
        views,
        data_files,
        name,
        path: path.to_path_buf(),
        depends,
        auto_install,
        classes: model_classes,
        access,
        implied,
    })
}

/// The index of the module at `path`, parsed once per run.
pub fn module_index(path: &Path) -> Option<Arc<ModuleIndex>> {
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let revalidate = REVALIDATE.load(Ordering::Relaxed);
    let generation = GENERATION.load(Ordering::Relaxed);
    // A closure is built many times per lint run: walk the files once.
    if let Some((found, _, checked)) = CACHE.lock().expect("index cache").get(&key) {
        if !revalidate || *checked == generation {
            return found.clone();
        }
    }
    // The manifest counts too: `depends` live there.
    let current = revalidate.then(|| {
        let mut print = fingerprint(&key);
        print.files += 1;
        let manifest = manifest_path(&key);
        let modified = manifest.and_then(|m| m.metadata().ok()).and_then(|m| m.modified().ok());
        print.newest = print.newest.max(modified);
        print
    });
    if let Some((found, print, checked)) = CACHE.lock().expect("index cache").get_mut(&key) {
        if current.is_none_or(|current| current == *print) {
            *checked = generation;
            return found.clone();
        }
    }
    let parsed = parse(&key).map(Arc::new);
    CACHE
        .lock()
        .expect("index cache")
        .insert(key, (parsed.clone(), current.unwrap_or_default(), generation));
    parsed
}

/// The index of module `name` in the first of `dirs` that has it.
pub fn find_module(dirs: &[PathBuf], name: &str) -> Option<Arc<ModuleIndex>> {
    module_index(&find_module_dir(dirs, name)?)
}

/// The modules in `dirs` whose Python mentions every one of `words`, in
/// addons path order. A model or field is declared with its name in the
/// source, so a text search rules out most of the addons path before any
/// module is parsed.
pub fn modules_mentioning(dirs: &[PathBuf], words: &[&str]) -> Vec<Arc<ModuleIndex>> {
    module_dirs(dirs)
        .par_iter()
        .filter(|path| mentions(path, words))
        .filter_map(|path| module_index(path))
        .collect()
}

fn mentions(module: &Path, words: &[&str]) -> bool {
    let mut missing: Vec<&str> = words.to_vec();
    for file in indexed_files(module) {
        let Ok(source) = std::fs::read_to_string(file.path()) else {
            continue;
        };
        missing.retain(|word| !source.contains(word));
        if missing.is_empty() {
            return true;
        }
    }
    false
}

/// The modules in `dirs` that give `model` a field `field`: where to look
/// when a module uses a field its dependencies do not have.
pub fn modules_defining(dirs: &[PathBuf], model: &str, field: &str) -> Vec<String> {
    let mut found: Vec<String> = modules_mentioning(dirs, &[model, field])
        .into_iter()
        .filter(|module| {
            module.classes.iter().any(|class| {
                let on_model = class.name.as_deref() == Some(model)
                    || (class.name.is_none() && class.inherit.iter().any(|i| i == model));
                on_model && class.fields.iter().any(|f| f.name == field)
            })
        })
        .map(|module| module.name.clone())
        .collect();
    found.sort();
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_revalidated_cache_sees_new_fields() {
        let dir = tempfile::tempdir().unwrap();
        let module = dir.path().join("acme_idx");
        std::fs::create_dir_all(&module).unwrap();
        std::fs::write(module.join("__manifest__.py"), "{'name': 'Idx'}\n").unwrap();
        let model = "from odoo import fields, models\n\n\nclass P(models.Model):\n    _name = 'acme.p'\n";
        std::fs::write(module.join("a.py"), format!("{model}\n    a = fields.Char()\n")).unwrap();
        revalidate_cache();
        let fields = |index: &ModuleIndex| -> Vec<String> {
            index
                .classes
                .iter()
                .flat_map(|c| c.fields.iter().map(|f| f.name.clone()))
                .collect()
        };
        assert_eq!(fields(&module_index(&module).unwrap()), vec!["a"]);
        std::fs::write(module.join("b.py"), format!("{model}\n    b = fields.Char()\n")).unwrap();
        // Checked once per generation: the server starts one per message.
        revalidate_cache();
        let mut found = fields(&module_index(&module).unwrap());
        found.sort();
        assert_eq!(found, vec!["a", "b"]);
    }

    #[test]
    fn fields_of_a_module_field_class() {
        let dir = tempfile::tempdir().unwrap();
        let module = dir.path().join("acme_job");
        std::fs::create_dir_all(&module).unwrap();
        std::fs::write(module.join("__manifest__.py"), "{'name': 'Job'}\n").unwrap();
        std::fs::write(
            module.join("a.py"),
            "from odoo import fields, models\n\nfrom .fields import JobSerialized\n\n\nclass F(models.Model):\n    _name = 'acme.f'\n\n    MAX = Decimal(1)\n    _cache = LRU(10)\n    action = JobSerialized(base_type=dict)\n    folder = fs_fields.FsFolder()\n    name = fields.Char()\n",
        )
        .unwrap();
        let index = module_index(&module).unwrap();
        let fields: Vec<(&str, &str)> = index.classes[0]
            .fields
            .iter()
            .map(|f| (f.name.as_str(), f.kind.as_str()))
            .collect();
        assert_eq!(
            fields,
            vec![("action", "JobSerialized"), ("folder", "FsFolder"), ("name", "Char")]
        );
    }

    #[test]
    fn inherits_keyed_by_a_class_attribute() {
        let dir = tempfile::tempdir().unwrap();
        let module = dir.path().join("acme_line");
        std::fs::create_dir_all(&module).unwrap();
        std::fs::write(module.join("__manifest__.py"), "{'name': 'Line'}\n").unwrap();
        std::fs::write(
            module.join("a.py"),
            "from odoo import fields, models\n\n\nclass L(models.Model):\n    _name = 'account.move.line'\n    _decorator = 'acme.document.line'\n    _inherit = [_name, 'acme.mixin']\n    _inherits = {_decorator: 'document_line_id'}\n    partner_id = fields.Many2one(comodel_name='res.partner', delegate=True)\n",
        )
        .unwrap();
        let index = module_index(&module).unwrap();
        assert_eq!(
            index.classes[0].delegates,
            vec!["acme.document.line".to_string(), "res.partner".to_string()]
        );
    }
}
