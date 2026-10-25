//! The terms Odoo creates for a module's models when it installs the module:
//! model descriptions, field labels and help, selection labels and
//! constraint messages, under the XML ids Odoo gives them in that module
//! (`ir.model`, `ir.model.fields`, `ir.model.fields.selection` and
//! `ir.model.constraint` records; odoo/addons/base/models/ir_model.py).
//!
//! Which records the module gets follows from the modules loaded up to it;
//! their values from every installed module, as the database has them.

use super::Term;
use crate::index::{Closure, Field, ModelClass, Selection};
use crate::odoo_version::OdooVersion;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

/// A class of the closure on a model, with the module that declares it.
struct Def<'c> {
    module: &'c str,
    class: &'c ModelClass,
}

/// A field of a model as Odoo sets it up from all its definitions.
#[derive(Debug, Clone, Default)]
struct Resolved {
    kind: String,
    comodel: Option<String>,
    string: Option<String>,
    help: Option<String>,
    related: Option<String>,
    export: Option<bool>,
    /// The modules that define the field (Odoo's `field._modules`).
    modules: BTreeSet<String>,
    /// The labels of a literal selection, by value.
    selection: BTreeMap<String, String>,
    /// The modules that define each selection value.
    selection_modules: BTreeMap<String, BTreeSet<String>>,
    /// Inherited through `_inherits`: from `field` of the parent model.
    inherited_from: Option<(String, String)>,
}

impl Resolved {
    /// Applies one more definition: its attributes override the earlier
    /// ones, unless the field type changes, which starts over.
    fn apply(&mut self, field: &Field, module: &str) {
        if !self.kind.is_empty() && !field.kind.is_empty() && self.kind != field.kind {
            *self = Self::default();
        }
        if !field.kind.is_empty() {
            self.kind.clone_from(&field.kind);
        }
        if field.comodel.is_some() {
            self.comodel.clone_from(&field.comodel);
        }
        let label = &field.label;
        if label.string.is_some() {
            self.string.clone_from(&label.string);
        }
        if label.help.is_some() {
            self.help.clone_from(&label.help);
        }
        if label.related.is_some() {
            self.related.clone_from(&label.related);
        }
        if label.export_string_translation.is_some() {
            self.export = label.export_string_translation;
        }
        self.modules.insert(module.to_string());
        // `_selection_modules`: a `selection` starts over, `selection_add`
        // adds the values that come with a label.
        match &label.selection {
            Some(Selection::List(pairs)) => {
                self.selection = pairs.iter().cloned().collect();
                self.selection_modules = pairs
                    .iter()
                    .map(|(value, _)| (value.clone(), BTreeSet::from([module.to_string()])))
                    .collect();
            }
            Some(Selection::Dynamic) => {
                self.selection.clear();
                self.selection_modules.clear();
            }
            None => {}
        }
        for (value, label) in &label.selection_add {
            if let Some(label) = label {
                self.selection.insert(value.clone(), label.clone());
                self.selection_modules
                    .entry(value.clone())
                    .or_default()
                    .insert(module.to_string());
            }
        }
    }
}

type Fields = BTreeMap<String, Resolved>;

/// The models of a closure, each with its classes in load order.
struct Models<'c> {
    defs: HashMap<String, Vec<Def<'c>>>,
    /// From 19.0 `id` and `display_name` are fields of `BaseModel`, so every
    /// model has them; before, the class defining a model adds them.
    id_fields_on_base: bool,
    resolved: std::cell::RefCell<HashMap<String, Option<Rc<Fields>>>>,
}

/// The model a class is on: its `_name`, else the first model it inherits.
fn model_of(class: &ModelClass) -> Option<&str> {
    class
        .name
        .as_deref()
        .or_else(|| class.inherit.first().map(String::as_str))
}

/// Whether `class` defines `model` rather than extending it.
fn is_definition(class: &ModelClass, model: &str) -> bool {
    class.name.as_deref() == Some(model) && !class.inherit.iter().any(|i| i == model)
}

/// The fields every model has, and their labels.
const ID_FIELDS: &[(&str, &str)] = &[("id", "ID"), ("display_name", "Display Name")];
/// The fields Odoo adds to the models that log access.
const LOG_FIELDS: &[(&str, &str)] = &[
    ("create_uid", "Created by"),
    ("create_date", "Created on"),
    ("write_uid", "Last Updated by"),
    ("write_date", "Last Updated on"),
];

