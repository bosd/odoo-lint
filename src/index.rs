//! A model of the addons path: per module, its dependencies and the models
//! and fields its Python code declares. Checks across modules (fields in
//! views, ...) ask it what a module can rely on: the fields of a model as
//! the modules its `depends` reach define them.
//!
//! Modules are parsed on first use and cached for the run, so only the
//! dependencies of the checked modules are read, not all of Odoo. A hint
//! about where a missing model or field lives searches the whole addons
//! path, but parses only the modules whose Python mentions it.

use crate::checker::ModuleInfo;
use crate::manifest::{find_module_dir, manifest_path, module_dirs, Manifest, LOADED_KEYS};
use crate::rules::python::classes;
use crate::semantic::func_lib;
use rayon::prelude::*;
use ruff_python_ast::{Expr, Stmt, StmtClassDef};
use ruff_python_parser::parse_module;
use ruff_source_file::LineIndex;
use ruff_text_size::{Ranged, TextSize};
use std::collections::{BTreeMap, HashMap, HashSet};
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

fn string(expr: &Expr) -> Option<String> {
    expr.as_string_literal_expr().map(|s| s.value.to_str().to_string())
}

/// A value a field definition uses, as far as it can be known without
/// running the code.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Literal {
    Str(String),
    Bool(bool),
    /// A list or a tuple.
    Seq(Vec<Literal>),
    /// Anything else: a call, a method, a number, ...
    Other,
}

impl Literal {
    fn into_string(self) -> Option<String> {
        match self {
            Self::Str(value) => Some(value),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// `(value, label)` pairs of a selection; values that are not text are
    /// left out. `None` when it is not a list.
    fn into_pairs(self) -> Option<Vec<(String, Option<String>)>> {
        let Self::Seq(items) = self else { return None };
        Some(
            items
                .into_iter()
                .filter_map(|item| {
                    let Self::Seq(pair) = item else { return None };
                    let mut pair = pair.into_iter();
                    let value = pair.next()?.into_string()?;
                    Some((value, pair.next().and_then(Self::into_string)))
                })
                .collect(),
        )
    }
}

/// Where a field class takes `string` as a positional argument.
fn string_position(kind: &str) -> usize {
    match kind {
        "Many2one" | "Selection" | "Reference" => 1,
        "One2many" => 2,
        "Many2many" => 4,
        _ => 0,
    }
}

/// What a class body sees of an expression.
type Eval<'e> = dyn Fn(&Expr) -> Literal + 'e;

fn field_label(kind: &str, call: &ruff_python_ast::ExprCall, eval: &Eval) -> FieldLabel {
    let keyword = |name: &str| {
        call.arguments
            .keywords
            .iter()
            .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == name))
            .map(|k| eval(&k.value))
    };
    let positional = |at: usize| call.arguments.args.get(at).map(eval);
    let selection = matches!(kind, "Selection" | "Reference")
        .then(|| keyword("selection").or_else(|| positional(0)))
        .flatten()
        .map(|literal| match literal.into_pairs() {
            Some(pairs) => Selection::List(
                pairs
                    .into_iter()
                    .map(|(value, label)| (value, label.unwrap_or_default()))
                    .collect(),
            ),
            None => Selection::Dynamic,
        });
    FieldLabel {
        string: keyword("string")
            .or_else(|| positional(string_position(kind)))
            .and_then(Literal::into_string),
        help: keyword("help").and_then(Literal::into_string),
        related: keyword("related").and_then(Literal::into_string),
        selection,
        selection_add: keyword("selection_add")
            .and_then(Literal::into_pairs)
            .unwrap_or_default(),
        export_string_translation: keyword("export_string_translation").and_then(|l| l.as_bool()),
        translate: call
            .arguments
            .keywords
            .iter()
            .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "translate"))
            .and_then(|k| translate_mode(&k.value)),
        sanitize: keyword("sanitize").and_then(|value| match value {
            Literal::Bool(true) => Some(Sanitize::Yes),
            Literal::Bool(false) => Some(Sanitize::No),
            Literal::Str(mode) if mode == "email_outgoing" => Some(Sanitize::EmailOutgoing),
            Literal::Str(_) => Some(Sanitize::Yes),
            _ => None,
        }),
    }
}

/// The translation mode a `translate=` value names.
fn translate_mode(expr: &Expr) -> Option<Translate> {
    let name = match expr {
        Expr::BooleanLiteral(b) => return Some(if b.value { Translate::Whole } else { Translate::No }),
        Expr::Name(name) => name.id.as_str(),
        Expr::Attribute(attribute) => attribute.attr.as_str(),
        _ => return None,
    };
    match name {
        "xml_translate" => Some(Translate::Xml),
        "html_translate" => Some(Translate::Html),
        _ => None,
    }
}

