//! A model of the addons path: per module, its dependencies and the models
//! and fields its Python code declares. Checks across modules (fields in
//! views, ...) ask it what a module can rely on: the fields of a model as
//! the modules its `depends` reach define them.
//!
//! Modules are parsed on first use and cached for the run, so only the
//! dependencies of the checked modules are read, not all of Odoo.

use crate::checker::ModuleInfo;
use crate::manifest::{Manifest, MANIFEST_FILE_NAMES};
use crate::rules::python::classes;
use crate::semantic::func_lib;
use ruff_python_ast::{Expr, Stmt, StmtClassDef};
use ruff_python_parser::parse_module;
use ruff_text_size::Ranged;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// A module's index and the fingerprint of the files it was built from.
type Cached = (Option<Arc<ModuleIndex>>, Fingerprint);

static CACHE: LazyLock<Mutex<HashMap<PathBuf, Cached>>> = LazyLock::new(Default::default);

/// Whether cached modules are checked for changes: in a long-running
/// process (the language server) files change between runs.
static REVALIDATE: AtomicBool = AtomicBool::new(false);

/// Re-read a module whenever its Python files or manifest change, instead of
/// once per process.
pub fn revalidate_cache() {
    REVALIDATE.store(true, Ordering::Relaxed);
}

/// The number of indexed files of a module and their newest change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Fingerprint {
    files: usize,
    newest: Option<SystemTime>,
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

/// The models a class defines or extends: its `_name`, or every model of
/// its `_inherit`.
pub(crate) fn class_models(class: &StmtClassDef) -> Vec<String> {
    if let Some(name) = class_value(class, "_name").and_then(string) {
        return vec![name];
    }
    class_value(class, "_inherit").map(strings).unwrap_or_default()
}

fn model_class(class: &StmtClassDef) -> Option<ModelClass> {
    let name = class_value(class, "_name").and_then(string);
    let inherit = class_value(class, "_inherit").map(strings).unwrap_or_default();
    if name.is_none() && inherit.is_empty() {
        return None;
    }
    let mut delegates: Vec<String> = match class_value(class, "_inherits") {
        Some(Expr::Dict(dict)) => dict
            .items
            .iter()
            .filter_map(|i| i.key.as_ref().and_then(string))
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
            let Expr::Attribute(func) = &*call.func else {
                return None;
            };
            if func_lib(&call.func) != "fields" {
                return None;
            }
            let kind = func.attr.to_string();
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
            Some(Field {
                name: target.id.to_string(),
                kind,
                comodel,
                has_domain,
                groups,
                // A byte offset until `parse` turns it into a line.
                line: target.start().to_usize(),
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
        if let Some(comodel) = call.arguments.args.first().and_then(string).filter(|_| delegate) {
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
    Some(ModelClass {
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
    })
}

fn parse(path: &Path) -> Option<ModuleIndex> {
    let manifest_path = MANIFEST_FILE_NAMES.iter().map(|n| path.join(n)).find(|p| p.is_file())?;
    let manifest = Manifest::parse(&std::fs::read_to_string(manifest_path).ok()?)?;
    let depends = manifest.get("depends").map(strings).unwrap_or_default();
    let name = path.canonicalize().ok()?.file_name()?.to_string_lossy().into_owned();
    let mut model_classes = Vec::new();
    for file in indexed_files(path) {
        let Ok(source) = std::fs::read_to_string(file.path()) else {
            continue;
        };
        let Ok(parsed) = parse_module(&source) else { continue };
        let line_of = |offset: usize| source[..offset].matches('\n').count() + 1;
        let mut found: Vec<(String, ModelClass)> = classes(parsed.suite())
            .into_iter()
            .filter_map(|class| {
                let mut model = model_class(class)?;
                model.file = file.path().to_path_buf();
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
                });
            }
        }
        model_classes.extend(found.into_iter().map(|(_, model)| model));
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
        classes: model_classes,
        access,
        implied,
    })
}

/// The index of the module at `path`, parsed once per run.
pub fn module_index(path: &Path) -> Option<Arc<ModuleIndex>> {
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let revalidate = REVALIDATE.load(Ordering::Relaxed);
    // The manifest counts too: `depends` live there.
    let current = revalidate.then(|| {
        let mut print = fingerprint(&key);
        print.files += 1;
        let manifest = MANIFEST_FILE_NAMES.iter().map(|n| key.join(n)).find(|p| p.is_file());
        let modified = manifest.and_then(|m| m.metadata().ok()).and_then(|m| m.modified().ok());
        print.newest = print.newest.max(modified);
        print
    });
    if let Some((found, print)) = CACHE.lock().expect("index cache").get(&key) {
        if current.is_none_or(|current| current == *print) {
            return found.clone();
        }
    }
    let parsed = parse(&key).map(Arc::new);
    CACHE
        .lock()
        .expect("index cache")
        .insert(key, (parsed.clone(), current.unwrap_or_default()));
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
    let mut dirs: Vec<PathBuf> = path.parent().map(Path::to_path_buf).into_iter().collect();
    dirs.extend(addons_path.iter().cloned());
    let find = |name: &str| {
        dirs.iter()
            .map(|dir| dir.join(name))
            .find(|path| MANIFEST_FILE_NAMES.iter().any(|m| path.join(m).is_file()))
            .and_then(|path| module_index(&path))
    };
    let root = module_index(path)?;
    let mut seen: HashSet<String> = HashSet::from([root.name.clone()]);
    let mut queue: Vec<String> = root.depends.clone();
    queue.push("base".into());
    let mut modules = vec![root];
    while let Some(name) = queue.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let found = find(&name)?;
        queue.extend(found.depends.iter().cloned());
        modules.push(found);
    }
    Some(Closure {
        modules,
        fields: Mutex::new(HashMap::new()),
        methods: Mutex::new(HashMap::new()),
        implied: std::sync::OnceLock::new(),
    })
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
            });
        }
        visiting.remove(model);
        Some(fields)
    }
}

