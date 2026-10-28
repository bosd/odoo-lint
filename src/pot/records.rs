//! The records a module's data files create (`<record>`, `<menuitem>`,
//! `<template>` and CSV rows) with the values of their fields, as Odoo's
//! loader (odoo/tools/convert.py) writes them, for the terms of their
//! translated fields.

use super::markup::{
    arch_of_field, arch_root, html_of_field, html_root, parse_element, serialize_children, terms, Method,
};
use super::models::{Models, Stored};
use super::Term;
use crate::index::{ModuleIndex, Translate};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

/// A field value a data file sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Value {
    /// Text, as the loader stores it (not stripped).
    Text(String),
    /// A `type="xml"` or `type="html"` field, or a `<template>`: the
    /// element as the source has it.
    Markup(Markup, String),
    /// The name of the action with this XML id, else this text: a
    /// `<menuitem>` without a name, whose action another module defines.
    ActionName(String, String),
    /// Anything else: a `ref`, a `search`, a file, an `eval` that is no
    /// string.
    Other,
}

/// What a markup value is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Markup {
    /// A `<field type="xml">`.
    Xml,
    /// A `<field type="html">`.
    Html,
    /// A `<template>`, whose element is the arch.
    Template,
}

/// A record of a data file, with its full XML id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Record {
    pub model: String,
    pub xml_id: String,
    pub fields: Vec<(String, Value)>,
}

/// The records of `module`'s data files, in load order, its demo data
/// with `demo`.
pub(super) fn module_records(module: &ModuleIndex, demo: bool) -> Vec<Record> {
    let mut records = Vec::new();
    for file in module.data_files.iter().filter(|f| demo || !f.demo) {
        let Ok(source) = std::fs::read_to_string(&file.path) else {
            continue;
        };
        match file.path.extension().and_then(|e| e.to_str()) {
            Some("xml") => xml_records(&module.name, &source, &mut records),
            Some("csv") => csv_records(&module.name, &file.path, &source, &mut records),
            _ => {}
        }
    }
    records
}

/// `module.name` for an id that has no module.
fn full_id(module: &str, id: &str) -> String {
    if id.contains('.') {
        id.to_string()
    } else {
        format!("{module}.{id}")
    }
}

fn xml_records(module: &str, source: &str, records: &mut Vec<Record>) {
    let Ok(doc) = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        },
    ) else {
        return;
    };
    walk(module, source, doc.root_element(), records);
}

/// The record elements of `<odoo>` and `<data>`, and of nested menu items.
fn walk(module: &str, source: &str, node: roxmltree::Node, records: &mut Vec<Record>) {
    for child in node.children().filter(roxmltree::Node::is_element) {
        match child.tag_name().name() {
            "data" | "odoo" | "openerp" => walk(module, source, child, records),
            "record" => records.extend(record(module, source, child)),
            "menuitem" => {
                let Some(id) = child.attribute("id") else { continue };
                let xml_id = full_id(module, id);
                records.push(menu(module, child, &xml_id, records));
                walk(module, source, child, records);
            }
            "template" => records.extend(template(module, source, child)),
            _ => {}
        }
    }
}

/// The value of a string `eval`, such as `eval="'Sales'"`.
fn eval_string(expr: &str) -> Option<String> {
    let parsed = ruff_python_parser::parse_expression(expr).ok()?;
    parsed
        .expr()
        .as_string_literal_expr()
        .map(|literal| literal.value.to_str().to_string())
}

fn record(module: &str, source: &str, node: roxmltree::Node) -> Option<Record> {
    let model = node.attribute("model")?.to_string();
    let xml_id = full_id(module, node.attribute("id")?);
    let mut fields = Vec::new();
    for field in node.children().filter(|n| n.has_tag_name("field")) {
        let Some(name) = field.attribute("name") else { continue };
        // A view's `arch` is computed: setting it writes `arch_db`.
        let name = match (model.as_str(), name) {
            ("ir.ui.view", "arch" | "arch_base") => "arch_db",
            _ => name,
        };
        let value = if field.has_attribute("ref") || field.has_attribute("search") || field.has_attribute("file") {
            Some(Value::Other)
        } else if let Some(expr) = field.attribute("eval") {
            Some(eval_string(expr).map_or(Value::Other, Value::Text))
        } else {
            match field.attribute("type") {
                Some("xml") => Some(Value::Markup(Markup::Xml, source[field.range()].to_string())),
                Some("html") => Some(Value::Markup(Markup::Html, source[field.range()].to_string())),
                Some("char") | None => Some(Value::Text(
                    field
                        .first_child()
                        .filter(roxmltree::Node::is_text)
                        .and_then(|t| t.text())
                        .unwrap_or_default()
                        .to_string(),
                )),
                _ => Some(Value::Other),
            }
        };
        if let Some(value) = value {
            fields.push((name.to_string(), value));
        }
    }
    Some(Record { model, xml_id, fields })
}