/// The constraints of a class: `_sql_constraints` entries and
/// `models.Constraint` / `models.UniqueIndex` attributes.
fn class_constraints(class: &StmtClassDef, eval: &Eval) -> Vec<Constraint> {
    let mut found = Vec::new();
    if let Some(Literal::Seq(entries)) = class_value(class, "_sql_constraints").map(eval) {
        for entry in entries {
            let Literal::Seq(entry) = entry else { continue };
            let mut entry = entry.into_iter();
            let Some(name) = entry.next().and_then(Literal::into_string) else {
                continue;
            };
            found.push(Constraint {
                name,
                message: entry.nth(1).and_then(Literal::into_string),
                table_object: false,
            });
        }
    }
    for stmt in &class.body {
        let Stmt::Assign(assign) = stmt else { continue };
        let [Expr::Name(target)] = assign.targets.as_slice() else {
            continue;
        };
        let Some(name) = target.id.strip_prefix('_') else {
            continue;
        };
        let Expr::Call(call) = &*assign.value else { continue };
        let Expr::Attribute(func) = &*call.func else { continue };
        if !matches!(func.attr.as_str(), "Constraint" | "UniqueIndex") {
            continue;
        }
        let message = call
            .arguments
            .keywords
            .iter()
            .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "message"))
            .map(|k| &k.value)
            .or_else(|| call.arguments.args.get(1))
            .map(eval)
            .and_then(Literal::into_string);
        found.push(Constraint {
            name: name.to_string(),
            message,
            table_object: true,
        });
    }
    found
}

fn strings(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::StringLiteral(_) => string(expr).into_iter().collect(),
        Expr::List(list) => list.elts.iter().filter_map(string).collect(),
        Expr::Tuple(tuple) => tuple.elts.iter().filter_map(string).collect(),
        _ => Vec::new(),
    }
}

/// The value assigned to `name` in a class body.
fn class_value<'a>(class: &'a StmtClassDef, name: &str) -> Option<&'a Expr> {
    class.body.iter().rev().find_map(|stmt| match stmt {
        Stmt::Assign(assign)
            if assign
                .targets
                .iter()
                .any(|t| t.as_name_expr().is_some_and(|n| n.id.as_str() == name)) =>
        {
            Some(&*assign.value)
        }
        _ => None,
    })
}

/// A string literal, or the name of a string the class body assigns
/// (`_inherits = {_decorator_model: "line_id"}`).
fn class_string(class: &StmtClassDef, expr: &Expr) -> Option<String> {
    string(expr).or_else(|| {
        let name = expr.as_name_expr()?;
        class_value(class, name.id.as_str()).and_then(string)
    })
}

/// The models a class defines or extends: its `_name`, or every model of
/// its `_inherit`.
pub(crate) fn class_models(class: &StmtClassDef) -> Vec<String> {
    if let Some(name) = class_value(class, "_name").and_then(string) {
        return vec![name];
    }
    class_value(class, "_inherit").map(strings).unwrap_or_default()
}

/// The module-level names of a module's Python files, so that a field can
/// use a constant (`selection=STATES`), also one of another file of the
/// module (`from .stock import STATES`, `stock.STATES`).
#[derive(Default)]
struct Constants<'a> {
    assigned: HashMap<PathBuf, HashMap<&'a str, &'a Expr>>,
    /// `from .x import NAME [as ALIAS]`: alias to (file, name).
    imported: HashMap<PathBuf, HashMap<&'a str, (PathBuf, &'a str)>>,
    /// `from . import x`: alias to the file of module `x`.
    modules: HashMap<PathBuf, HashMap<&'a str, PathBuf>>,
}

impl<'a> Constants<'a> {
    fn add(&mut self, file: &Path, suite: &'a [Stmt]) {
        let mut assigned = HashMap::new();
        let mut imported = HashMap::new();
        let mut modules = HashMap::new();
        for stmt in suite {
            match stmt {
                Stmt::Assign(assign) => {
                    for target in &assign.targets {
                        if let Expr::Name(name) = target {
                            assigned.insert(name.id.as_str(), &*assign.value);
                        }
                    }
                }
                Stmt::ImportFrom(import) if import.level > 0 => {
                    let package = import.module.as_ref().map(|m| m.as_str());
                    for alias in &import.names {
                        let local = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                        let submodule = match package {
                            Some(package) => format!("{package}.{}", alias.name),
                            None => alias.name.to_string(),
                        };
                        if let Some(module_file) =
                            relative_file(file, import.level, Some(&submodule)).filter(|f| f.is_file())
                        {
                            modules.insert(local, module_file);
                        } else if let Some(target) = relative_file(file, import.level, package) {
                            imported.insert(local, (target, alias.name.as_str()));
                        }
                    }
                }
                _ => {}
            }
        }
        self.assigned.insert(file.to_path_buf(), assigned);
        self.imported.insert(file.to_path_buf(), imported);
        self.modules.insert(file.to_path_buf(), modules);
    }