impl<'c> Models<'c> {
    /// The models of `closure`, whose modules are in load order.
    fn new(closure: &'c Closure, version: OdooVersion) -> Self {
        let mut defs: HashMap<String, Vec<Def>> = HashMap::new();
        for module in closure.modules() {
            for class in module.classes.iter().filter(|c| c.odoo_model) {
                if let Some(model) = model_of(class) {
                    defs.entry(model.to_string()).or_default().push(Def {
                        module: &module.name,
                        class,
                    });
                }
            }
        }
        Self {
            defs,
            id_fields_on_base: version.major >= 19,
            resolved: std::cell::RefCell::default(),
        }
    }

    fn defs(&self, model: &str) -> &[Def<'c>] {
        self.defs.get(model).map_or(&[], Vec::as_slice)
    }

    /// The module that defines `model` first (`_original_module`).
    fn original_module(&self, model: &str) -> Option<&'c str> {
        self.defs(model)
            .iter()
            .find(|def| is_definition(def.class, model))
            .map(|def| def.module)
    }

    /// The parents `model` inherits through `_inherit`, other than itself,
    /// and `base`, which every model inherits.
    fn parents<'m>(&'m self, model: &'m str) -> impl Iterator<Item = &'m str> + 'm {
        let base = (model != "base").then_some("base");
        base.into_iter().chain(
            self.defs(model)
                .iter()
                .flat_map(move |def| def.class.inherit.iter().filter(move |p| *p != model))
                .map(String::as_str),
        )
    }

    /// The fields of `model` as Odoo sets them up: those of its parents,
    /// then its own definitions in load order, then the fields it inherits
    /// through `_inherits`. `None` when no class of the closure is on it.
    fn fields(&self, model: &str) -> Option<Rc<Fields>> {
        if let Some(found) = self.resolved.borrow().get(model) {
            return found.clone();
        }
        // A cycle resolves to nothing rather than looping.
        self.resolved.borrow_mut().insert(model.to_string(), None);
        let computed = self.compute(model).map(Rc::new);
        self.resolved.borrow_mut().insert(model.to_string(), computed.clone());
        computed
    }

    fn compute(&self, model: &str) -> Option<Fields> {
        let defs = self.defs(model);
        if defs.is_empty() {
            return None;
        }
        let mut fields = Fields::new();
        for parent in self.parents(model) {
            for (name, field) in self.fields(parent).iter().flat_map(|f| f.iter()) {
                fields.entry(name.clone()).or_insert_with(|| field.clone());
            }
        }
        if self.id_fields_on_base {
            for (name, label) in ID_FIELDS {
                fields.entry((*name).to_string()).or_insert_with(|| Resolved {
                    string: Some((*label).to_string()),
                    ..Resolved::default()
                });
            }
        }
        if let Some(definition) = defs.iter().find(|def| is_definition(def.class, model)) {
            let class = definition.class;
            let magic = |fields: &mut Fields, added: &[(&str, &str)]| {
                for (name, label) in added {
                    fields.entry((*name).to_string()).or_insert_with(|| Resolved {
                        string: Some((*label).to_string()),
                        modules: BTreeSet::from([definition.module.to_string()]),
                        ..Resolved::default()
                    });
                }
            };
            if !class.abstract_model && !self.id_fields_on_base {
                magic(&mut fields, ID_FIELDS);
            }
            let logs = !class.abstract_model && class.log_access.or(class.auto).unwrap_or(true);
            if logs {
                magic(&mut fields, LOG_FIELDS);
            }
        }
        for def in defs {
            for field in &def.class.fields {
                fields.entry(field.name.clone()).or_default().apply(field, def.module);
            }
        }
        for def in defs {
            for parent in &def.class.delegates {
                for (name, field) in self.fields(parent).iter().flat_map(|f| f.iter()) {
                    if fields.contains_key(name) {
                        continue;
                    }
                    let mut modules = field.modules.clone();
                    modules.insert(def.module.to_string());
                    fields.insert(
                        name.clone(),
                        Resolved {
                            kind: field.kind.clone(),
                            comodel: field.comodel.clone(),
                            export: field.export,
                            modules,
                            inherited_from: Some((parent.clone(), name.clone())),
                            ..Resolved::default()
                        },
                    );
                }
            }
        }
        Some(fields)
    }

    /// The label and help of a field as Odoo shows them: its own, those of
    /// the field it relates to, or a label made from its name.
    fn label(&self, model: &str, name: &str, depth: usize) -> Option<(String, Option<String>)> {
        let fields = self.fields(model)?;
        let field = fields.get(name)?;
        let target = match (&field.inherited_from, &field.related) {
            _ if depth > 8 => None,
            (Some((parent, parent_field)), _) => self.label(parent, parent_field, depth + 1),
            (None, Some(path)) if field.string.is_none() || field.help.is_none() => {
                self.related_label(model, path, depth)
            }
            _ => None,
        };
        let string = match (&field.string, &field.related, &field.inherited_from) {
            (Some(string), _, _) => string.clone(),
            (None, None, None) => default_label(name),
            _ => target.as_ref()?.0.clone(),
        };
        let help = field.help.clone().or_else(|| target.and_then(|(_, help)| help));
        Some((string, help))
    }

    /// The label of the field at the end of `path` (`partner_id.name`).
    fn related_label(&self, model: &str, path: &str, depth: usize) -> Option<(String, Option<String>)> {
        let mut current = model.to_string();
        let mut parts = path.split('.').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                return self.label(&current, part, depth + 1);
            }
            current = self.fields(&current)?.get(part)?.comodel.clone()?;
        }
        None
    }

    /// `models` and the models that inherit from them, directly or not
    /// (`_inherit` or `_inherits`): those Odoo sets up again when the
    /// module loads, sorted.
    fn descendants<'a>(&'a self, models: &BTreeSet<&'a str>) -> BTreeSet<&'a str> {
        if models.contains("base") {
            return self.defs.keys().map(String::as_str).collect();
        }
        let mut found: BTreeSet<&str> = models.clone();
        loop {
            let before = found.len();
            for (model, defs) in &self.defs {
                let inherits = defs.iter().any(|def| {
                    def.class
                        .inherit
                        .iter()
                        .chain(&def.class.delegates)
                        .any(|parent| parent != model && found.contains(parent.as_str()))
                });
                if inherits {
                    found.insert(model.as_str());
                }
            }
            if found.len() == before {
                return found;
            }
        }
    }

    /// The final value of `_description`, else the model's name.
    fn description(&self, model: &str) -> String {
        self.defs(model)
            .iter()
            .filter_map(|def| def.class.description.clone())
            .next_back()
            .unwrap_or_else(|| model.to_string())
    }

    /// The database table of `model`.
    fn table(&self, model: &str) -> String {
        self.defs(model)
            .iter()
            .filter_map(|def| def.class.table.clone())
            .next_back()
            .unwrap_or_else(|| model.replace('.', "_"))
    }

    /// The constraints of `model`, its parents' first: name to the module
    /// that defines it last and its message.
    fn constraints(
        &self,
        model: &str,
        table_objects: bool,
        seen: &mut HashSet<String>,
    ) -> BTreeMap<String, (String, Option<String>)> {
        let mut found = BTreeMap::new();
        if !seen.insert(model.to_string()) {
            return found;
        }
        let parents: Vec<&str> = self.parents(model).collect();
        for parent in parents {
            found.extend(self.constraints(parent, table_objects, seen));
        }
        for def in self.defs(model) {
            for constraint in def.class.constraints.iter().filter(|c| c.table_object == table_objects) {
                found.insert(
                    constraint.name.clone(),
                    (def.module.to_string(), constraint.message.clone()),
                );
            }
        }
        found
    }
}