/// A `<menuitem>`: its `name`, else the name of its action (not of a
/// report), else its id (`_tag_menuitem`).
fn menu(module: &str, node: roxmltree::Node, xml_id: &str, records: &[Record]) -> Record {
    let id = node.attribute("id").unwrap_or("?").to_string();
    let name = match (node.attribute("name"), node.attribute("action")) {
        (Some(name), _) if !name.is_empty() => Value::Text(name.to_string()),
        (_, Some(action)) => {
            let action = full_id(module, action);
            // The module's own record of the action, unless it only
            // updates another module's action without naming it.
            let own = records
                .iter()
                .rev()
                .find(|r| r.xml_id == action)
                .filter(|r| action.starts_with(&format!("{module}.")) || r.fields.iter().any(|(f, _)| f == "name"));
            match own {
                Some(found) => action_name(found).map_or(Value::Text(id), Value::Text),
                None => Value::ActionName(action, id),
            }
        }
        _ => Value::Text(id),
    };
    Record {
        model: "ir.ui.menu".into(),
        xml_id: xml_id.to_string(),
        fields: vec![("name".into(), name)],
    }
}

/// The name a menu takes from its action: reports lend theirs to none.
fn action_name(action: &Record) -> Option<String> {
    if action.model == "ir.actions.report" {
        return None;
    }
    match &action.fields.iter().find(|(f, _)| f == "name")?.1 {
        Value::Text(name) if !name.is_empty() => Some(name.clone()),
        _ => None,
    }
}

/// A `<template>`: a QWeb view whose arch is the element itself (renamed
/// `t` or `data`, which changes none of its terms); in a theme module a
/// `theme.ir.ui.view`, whose arch is the `arch` field.
fn template(module: &str, source: &str, node: roxmltree::Node) -> Option<Record> {
    let xml_id = full_id(module, node.attribute("id")?);
    let (model, field) = if module.starts_with("theme_") {
        ("theme.ir.ui.view", "arch")
    } else {
        ("ir.ui.view", "arch_db")
    };
    Some(Record {
        model: model.into(),
        xml_id,
        fields: vec![(
            field.into(),
            Value::Markup(Markup::Template, source[node.range()].to_string()),
        )],
    })
}

/// The rows of a CSV data file: the model is the file name up to a `-`.
fn csv_records(module: &str, path: &Path, source: &str, records: &mut Vec<Record>) {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    let model = stem.split('-').next().unwrap_or(stem).to_string();
    let rows = parse_csv(source);
    let Some((header, rows)) = rows.split_first() else {
        return;
    };
    let Some(id_column) = header.iter().position(|h| h == "id") else {
        return;
    };
    // `name@fr` columns hold translations; `x:id` and `x/id` references.
    let columns: Vec<(usize, &str)> = header
        .iter()
        .enumerate()
        .filter(|(i, h)| *i != id_column && !h.contains(['@', ':', '/', '.']))
        .map(|(i, h)| (i, h.as_str()))
        .collect();
    for row in rows {
        let Some(id) = row.get(id_column).filter(|id| !id.is_empty()) else {
            continue;
        };
        let fields = columns
            .iter()
            .filter_map(|(i, name)| Some(((*name).to_string(), Value::Text(row.get(*i)?.clone()))))
            .collect();
        records.push(Record {
            model: model.clone(),
            xml_id: full_id(module, id),
            fields,
        });
    }
}

/// Rows of CSV text as Python's `csv.reader` reads them (`"` quotes, `,`).
fn parse_csv(source: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', true) => quoted = false,
            ('"', false) if field.is_empty() => quoted = true,
            (',', false) => row.push(std::mem::take(&mut field)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                }
                row.clear();
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// The value each field of each record has once the modules loaded, by
/// XML id: a later data file overrides an earlier one.
pub(super) fn final_values(records: impl IntoIterator<Item = Record>) -> BTreeMap<String, Record> {
    let mut found: BTreeMap<String, Record> = BTreeMap::new();
    for record in records {
        match found.get_mut(&record.xml_id) {
            Some(existing) => {
                for (name, value) in record.fields {
                    match existing.fields.iter_mut().find(|(f, _)| *f == name) {
                        Some(slot) => slot.1 = value,
                        None => existing.fields.push((name, value)),
                    }
                }
            }
            None => {
                found.insert(record.xml_id.clone(), record);
            }
        }
    }
    found
}

fn xml_terms(root: Option<super::markup::Element>) -> Vec<String> {
    root.map(|r| terms(&r, Method::Xml)).unwrap_or_default()
}

fn html_terms(root: Option<super::markup::Element>) -> Vec<String> {
    root.map(|r| terms(&r, Method::Html)).unwrap_or_default()
}

/// The names of actions of other modules, read once per module.
struct ActionNames<'m> {
    installed: &'m [Arc<ModuleIndex>],
    demo: bool,
    read: BTreeMap<String, BTreeMap<String, Record>>,
}