    /// The value of `expr` in `file`, inside `class` when given: literals,
    /// names of the class body, the file or an imported file, `_()` and
    /// lists added together.
    fn eval(&self, file: &Path, class: Option<&StmtClassDef>, expr: &Expr, depth: usize) -> Literal {
        if depth > 8 {
            return Literal::Other;
        }
        let deeper = |file: &Path, expr: &Expr| self.eval(file, None, expr, depth + 1);
        match expr {
            Expr::StringLiteral(literal) => Literal::Str(literal.value.to_str().to_string()),
            Expr::BooleanLiteral(literal) => Literal::Bool(literal.value),
            Expr::List(list) => Literal::Seq(list.elts.iter().map(|e| self.eval(file, class, e, depth + 1)).collect()),
            Expr::Tuple(tuple) => Literal::Seq(
                tuple
                    .elts
                    .iter()
                    .map(|e| self.eval(file, class, e, depth + 1))
                    .collect(),
            ),
            Expr::BinOp(binary) if binary.op == ruff_python_ast::Operator::Add => {
                match (
                    self.eval(file, class, &binary.left, depth + 1),
                    self.eval(file, class, &binary.right, depth + 1),
                ) {
                    (Literal::Seq(mut left), Literal::Seq(right)) => {
                        left.extend(right);
                        Literal::Seq(left)
                    }
                    (Literal::Str(left), Literal::Str(right)) => Literal::Str(left + &right),
                    _ => Literal::Other,
                }
            }
            Expr::Call(call) if matches!(&*call.func, Expr::Name(f) if matches!(f.id.as_str(), "_" | "_lt")) => call
                .arguments
                .args
                .first()
                .map_or(Literal::Other, |arg| self.eval(file, class, arg, depth + 1)),
            Expr::Name(name) => {
                let id = name.id.as_str();
                if let Some(value) = class.and_then(|c| class_value(c, id)) {
                    return self.eval(file, class, value, depth + 1);
                }
                if let Some(value) = self.assigned.get(file).and_then(|a| a.get(id)) {
                    return deeper(file, value);
                }
                if let Some((target, original)) = self.imported.get(file).and_then(|i| i.get(id)) {
                    if let Some(value) = self.assigned.get(target).and_then(|a| a.get(original)) {
                        return deeper(target, value);
                    }
                }
                Literal::Other
            }
            Expr::Attribute(attribute) => {
                let Expr::Name(module) = &*attribute.value else {
                    return Literal::Other;
                };
                let Some(target) = self.modules.get(file).and_then(|m| m.get(module.id.as_str())) else {
                    return Literal::Other;
                };
                match self.assigned.get(target).and_then(|a| a.get(attribute.attr.as_str())) {
                    Some(value) => deeper(target, value),
                    None => Literal::Other,
                }
            }
            _ => Literal::Other,
        }
    }
}

/// The file a relative import names: `from ..models.stock import X` in
/// `wizards/a.py` is `models/stock.py` (or `models/stock/__init__.py`).
fn relative_file(file: &Path, level: u32, module: Option<&str>) -> Option<PathBuf> {
    let mut dir = file.parent()?.to_path_buf();
    for _ in 1..level {
        dir = dir.parent()?.to_path_buf();
    }
    let module = module?;
    let base = module.split('.').fold(dir, |path, part| path.join(part));
    let file = base.with_extension("py");
    if file.is_file() {
        Some(file)
    } else {
        Some(base.join("__init__.py"))
    }
}

