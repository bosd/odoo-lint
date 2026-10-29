//! What a module can rely on: the models, fields and methods of everything
//! its `depends` reach.

use super::{
    find_module, module_index, Field, FieldLabel, FieldMap, ModelClass, ModuleIndex, Position, View, MAGIC_FIELDS,
};
use crate::checker::ModuleInfo;
use rayon::prelude::*;
use ruff_python_ast::Stmt;
use ruff_python_parser::parse_module;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

/// The method names of a model.
pub type MethodSet = HashSet<String>;

/// The modules a module can rely on: itself and everything its `depends`
/// reach, with the fields of their models.
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