/// The modules in `dirs` that give `model` a field `field`: where to look
/// when a module uses a field its dependencies do not have.
pub fn modules_defining(dirs: &[PathBuf], model: &str, field: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !MANIFEST_FILE_NAMES.iter().any(|m| path.join(m).is_file()) {
                continue;
            }
            let Some(module) = module_index(&path) else { continue };
            let defines = module.classes.iter().any(|class| {
                let on_model = class.name.as_deref() == Some(model)
                    || (class.name.is_none() && class.inherit.iter().any(|i| i == model));
                on_model && class.fields.iter().any(|f| f.name == field)
            });
            if defines && !found.contains(&module.name) {
                found.push(module.name.clone());
            }
        }
    }
    found.sort();
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
}

/// Manifest keys of data files, in the order Odoo loads them: data, then demo.
const DATA_KEYS: &[&str] = &["init_xml", "update_xml", "data", "demo_xml", "demo"];

/// Elements that define an XML id.
const ID_ELEMENTS: &[&str] = &["record", "template", "menuitem", "report", "act_window", "url", "asset"];

fn data_files(path: &Path, manifest: &Manifest) -> Vec<DataFile> {
    let mut files: Vec<DataFile> = Vec::new();
    for key in DATA_KEYS {
        for name in manifest.get(key).map(strings).unwrap_or_default() {
            let file = path.join(&name);
            if files.iter().any(|f| f.path == file) {
                continue;
            }
            let rank = files.len() + 1;
            files.push(DataFile { path: file, rank });
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
                    let line = text[..node.range().start].matches('\n').count() + 1;
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
        let line_of = |offset: usize| text[..offset].matches('\n').count() + 1;
        for node in doc.descendants().filter(|n| n.is_element()) {
            let Some(id) = node.attribute("id") else { continue };
            let position = Position {
                rank: file.rank,
                offset: node.range().start,
            };
            if node.has_tag_name("template") {
                let inherit_id = node.attribute("inherit_id").map(|i| qualify(module, i));
                let primary = inherit_id.is_none() || node.attribute("primary").is_some_and(truthy);
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
        let mut found = fields(&module_index(&module).unwrap());
        found.sort();
        assert_eq!(found, vec!["a", "b"]);
    }
}