/// The model class `class` declares, if any; `eval` gives the values its
/// fields use.
fn model_class(class: &StmtClassDef, eval: &Eval) -> Option<ModelClass> {
    let name = class_value(class, "_name").and_then(string);
    let inherit = class_value(class, "_inherit").map(strings).unwrap_or_default();
    if name.is_none() && inherit.is_empty() {
        return None;
    }
    let mut delegates: Vec<String> = match class_value(class, "_inherits") {
        Some(Expr::Dict(dict)) => dict
            .items
            .iter()
            .filter_map(|i| i.key.as_ref().and_then(|key| class_string(class, key)))
            .collect(),
        _ => Vec::new(),
    };
    // `name = fields.X(...)` and `name: Type = fields.X(...)`.
    let fields = class
        .body
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Assign(assign) => match assign.targets.as_slice() {
                [Expr::Name(target)] => Some((target, &*assign.value)),
                _ => None,
            },
            Stmt::AnnAssign(assign) => match (&*assign.target, assign.value.as_deref()) {
                (Expr::Name(target), Some(value)) => Some((target, value)),
                _ => None,
            },
            _ => None,
        })
        .filter_map(|(target, value)| {
            let Expr::Call(call) = value else { return None };
            let kind = match &*call.func {
                Expr::Attribute(func) if func_lib(&call.func) == "fields" => func.attr.to_string(),
                // A module's own field classes: `fs_fields.FsFolder()`.
                Expr::Attribute(func)
                    if func_lib(&call.func).ends_with("fields")
                        && func.attr.starts_with(|c: char| c.is_ascii_uppercase()) =>
                {
                    func.attr.to_string()
                }
                // A field class of a module (`JobSerialized(...)` of
                // queue_job), imported by name; constants aside.
                Expr::Name(func)
                    if func.id.starts_with(|c: char| c.is_ascii_uppercase())
                        && !target.id.starts_with('_')
                        && target.id.chars().any(|c| c.is_ascii_lowercase()) =>
                {
                    func.id.to_string()
                }
                _ => return None,
            };
            let comodel = call
                .arguments
                .keywords
                .iter()
                .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "comodel_name"))
                .map(|k| &k.value)
                .or_else(|| call.arguments.args.first())
                .filter(|_| matches!(kind.as_str(), "Many2one" | "One2many" | "Many2many"))
                .and_then(string);
            let has_domain = call
                .arguments
                .keywords
                .iter()
                .any(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "domain"));
            let groups = call
                .arguments
                .keywords
                .iter()
                .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "groups"))
                .and_then(|k| string(&k.value));
            let label = field_label(&kind, call, eval);
            Some(Field {
                name: target.id.to_string(),
                kind,
                comodel,
                has_domain,
                groups,
                // A byte offset until `parse` turns it into a line.
                line: target.start().to_usize(),
                label,
            })
        })
        .collect::<Vec<Field>>();
    // `fields.Many2one(..., delegate=True)` is an `_inherits` too.
    for stmt in &class.body {
        let value = match stmt {
            Stmt::Assign(assign) => Some(&*assign.value),
            Stmt::AnnAssign(assign) => assign.value.as_deref(),
            _ => None,
        };
        let Some(Expr::Call(call)) = value else { continue };
        let delegate = call.arguments.keywords.iter().any(|k| {
            k.arg.as_ref().is_some_and(|a| a.as_str() == "delegate")
                && matches!(&k.value, Expr::BooleanLiteral(b) if b.value)
        });
        let comodel = call
            .arguments
            .keywords
            .iter()
            .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "comodel_name"))
            .map(|k| &k.value)
            .or_else(|| call.arguments.args.first());
        if let Some(comodel) = comodel.and_then(string).filter(|_| delegate) {
            delegates.push(comodel);
        }
    }
    let methods = class
        .body
        .iter()
        .flat_map(|stmt| match stmt {
            Stmt::FunctionDef(function) => vec![function.name.to_string()],
            Stmt::Assign(assign) => assign
                .targets
                .iter()
                .filter_map(|t| t.as_name_expr().map(|n| n.id.to_string()))
                .collect(),
            _ => Vec::new(),
        })
        .collect();
    // Byte offsets until `parse` turns them into lines.
    let method_lines = class
        .body
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::FunctionDef(function) => Some((function.name.to_string(), function.name.start().to_usize())),
            _ => None,
        })
        .collect();
    let foreign_bases = class.arguments.as_ref().is_some_and(|a| {
        a.args.iter().any(|b| {
            !crate::semantic::dotted_name(b)
                .is_some_and(|d| d.ends_with("Model") || d.ends_with("AbstractModel") || d.ends_with("TransientModel"))
        })
    });
    let abstract_model = class.arguments.as_ref().is_some_and(|a| {
        a.args
            .iter()
            .any(|b| crate::semantic::dotted_name(b).is_some_and(|d| d.ends_with("AbstractModel")))
    });
    let odoo_model = class.arguments.as_ref().is_some_and(|a| {
        a.args.iter().any(|b| {
            crate::semantic::dotted_name(b)
                .is_some_and(|d| d.ends_with("Model") || d.ends_with("AbstractModel") || d.ends_with("TransientModel"))
        })
    });
    Some(ModelClass {
        odoo_model,
        abstract_model,
        file: PathBuf::new(),
        line: 0,
        name,
        inherit,
        delegates,
        fields,
        methods,
        method_lines,
        foreign_bases,
        description: class_value(class, "_description")
            .map(eval)
            .and_then(Literal::into_string),
        table: class_value(class, "_table").and_then(string),
        auto: class_value(class, "_auto").and_then(|e| eval(e).as_bool()),
        log_access: class_value(class, "_log_access").and_then(|e| eval(e).as_bool()),
        constraints: class_constraints(class, eval),
    })
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
    let per_file: Vec<Vec<ModelClass>> = parsed_files
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
            found.into_iter().map(|(_, model)| model).collect()
        })
        .collect();
    let model_classes: Vec<ModelClass> = per_file.into_iter().flatten().collect();
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

/// The modules a module can rely on: itself and everything its `depends`
/// reach, with the fields of their models.
/// The method names of a model.
pub type MethodSet = HashSet<String>;

pub struct Closure {
    modules: Vec<Arc<ModuleIndex>>,
    fields: Mutex<HashMap<String, Option<Arc<FieldMap>>>>,
    methods: Mutex<HashMap<String, Option<Arc<MethodSet>>>>,
    implied: std::sync::OnceLock<HashMap<String, HashSet<String>>>,
}