impl<'m> ActionNames<'m> {
    fn new(installed: &'m [Arc<ModuleIndex>], demo: bool) -> Self {
        Self {
            installed,
            demo,
            read: BTreeMap::new(),
        }
    }

    /// The name of the action `xml_id` once its module loaded.
    fn name(&mut self, xml_id: &str) -> Option<String> {
        let module = xml_id.split('.').next()?;
        if !self.read.contains_key(module) {
            let records = self
                .installed
                .iter()
                .find(|m| m.name == module)
                .map(|m| final_values(module_records(m, self.demo)))
                .unwrap_or_default();
            self.read.insert(module.to_string(), records);
        }
        action_name(self.read.get(module)?.get(xml_id)?)
    }
}

/// `module` and the installed modules that depend on it, directly or not:
/// only they can write its records.
fn writers<'m>(module: &str, installed: &'m [Arc<ModuleIndex>]) -> Vec<&'m ModuleIndex> {
    let mut found: BTreeSet<&str> = BTreeSet::from([module]);
    loop {
        let before = found.len();
        for candidate in installed {
            if candidate.depends.iter().any(|d| found.contains(d.as_str())) {
                found.insert(&candidate.name);
            }
        }
        if found.len() == before {
            break;
        }
    }
    installed
        .iter()
        .filter(|m| found.contains(m.name.as_str()))
        .map(Arc::as_ref)
        .collect()
}