/// The label Odoo makes from a field's name: `partner_id` is "Partner".
fn default_label(name: &str) -> String {
    let base = name
        .strip_suffix("_ids")
        .or_else(|| name.strip_suffix("_id"))
        .unwrap_or(name);
    // Python's `str.title()` on the name with spaces for underscores.
    let mut label = String::with_capacity(base.len());
    let mut previous_cased = false;
    for c in base.replace('_', " ").chars() {
        if previous_cased {
            label.extend(c.to_lowercase());
        } else {
            label.extend(c.to_uppercase());
        }
        previous_cased = c.is_alphabetic();
    }
    label
}

/// PostgreSQL's identifier limit as Odoo applies it (`make_identifier`).
fn make_identifier(identifier: &str) -> String {
    if identifier.chars().count() <= 63 {
        return identifier.to_string();
    }
    let prefix: String = identifier.chars().take(54).collect();
    format!("{prefix}_{:08x}", crc32(identifier.as_bytes()))
}

/// CRC-32 (IEEE), as Python's `zlib.crc32`.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn model_term(source: String, record: &str, xml_id: String) -> Term {
    Term {
        source,
        reference: ("model".into(), record.into(), xml_id),
        comment: None,
    }
}

/// The terms Odoo creates for `module`'s models: the records follow from
/// `at_load` (the modules loaded up to it, in load order), their values from
/// `installed` (every installed module, in load order).
pub fn model_terms(module: &str, at_load: &Closure, installed: &Closure, version: OdooVersion) -> Vec<Term> {
    let models = Models::new(at_load, version);
    let values = Models::new(installed, version);
    let mut terms = Vec::new();
    // The models the module defines or extends, in a stable order.
    let own: BTreeSet<&str> = models
        .defs
        .iter()
        .filter(|(_, defs)| defs.iter().any(|def| def.module == module))
        .map(|(model, _)| model.as_str())
        .collect();
    for model in &own {
        let xml_model = model.replace('.', "_");
        terms.push(model_term(
            values.description(model),
            "ir.model,name",
            format!("{module}.model_{xml_model}"),
        ));
    }
    for model in models.descendants(&own) {
        let Some(fields) = models.fields(model) else { continue };
        let xml_model = model.replace('.', "_");
        let original = models.original_module(model) == Some(module);
        let extends = own.contains(model);
        // Fields of the parents the module adds through `_inherit`.
        let introduced: Vec<Rc<Fields>> = models
            .defs(model)
            .iter()
            .filter(|def| def.module == module)
            .flat_map(|def| def.class.inherit.iter().filter(|p| *p != model))
            .filter_map(|parent| models.fields(parent))
            .collect();
        for (name, field) in fields.iter() {
            let has_id = original
                || field.modules.contains(module)
                || (extends && models.id_fields_on_base && ID_FIELDS.iter().any(|(id, _)| id == name))
                || introduced.iter().any(|parent| parent.contains_key(name));
            if !has_id || field.export == Some(false) {
                continue;
            }
            if let Some((string, help)) = values.label(model, name, 0).or_else(|| models.label(model, name, 0)) {
                let xml_id = format!("{module}.field_{xml_model}__{name}");
                terms.push(model_term(string, "ir.model.fields,field_description", xml_id.clone()));
                if let Some(help) = help.filter(|h| !h.is_empty()) {
                    terms.push(model_term(help, "ir.model.fields,help", xml_id));
                }
            }
        }
    }
    // Selections and constraints get the XML id of the module that defines
    // them whenever a model is set up, also by a module loaded later.
    let table_objects = version.major >= 19;
    for model in values.descendants(&own) {
        let Some(fields) = values.fields(model) else { continue };
        let xml_model = model.replace('.', "_");
        for (name, field) in fields.iter() {
            for (value, modules) in &field.selection_modules {
                let Some(label) = field.selection.get(value).filter(|_| modules.contains(module)) else {
                    continue;
                };
                let xml_value = value.replace(['.', ' '], "_").to_lowercase();
                terms.push(model_term(
                    label.clone(),
                    "ir.model.fields.selection,name",
                    format!("{module}.selection__{xml_model}__{name}__{xml_value}"),
                ));
            }
        }
        // `models.Constraint` from 19.0, `_sql_constraints` before.
        for (name, (owner, message)) in values.constraints(model, table_objects, &mut HashSet::new()) {
            let Some(message) = message.filter(|_| owner == module) else {
                continue;
            };
            let conname = format!("{}_{name}", values.table(model));
            let conname = if table_objects {
                make_identifier(&conname)
            } else {
                conname
            };
            terms.push(model_term(
                message,
                "ir.model.constraint,message",
                format!("{module}.constraint_{conname}"),
            ));
        }
    }
    terms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_label_drops_the_id_suffix() {
        assert_eq!(default_label("partner_id"), "Partner");
    }

    #[test]
    fn default_label_titles_each_word() {
        assert_eq!(default_label("minimum_so_amount"), "Minimum So Amount");
    }

    #[test]
    fn default_label_titles_after_digits_like_python() {
        assert_eq!(default_label("x_2nd_line_ids"), "X 2Nd Line");
    }

    #[test]
    fn make_identifier_keeps_short_names() {
        assert_eq!(make_identifier("res_partner_name_uniq"), "res_partner_name_uniq");
    }

    #[test]
    fn make_identifier_hashes_long_names_like_odoo() {
        let name = "account_analytic_distribution_model_recalculate_line_company_uniq";
        assert_eq!(
            make_identifier(name),
            "account_analytic_distribution_model_recalculate_line_c_96884b58"
        );
    }
}