/// The methods of Odoo's model classes (`BaseModel`, `Model`, ...), from
/// the Odoo source around `base` (`odoo/addons/base`): `odoo/models.py`, or
/// the files of `odoo/orm` from 19.0.
fn base_model_methods(base: &Path) -> Option<Arc<HashSet<String>>> {
    type Methods = Option<Arc<HashSet<String>>>;
    static CACHE: LazyLock<Mutex<HashMap<PathBuf, Methods>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
    let odoo = base.canonicalize().ok()?.parent()?.parent()?.to_path_buf();
    if let Some(found) = CACHE.lock().expect("base methods cache").get(&odoo) {
        return found.clone();
    }
    let mut files = vec![odoo.join("models.py")];
    if let Ok(entries) = std::fs::read_dir(odoo.join("orm")) {
        files.extend(
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "py")),
        );
    }
    let mut methods = HashSet::new();
    let mut found = false;
    for file in files {
        let Ok(source) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(parsed) = parse_module(&source) else { continue };
        for stmt in parsed.suite() {
            let Stmt::ClassDef(class) = stmt else { continue };
            if !matches!(
                class.name.as_str(),
                "BaseModel" | "Model" | "AbstractModel" | "TransientModel"
            ) {
                continue;
            }
            found = true;
            for item in &class.body {
                match item {
                    Stmt::FunctionDef(function) => {
                        methods.insert(function.name.to_string());
                    }
                    Stmt::Assign(assign) => {
                        methods.extend(
                            assign
                                .targets
                                .iter()
                                .filter_map(|t| t.as_name_expr().map(|n| n.id.to_string())),
                        );
                    }
                    _ => {}
                }
            }
        }
    }
    let result = found.then(|| Arc::new(methods));
    CACHE.lock().expect("base methods cache").insert(odoo, result.clone());
    result
}

/// The closure of `module`, looking up dependencies next to it and in
/// `addons_path`. `None` when a dependency cannot be found: the checks then
/// know too little to report anything.
pub fn closure(module: &ModuleInfo, addons_path: &[PathBuf]) -> Option<Closure> {
    closure_at(&module.path, addons_path)
}

/// [`closure`] of the module in the folder `path`.
pub fn closure_at(path: &Path, addons_path: &[PathBuf]) -> Option<Closure> {
    let modules = dependencies(path, addons_path, true)?;
    Some(Closure {
        modules,
        fields: Mutex::new(HashMap::new()),
        methods: Mutex::new(HashMap::new()),
        implied: std::sync::OnceLock::new(),
    })
}

/// A closure of the given modules, which must include their dependencies.
pub fn closure_of(modules: Vec<Arc<ModuleIndex>>) -> Closure {
    Closure {
        modules,
        fields: Mutex::new(HashMap::new()),
        methods: Mutex::new(HashMap::new()),
        implied: std::sync::OnceLock::new(),
    }
}

/// The module in `path` and the modules it depends on, directly or not,
/// looked up next to it and in `addons_path`. A dependency that cannot be
/// found gives `None` when `strict`, and is skipped otherwise.
pub fn dependencies(path: &Path, addons_path: &[PathBuf], strict: bool) -> Option<Vec<Arc<ModuleIndex>>> {
    let (modules, missing) = walk_dependencies(path, addons_path)?;
    (!strict || missing.is_empty()).then_some(modules)
}

/// The dependencies of the module in `path`, directly or not, that are
/// neither next to it nor in `addons_path`: without them the checks across
/// modules skip it.
pub fn missing_dependencies(path: &Path, addons_path: &[PathBuf]) -> Vec<String> {
    walk_dependencies(path, addons_path)
        .map(|(_, missing)| missing)
        .unwrap_or_default()
}

fn walk_dependencies(path: &Path, addons_path: &[PathBuf]) -> Option<(Vec<Arc<ModuleIndex>>, Vec<String>)> {
    let mut dirs: Vec<PathBuf> = path.parent().map(Path::to_path_buf).into_iter().collect();
    dirs.extend(addons_path.iter().cloned());
    let find = |name: &str| find_module(&dirs, name);
    let root = module_index(path)?;
    let mut seen: HashSet<String> = HashSet::from([root.name.clone()]);
    let mut queue: Vec<String> = root.depends.clone();
    // Odoo loads `base` and `web` (a server-wide module) before any other.
    queue.push("base".into());
    if root.name != "base" {
        queue.push("web".into());
    }
    let mut modules = vec![root];
    let mut missing = Vec::new();
    // Level by level, each level's modules parsed in parallel.
    while !queue.is_empty() {
        let level: Vec<String> = queue.drain(..).filter(|name| seen.insert(name.clone())).collect();
        let found: Vec<(String, Option<Arc<ModuleIndex>>)> =
            level.into_par_iter().map(|name| (name.clone(), find(&name))).collect();
        for (name, found) in found {
            let Some(found) = found else {
                // Trees of only some modules (tests, `base` alone) lack `web`.
                if name != "web" {
                    missing.push(name);
                }
                continue;
            };
            queue.extend(found.depends.iter().cloned());
            modules.push(found);
        }
    }
    missing.sort();
    Some((modules, missing))
}