/// The terms of the translated fields of `module`'s records, with the
/// values they have once every installed module (in load order) loaded.
pub(super) fn record_terms(module: &str, installed: &[Arc<ModuleIndex>], models: &Models, demo: bool) -> Vec<Term> {
    let prefix = format!("{module}.");
    let records = writers(module, installed)
        .into_iter()
        .flat_map(|m| module_records(m, demo))
        .filter(|record| record.xml_id.starts_with(&prefix));
    let mut found = Vec::new();
    let mut actions = ActionNames::new(installed, demo);
    for record in final_values(records).into_values() {
        for (field, value) in &record.fields {
            let (model, xml_id, translate) = match models.stored(&record.model, field) {
                Some(Stored::Here(translate)) => (record.model.clone(), record.xml_id.clone(), translate),
                Some(Stored::Parent(parent, translate)) => {
                    // A record that names its parent record writes there:
                    // Odoo gives the parent its own XML id only when it
                    // creates it.
                    let names_parent = record
                        .fields
                        .iter()
                        .any(|(f, _)| models.comodel(&record.model, f, 0).as_deref() == Some(parent.as_str()));
                    if names_parent {
                        continue;
                    }
                    let xml_id = format!("{}_{}", record.xml_id, parent.replace('.', "_"));
                    (parent, xml_id, translate)
                }
                None => continue,
            };
            let text = match value {
                Value::Text(text) => Some(text.clone()),
                Value::ActionName(action, id) => Some(actions.name(action).unwrap_or_else(|| id.clone())),
                Value::Markup(..) | Value::Other => None,
            };
            let reference = format!("{model},{field}");
            let (kind, sources) = match (translate, text, value) {
                (Translate::No, ..) => continue,
                // The sanitizer of outgoing emails drops the whitespace before
                // the content (`html_normalize` parses it); what follows is
                // the tail of the last element and stays.
                (Translate::Whole, Some(text), _) if models.sanitized_as_xml(&model, field) => {
                    ("model", vec![text.trim_start().to_string()])
                }
                (Translate::Whole, Some(text), _) => ("model", vec![text]),
                (Translate::Xml, Some(text), _) => ("model_terms", xml_terms(arch_root(&text))),
                (Translate::Html, Some(text), _) => ("model_terms", html_terms(html_root(&text))),
                (_, None, Value::Markup(markup, source)) => {
                    let Some(mut element) = parse_element(source) else {
                        continue;
                    };
                    // A template becomes an `xml` field too.
                    element.unescape_percents();
                    match (translate, markup) {
                        (Translate::Whole, Markup::Html) => {
                            // The sanitizer of outgoing emails stores XML.
                            let method = if models.sanitized_as_xml(&model, field) {
                                Method::Xml
                            } else {
                                Method::Html
                            };
                            let value = serialize_children(&element, method);
                            let value = if method == Method::Xml {
                                value.trim_start().to_string()
                            } else {
                                value
                            };
                            ("model", vec![value])
                        }
                        (Translate::Whole, Markup::Xml) => (
                            "model",
                            vec![format!(
                                "<?xml version=\"1.0\"?>\n{}",
                                serialize_children(&element, Method::Xml)
                            )],
                        ),
                        (Translate::Xml, Markup::Template) => ("model_terms", xml_terms(Some(element))),
                        (Translate::Xml, _) => ("model_terms", xml_terms(Some(arch_of_field(element)))),
                        (Translate::Html, _) => ("model_terms", html_terms(Some(html_of_field(element)))),
                        _ => continue,
                    }
                }
                _ => continue,
            };
            for source in sources {
                found.push(Term {
                    source,
                    reference: (kind.into(), reference.clone(), xml_id.clone()),
                    comment: None,
                });
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records(source: &str) -> Vec<Record> {
        let mut found = Vec::new();
        xml_records("acme", source, &mut found);
        found
    }

    #[test]
    fn a_menu_takes_the_name_of_another_module_action_it_only_updates() {
        let found = records(
            r#"<odoo><record id="base.ir_action_report" model="ir.actions.act_window"><field name="view_id" ref="view_x"/></record><menuitem id="menu_x" action="base.ir_action_report"/></odoo>"#,
        );
        assert_eq!(
            found[1].fields,
            [(
                "name".to_string(),
                Value::ActionName("base.ir_action_report".into(), "menu_x".into())
            )]
        );
    }

    #[test]
    fn reads_a_record_field_as_written() {
        let found = records(
            r#"<odoo><record id="action_x" model="ir.actions.act_window"><field name="name">My  Orders</field></record></odoo>"#,
        );
        assert_eq!(
            found[0].fields,
            [("name".to_string(), Value::Text("My  Orders".into()))]
        );
    }

    #[test]
    fn reads_a_string_eval() {
        let found =
            records(r#"<odoo><record id="x" model="res.groups"><field name="name" eval="'Manager'"/></record></odoo>"#);
        assert_eq!(found[0].fields, [("name".to_string(), Value::Text("Manager".into()))]);
    }

    #[test]
    fn keeps_references_as_no_text() {
        let found = records(
            r#"<odoo><record id="x" model="res.groups"><field name="category_id" ref="base.cat"/></record></odoo>"#,
        );
        assert_eq!(found[0].fields, [("category_id".to_string(), Value::Other)]);
    }

    #[test]
    fn names_a_menu_after_its_action() {
        let found = records(
            r#"<odoo><data>
            <record id="action_x" model="ir.actions.act_window"><field name="name">Orders</field></record>
            <menuitem id="menu_x" action="action_x"/>
            </data></odoo>"#,
        );
        assert_eq!(found[1].xml_id, "acme.menu_x");
        assert_eq!(found[1].fields, [("name".to_string(), Value::Text("Orders".into()))]);
    }

    #[test]
    fn keeps_a_markup_field_and_writes_a_view_arch_to_arch_db() {
        let field = r#"<field name="arch" type="xml"><form><field name="x"/></form></field>"#;
        let found = records(&format!(
            r#"<odoo><record id="v" model="ir.ui.view">{field}</record></odoo>"#
        ));
        assert_eq!(
            found[0].fields,
            [("arch_db".to_string(), Value::Markup(Markup::Xml, field.into()))]
        );
    }

    #[test]
    fn reads_csv_rows_without_translation_or_reference_columns() {
        let mut found = Vec::new();
        csv_records(
            "acme",
            Path::new("data/res.city-extra.csv"),
            "id,name,name@fr,country_id:id\ncity_a,\"Paris, Old\",Paris,base.fr\n",
            &mut found,
        );
        assert_eq!(found[0].model, "res.city");
        assert_eq!(
            found[0].fields,
            [("name".to_string(), Value::Text("Paris, Old".into()))]
        );
    }

    #[test]
    fn final_values_let_a_later_record_override_a_field() {
        let first = Record {
            model: "m".into(),
            xml_id: "acme.x".into(),
            fields: vec![("name".into(), Value::Text("Old".into()))],
        };
        let mut second = first.clone();
        second.fields = vec![("name".into(), Value::Text("New".into()))];
        let found = final_values([first, second]);
        assert_eq!(
            found["acme.x"].fields,
            [("name".to_string(), Value::Text("New".into()))]
        );
    }
}