impl Closure {
    /// The groups each group implies, directly or not, itself included.
    fn implied_groups(&self) -> &HashMap<String, HashSet<String>> {
        self.implied.get_or_init(|| {
            let mut direct: HashMap<&str, Vec<&str>> = HashMap::new();
            for module in &self.modules {
                for (group, groups) in &module.implied {
                    direct
                        .entry(group.as_str())
                        .or_default()
                        .extend(groups.iter().map(String::as_str));
                }
            }
            let mut all: HashSet<&str> = direct.keys().copied().collect();
            all.extend(direct.values().flatten().copied());
            all.insert("base.group_user");
            all.into_iter()
                .map(|group| {
                    let mut found = HashSet::new();
                    let mut queue = vec![group];
                    while let Some(current) = queue.pop() {
                        if found.insert(current.to_string()) {
                            queue.extend(direct.get(current).into_iter().flatten());
                        }
                    }
                    (group.to_string(), found)
                })
                .collect()
        })
    }

    /// The read grants of `model` in the access rights of the closure: the
    /// group of each (none for everyone).
    fn read_grants(&self, model: &str) -> Option<Vec<Option<&str>>> {
        let id = format!("model_{}", model.replace('.', "_"));
        let mut known = false;
        let mut grants = Vec::new();
        for access in self.modules.iter().flat_map(|m| m.access.iter()) {
            if access.model.rsplit('.').next() != Some(id.as_str()) {
                continue;
            }
            known = true;
            if access.read {
                grants.push(access.group.as_deref());
            }
        }
        known.then_some(grants)
    }

    /// Whether every internal user may read `model`; `None` when no access
    /// right mentions it.
    pub fn users_can_read(&self, model: &str) -> Option<bool> {
        let implied = self.implied_groups();
        let users = implied.get("base.group_user")?;
        let grants = self.read_grants(model)?;
        Some(grants.iter().any(|g| g.is_none_or(|g| users.contains(g))))
    }

    /// Whether only administrators may read `model`: every group its access
    /// rights let read it implies Access Rights (`base.group_erp_manager`).
    pub fn only_admins_read(&self, model: &str) -> bool {
        let implied = self.implied_groups();
        self.read_grants(model).is_some_and(|grants| {
            grants.iter().all(|g| {
                g.and_then(|g| implied.get(g))
                    .is_some_and(|i| i.contains("base.group_erp_manager"))
            })
        })
    }

    /// Every view of the closure, by full XML id; for an id defined twice,
    /// the definition loaded last.
    pub fn views(&self) -> HashMap<String, (&View, usize)> {
        let ranks = self.module_ranks();
        let mut found: HashMap<String, (&View, usize)> = HashMap::new();
        for module in &self.modules {
            let rank = ranks.get(&module.name).copied().unwrap_or(usize::MAX);
            for view in &module.views {
                let key = (rank, view.position.rank, view.position.offset);
                match found.get(&view.id) {
                    Some((existing, existing_rank))
                        if (*existing_rank, existing.position.rank, existing.position.offset) > key => {}
                    _ => {
                        found.insert(view.id.clone(), (view, rank));
                    }
                }
            }
        }
        found
    }

    /// The load order of the closure's modules: dependencies first.
    pub fn module_ranks(&self) -> HashMap<String, usize> {
        fn visit(closure: &Closure, name: &str, visiting: &mut HashSet<String>, order: &mut Vec<String>) {
            // Done, or in progress (a dependency cycle).
            if order.iter().any(|s| s == name) || !visiting.insert(name.to_string()) {
                return;
            }
            if let Some(module) = closure.module(name) {
                let mut depends = module.depends.clone();
                if name != "base" {
                    depends.push("base".into());
                }
                for dependency in depends {
                    visit(closure, &dependency, visiting, order);
                }
                order.push(name.to_string());
            }
        }
        let (mut visiting, mut order) = (HashSet::new(), Vec::new());
        for module in &self.modules {
            visit(self, &module.name, &mut visiting, &mut order);
        }
        order.into_iter().enumerate().map(|(i, m)| (m, i)).collect()
    }

    /// The module `name` if the closure has it.
    /// The modules of the closure, the module itself included.
    pub fn modules(&self) -> &[Arc<ModuleIndex>] {
        &self.modules
    }

    pub fn module(&self, name: &str) -> Option<&Arc<ModuleIndex>> {
        self.modules.iter().find(|m| m.name == name)
    }

    /// Where a module of the closure creates the XML id `id` (`module.name`).
    pub fn xml_id(&self, id: &str) -> Option<Position> {
        self.modules.iter().find_map(|m| m.xml_ids.get(id).copied())
    }

    /// Whether a module of the closure other than `module` creates `id`:
    /// it then exists before `module` loads.
    pub fn xml_id_elsewhere(&self, id: &str, module: &str) -> bool {
        self.modules
            .iter()
            .any(|m| m.name != module && m.xml_ids.contains_key(id))
    }

    /// Whether `model` is an `AbstractModel`: a mixin, whose methods may
    /// name fields of the models that inherit it.
    pub fn is_abstract(&self, model: &str) -> bool {
        self.classes()
            .any(|c| c.name.as_deref() == Some(model) && c.abstract_model)
    }

    /// Where `model` is defined: the file and line of its class with `_name`.
    pub fn model_location(&self, model: &str) -> Option<(PathBuf, usize)> {
        self.classes()
            .find(|c| c.name.as_deref() == Some(model))
            .map(|c| (c.file.clone(), c.line))
    }

    /// Where `field` of `model` is defined: the class that defines the
    /// model first, then its extensions, then its parents.
    pub fn field_location(&self, model: &str, field: &str) -> Option<(PathBuf, usize)> {
        self.field_location_in(model, field, &mut HashSet::new())
    }

    fn field_location_in(&self, model: &str, field: &str, seen: &mut HashSet<String>) -> Option<(PathBuf, usize)> {
        if !seen.insert(model.to_string()) {
            return None;
        }
        let on_model = |c: &&ModelClass| {
            c.name.as_deref() == Some(model) || (c.name.is_none() && c.inherit.iter().any(|i| i == model))
        };
        let mut classes: Vec<&ModelClass> = self.classes().filter(on_model).collect();
        classes.sort_by_key(|c| c.name.is_none());
        for class in &classes {
            if let Some(found) = class.fields.iter().find(|f| f.name == field) {
                return Some((class.file.clone(), found.line));
            }
        }
        let parents: Vec<String> = classes
            .iter()
            .flat_map(|c| c.inherit.iter().chain(c.delegates.iter()))
            .filter(|p| *p != model)
            .cloned()
            .collect();
        parents
            .iter()
            .find_map(|parent| self.field_location_in(parent, field, seen))
    }

    /// Where the method `method` of `model` is defined: in the class that
    /// defines the model, else in an extension, else in a parent model.
    pub fn method_location(&self, model: &str, method: &str) -> Option<(PathBuf, usize)> {
        self.method_location_in(model, method, &mut HashSet::new())
    }

    fn method_location_in(&self, model: &str, method: &str, seen: &mut HashSet<String>) -> Option<(PathBuf, usize)> {
        if !seen.insert(model.to_string()) {
            return None;
        }
        let on_model = |c: &&ModelClass| {
            c.name.as_deref() == Some(model) || (c.name.is_none() && c.inherit.iter().any(|i| i == model))
        };
        let mut classes: Vec<&ModelClass> = self.classes().filter(on_model).collect();
        classes.sort_by_key(|c| c.name.is_none());
        for class in &classes {
            if let Some((_, line)) = class.method_lines.iter().find(|(name, _)| name == method) {
                return Some((class.file.clone(), *line));
            }
        }
        let parents: Vec<String> = classes
            .iter()
            .flat_map(|c| c.inherit.iter().chain(c.delegates.iter()))
            .filter(|p| *p != model)
            .cloned()
            .collect();
        parents
            .iter()
            .find_map(|parent| self.method_location_in(parent, method, seen))
    }

    /// Where the XML id `id` (`module.name`) is defined in a data file.
    pub fn xml_id_location(&self, id: &str) -> Option<(PathBuf, usize)> {
        self.modules.iter().find_map(|m| m.xml_id_lines.get(id).cloned())
    }

    /// Whether a module of the closure defines `model` (with `_name`).
    pub fn defines(&self, model: &str) -> bool {
        model == "base" || self.classes().any(|c| c.name.as_deref() == Some(model))
    }

    /// The fields of `model` in this closure; `None` when no module of the
    /// closure defines it (with `_name`).
    pub fn fields(&self, model: &str) -> Option<Arc<FieldMap>> {
        if let Some(found) = self.fields.lock().expect("fields cache").get(model) {
            return found.clone();
        }
        let computed = self.compute(model, &mut HashSet::new()).map(Arc::new);
        self.fields
            .lock()
            .expect("fields cache")
            .insert(model.to_string(), computed.clone());
        computed
    }

    fn classes(&self) -> impl Iterator<Item = &ModelClass> {
        self.modules.iter().flat_map(|m| m.classes.iter())
    }

    /// The methods of `model` in this closure, those of Odoo's `BaseModel`
    /// included; `None` when some are unknown: a model, a parent or a
    /// Python mixin outside the index, or no Odoo source next to `base`.
    pub fn methods(&self, model: &str) -> Option<Arc<HashSet<String>>> {
        if let Some(found) = self.methods.lock().expect("methods cache").get(model) {
            return found.clone();
        }
        let computed = self.compute_methods(model, &mut HashSet::new()).map(Arc::new);
        self.methods
            .lock()
            .expect("methods cache")
            .insert(model.to_string(), computed.clone());
        computed
    }

    /// The methods (`def`) of `model` that the closure shows, with the
    /// module defining each: its classes, those of its parents and, when an
    /// Odoo source is next to `base`, `BaseModel`'s. Unlike [`Self::methods`]
    /// a part that is unknown is left out rather than making it `None`.
    pub fn method_names(&self, model: &str) -> BTreeMap<String, String> {
        let mut found = BTreeMap::new();
        self.collect_method_names(model, &mut HashSet::new(), &mut found);
        if let Some(methods) = self.module("base").and_then(|base| base_model_methods(&base.path)) {
            for method in methods.iter() {
                found.entry(method.clone()).or_insert_with(|| "BaseModel".to_string());
            }
        }
        found
    }

    fn collect_method_names(&self, model: &str, visiting: &mut HashSet<String>, found: &mut BTreeMap<String, String>) {
        if !visiting.insert(model.to_string()) {
            return;
        }
        let mut parents: Vec<String> = Vec::new();
        for module in &self.modules {
            for class in &module.classes {
                let defines = class.name.as_deref() == Some(model);
                let extends = class.name.is_none() && class.inherit.iter().any(|i| i == model);
                if !defines && !extends {
                    continue;
                }
                for (method, _) in &class.method_lines {
                    found.entry(method.clone()).or_insert_with(|| module.name.clone());
                }
                parents.extend(class.inherit.iter().filter(|i| *i != model).cloned());
            }
        }
        for parent in parents {
            self.collect_method_names(&parent, visiting, found);
        }
    }

    fn compute_methods(&self, model: &str, visiting: &mut HashSet<String>) -> Option<HashSet<String>> {
        if !visiting.insert(model.to_string()) {
            return Some(HashSet::new());
        }
        let mut methods: HashSet<String> = HashSet::new();
        if model == "base" {
            let base = self.module("base")?;
            methods.extend(base_model_methods(&base.path)?.iter().cloned());
        } else if !self.classes().any(|c| c.name.as_deref() == Some(model)) {
            return None;
        }
        let mut parents: Vec<String> = Vec::new();
        for class in self.classes() {
            let defines = class.name.as_deref() == Some(model);
            let extends = class.name.is_none() && class.inherit.iter().any(|i| i == model);
            if !defines && !extends {
                continue;
            }
            if class.foreign_bases {
                return None;
            }
            methods.extend(class.methods.iter().cloned());
            parents.extend(class.inherit.iter().filter(|i| *i != model).cloned());
        }
        if model != "base" {
            parents.push("base".into());
        }
        for parent in parents {
            methods.extend(self.compute_methods(&parent, visiting)?);
        }
        Some(methods)
    }

    fn compute(&self, model: &str, visiting: &mut HashSet<String>) -> Option<HashMap<String, Field>> {
        if !visiting.insert(model.to_string()) {
            return Some(HashMap::new());
        }
        // `base` is every model; modules extend it without defining it.
        if model != "base" && !self.classes().any(|c| c.name.as_deref() == Some(model)) {
            visiting.remove(model);
            return None;
        }
        let mut fields: HashMap<String, Field> = HashMap::new();
        let mut parents: Vec<String> = Vec::new();
        for class in self.classes() {
            let defines = class.name.as_deref() == Some(model);
            let extends = class.name.is_none() && class.inherit.iter().any(|i| i == model);
            if !defines && !extends {
                continue;
            }
            for field in &class.fields {
                let mut field = field.clone();
                // A redefinition keeps the attributes it does not set.
                if field.groups.is_none() {
                    field.groups = fields.get(&field.name).and_then(|f| f.groups.clone());
                }
                fields.insert(field.name.clone(), field);
            }
            parents.extend(class.inherit.iter().filter(|i| *i != model).cloned());
            parents.extend(class.delegates.iter().cloned());
        }
        if model != "base" {
            parents.push("base".into());
        }
        for parent in parents {
            // A parent no module of the closure defines: its fields are unknown.
            let inherited = match self.compute(&parent, visiting) {
                Some(inherited) => inherited,
                None if parent == "base" => HashMap::new(),
                None => return None,
            };
            for (name, field) in inherited {
                fields.entry(name).or_insert(field);
            }
        }
        for name in MAGIC_FIELDS {
            fields.entry(name.to_string()).or_insert_with(|| Field {
                name: name.to_string(),
                kind: String::new(),
                comodel: None,
                has_domain: false,
                groups: None,
                line: 0,
                label: FieldLabel::default(),
            });
        }
        visiting.remove(model);
        Some(fields)
    }
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

fn data_files(path: &Path, manifest: &Manifest) -> Vec<DataFile> {
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

fn xml_ids(
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

fn security(module: &str, files: &[DataFile]) -> Security {
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
fn views(module: &str, files: &[DataFile]) -> Vec<View> {
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
