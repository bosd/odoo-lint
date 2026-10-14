//! What each Odoo version renamed and removed (U1590, U1591, ... U1991):
//! models and fields, from the migration scripts and analysis of OCA/OpenUpgrade, kept
//! in `data/v<major>.tsv` (see `scripts/gen_upgrade_data.py`).

use super::{data_elements, text_range, version_guards, view_archs};
use crate::checker::{PythonContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::receiver::Receivers;
use crate::rules::xml::{child_field, is, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use crate::visit::{enclosing_class, walk, Node, Scope};
use ruff_python_ast::{Expr, Stmt};
use ruff_text_size::{Ranged, TextRange};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// What one version renamed and removed.
#[derive(Default)]
pub struct Changes {
    pub models: HashMap<&'static str, &'static str>,
    pub merged_models: HashMap<&'static str, &'static str>,
    pub removed_models: HashSet<&'static str>,
    pub fields: HashMap<(&'static str, &'static str), &'static str>,
    pub removed_fields: HashSet<(&'static str, &'static str)>,
}

fn parse(text: &'static str) -> Changes {
    let mut changes = Changes::default();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let parts: Vec<&str> = line.split('\t').collect();
        match parts.as_slice() {
            ["model", old, new] => {
                changes.models.insert(old, new);
            }
            ["model-merged", old, new] => {
                changes.merged_models.insert(old, new);
            }
            ["model-removed", model] => {
                changes.removed_models.insert(model);
            }
            ["field", model, old, new] => {
                changes.fields.insert((model, old), new);
            }
            ["field-removed", model, field] => {
                changes.removed_fields.insert((model, field));
            }
            _ => {}
        }
    }
    changes
}

static V15: LazyLock<Changes> = LazyLock::new(|| parse(include_str!("data/v15.tsv")));
static V16: LazyLock<Changes> = LazyLock::new(|| parse(include_str!("data/v16.tsv")));
static V17: LazyLock<Changes> = LazyLock::new(|| parse(include_str!("data/v17.tsv")));
static V18: LazyLock<Changes> = LazyLock::new(|| parse(include_str!("data/v18.tsv")));
static V19: LazyLock<Changes> = LazyLock::new(|| parse(include_str!("data/v19.tsv")));

/// The changes of `major`, for the versions there is data for.
pub fn changes(major: u16) -> Option<&'static Changes> {
    Some(match major {
        15 => &V15,
        16 => &V16,
        17 => &V17,
        18 => &V18,
        19 => &V19,
        _ => return None,
    })
}

/// Models with a rule of their own (U1606).
const COVERED_MODELS: &[&str] = &["ir.translation"];

/// A name the version renamed or removed, as a finding: the message and,
/// for a rename, the new name.
fn model_change(changes: &Changes, major: u16, model: &str, renamed: bool) -> Option<(String, Option<&'static str>)> {
    if COVERED_MODELS.contains(&model) {
        return None;
    }
    if renamed {
        if let Some(new) = changes.models.get(model) {
            return Some((format!("Model `{model}` is `{new}` since Odoo {major}.0"), Some(new)));
        }
        if let Some(new) = changes.merged_models.get(model) {
            return Some((
                format!("Model `{model}` was merged into `{new}` in Odoo {major}.0"),
                None,
            ));
        }
    } else if changes.removed_models.contains(model) {
        return Some((format!("Model `{model}` was removed in Odoo {major}.0"), None));
    }
    None
}

/// Fields with rules of their own, left to them: `groups_id` (U1907,
/// U1916) everywhere; in data files also the view's `field_parent` (U1707),
/// `users` of groups (U1918), the cron's `numbercall` and `doall` (U1804), and the partner's `mobile` and
/// `title` (U1921).
fn covered_field(major: u16, model: &str, field: &str, xml: bool) -> bool {
    (major == 19 && field == "groups_id")
        || (xml
            && match major {
                17 => model == "ir.ui.view" && field == "field_parent",
                18 => model == "ir.cron" && matches!(field, "numbercall" | "doall"),
                19 => {
                    (model == "res.groups" && field == "users")
                        || (matches!(model, "res.partner" | "res.company") && matches!(field, "mobile" | "title"))
                }
                _ => false,
            })
}

/// A field of one of `models` the version renamed or removed: the message
/// and, for a rename, the new name.
fn field_change(
    changes: &Changes,
    major: u16,
    models: &[String],
    field: &str,
    renamed: bool,
    own: &HashSet<(String, String)>,
    xml: bool,
) -> Option<(String, Option<&'static str>)> {
    for model in models {
        if own.contains(&(model.clone(), field.to_string())) || covered_field(major, model, field, xml) {
            return None;
        }
        let key = (model.as_str(), field);
        if renamed {
            if let Some(new) = changes.fields.get(&key) {
                return Some((
                    format!("Field `{model}.{field}` is `{new}` since Odoo {major}.0"),
                    Some(new),
                ));
            }
        } else if changes.removed_fields.contains(&key) {
            return Some((format!("Field `{model}.{field}` was removed in Odoo {major}.0"), None));
        }
    }
    None
}

/// The fields the module defines itself, by model: a removed field it
/// defines again is its own.
fn own_fields(module: Option<&crate::checker::ModuleInfo>) -> HashSet<(String, String)> {
    let Some(index) = module.and_then(|m| crate::index::module_index(&m.path)) else {
        return HashSet::new();
    };
    let mut own = HashSet::new();
    for class in &index.classes {
        for model in class.name.iter().chain(&class.inherit) {
            for field in &class.fields {
                own.insert((model.clone(), field.name.clone()));
            }
        }
    }
    own
}

/// The models the module defines itself (`_name` without inheriting it):
/// an old name there is the module's own model.
fn own_models(module: Option<&crate::checker::ModuleInfo>) -> HashSet<String> {
    let Some(index) = module.and_then(|m| crate::index::module_index(&m.path)) else {
        return HashSet::new();
    };
    index
        .classes
        .iter()
        .filter_map(|c| c.name.as_ref().filter(|name| !c.inherit.contains(name)).cloned())
        .collect()
}

/// Files that mention old names on purpose: migration scripts.
fn is_migration(path: &str) -> bool {
    path.split(['/', '\\'])
        .any(|part| matches!(part, "migrations" | "upgrades"))
}

fn check_python(ctx: &PythonContext, reporter: &mut Reporter, rule: &Rule, major: u16, renamed: bool) {
    check_python_fields(ctx, reporter, rule, major, renamed);
    let Some(changes) = changes(major) else { return };
    if is_migration(ctx.file_path) {
        return;
    }
    let suite = ctx.parsed.suite();
    let mut docstrings = Vec::new();
    let mut found = Vec::new();
    walk(suite, |node, _| match node {
        Node::Stmt(Stmt::Expr(statement)) if statement.value.is_string_literal_expr() => {
            docstrings.push(statement.range());
        }
        Node::Expr(Expr::StringLiteral(literal)) => {
            let value = literal.value.to_str();
            if let Some((message, new)) = model_change(changes, major, value, renamed) {
                found.push((
                    literal.range(),
                    value.to_string(),
                    message,
                    new,
                    literal.value.is_implicit_concatenated(),
                ));
            }
        }
        _ => {}
    });
    if found.is_empty() {
        return;
    }
    let own = own_models(ctx.module);
    let names: Vec<&str> = found.iter().map(|(_, name, ..)| name.as_str()).collect();
    let guards = version_guards(suite, &names);
    for (range, name, message, new, concatenated) in &found {
        let skipped = own.contains(name)
            || docstrings.iter().any(|d| d.contains_range(*range))
            || guards.iter().any(|g| g.contains_range(*range));
        if skipped {
            continue;
        }
        let violation = reporter.report(rule, range.start(), message.clone());
        if let (Some(new), false) = (new, concatenated) {
            violation.fix =
                rename_in(ctx.source, *range, name, new).map(|edit| Fix::safe(format!("Use `{new}`"), vec![edit]));
        }
    }
}

/// A use of a field: the models it is looked up on, the range holding its
/// name, and the name.
struct FieldUse<'a> {
    models: Vec<String>,
    range: TextRange,
    name: &'a str,
}

/// The first field of a dotted path in a string literal (`"partner_id.name"`).
fn path_use<'a>(models: &[String], expr: &'a Expr) -> Option<FieldUse<'a>> {
    let literal = expr.as_string_literal_expr()?;
    if literal.value.is_implicit_concatenated() {
        return None;
    }
    let name = literal.value.to_str().split('.').next()?;
    (!name.is_empty()).then(|| FieldUse {
        models: models.to_vec(),
        range: literal.range(),
        name,
    })
}

/// The fields a domain names: the first element of its conditions.
fn domain_uses<'a>(models: &[String], expr: &'a Expr, out: &mut Vec<FieldUse<'a>>) {
    let Expr::List(list) = expr else { return };
    for condition in &list.elts {
        let first = match condition {
            Expr::Tuple(t) if t.elts.len() == 3 => &t.elts[0],
            Expr::List(l) if l.elts.len() == 3 => &l.elts[0],
            _ => continue,
        };
        out.extend(path_use(models, first));
    }
}

/// The keys of a dict literal of values (`write({...})`), or of a list of
/// them (`create([...])`).
fn value_uses<'a>(models: &[String], expr: &'a Expr, out: &mut Vec<FieldUse<'a>>) {
    match expr {
        Expr::Dict(dict) => {
            for item in &dict.items {
                if let Some(key) = item.key.as_ref().and_then(|k| k.as_string_literal_expr()) {
                    if !key.value.is_implicit_concatenated() {
                        out.push(FieldUse {
                            models: models.to_vec(),
                            range: key.range(),
                            name: key.value.to_str(),
                        });
                    }
                }
            }
        }
        Expr::List(list) => {
            for elt in &list.elts {
                value_uses(models, elt, out);
            }
        }
        _ => {}
    }
}

/// The models of a recordset expression, when the code shows them directly.
fn models_of(receivers: &Receivers, expr: &Expr, scopes: &[Scope]) -> Option<Vec<String>> {
    receivers
        .records(expr, scopes)
        .filter(|r| r.path.is_empty() && !r.models.is_empty())
        .map(|r| r.models)
}

fn check_python_fields(ctx: &PythonContext, reporter: &mut Reporter, rule: &Rule, major: u16, renamed: bool) {
    let Some(changes) = changes(major) else { return };
    let any = if renamed {
        !changes.fields.is_empty()
    } else {
        !changes.removed_fields.is_empty()
    };
    if !any || is_migration(ctx.file_path) {
        return;
    }
    let suite = ctx.parsed.suite();
    let receivers = Receivers::new(suite);
    let mut uses: Vec<FieldUse> = Vec::new();
    walk(suite, |node, scopes| match node {
        // `order.partner_id`
        Node::Expr(Expr::Attribute(attribute)) => {
            if let Some(models) = models_of(&receivers, &attribute.value, scopes) {
                uses.push(FieldUse {
                    models,
                    range: attribute.attr.range(),
                    name: attribute.attr.as_str(),
                });
            }
        }
        Node::Expr(Expr::Call(call)) => {
            let Expr::Attribute(func) = &*call.func else { return };
            let Some(models) = models_of(&receivers, &func.value, scopes) else {
                return;
            };
            let args = &call.arguments.args;
            let keyword = |name: &str| call.arguments.find_keyword(name).map(|k| &k.value);
            match func.attr.as_str() {
                "write" | "create" | "new" | "update" => {
                    if let Some(values) = args.first() {
                        value_uses(&models, values, &mut uses);
                    }
                }
                "search" | "search_count" | "search_read" | "search_fetch" | "filtered_domain" | "_search" => {
                    if let Some(domain) = args.first().or_else(|| keyword("domain")) {
                        domain_uses(&models, domain, &mut uses);
                    }
                }
                "mapped" | "filtered" | "sorted" | "grouped" => {
                    if let Some(path) = args.first() {
                        uses.extend(path_use(&models, path));
                    }
                }
                "read" => {
                    if let Some(Expr::List(fields)) = args.first().or_else(|| keyword("fields")) {
                        uses.extend(fields.elts.iter().filter_map(|f| path_use(&models, f)));
                    }
                }
                _ => {}
            }
        }
        // `@api.depends("partner_id.name")` in a model's class.
        Node::Stmt(Stmt::FunctionDef(function)) => {
            let Some(class) = enclosing_class(scopes) else { return };
            let models = crate::index::class_models(class);
            for decorator in &function.decorator_list {
                let Expr::Call(call) = &decorator.expression else {
                    continue;
                };
                if matches!(
                    crate::semantic::func_name(&call.func),
                    "depends" | "onchange" | "constrains"
                ) {
                    uses.extend(call.arguments.args.iter().filter_map(|a| path_use(&models, a)));
                }
            }
        }
        // `related="partner_id.name"` in a field of the class body.
        Node::Stmt(Stmt::Assign(assign)) => {
            let Some(Scope::Class(class)) = scopes.last() else {
                return;
            };
            let Expr::Call(call) = &*assign.value else { return };
            if let Some(related) = call.arguments.find_keyword("related") {
                uses.extend(path_use(&crate::index::class_models(class), &related.value));
            }
        }
        _ => {}
    });
    if uses.is_empty() {
        return;
    }
    let own = own_fields(ctx.module);
    let names: Vec<&str> = uses.iter().map(|u| u.name).collect();
    let guards = version_guards(suite, &names);
    let mut seen = HashSet::new();
    for field_use in uses {
        if guards.iter().any(|g| g.contains_range(field_use.range)) || !seen.insert(field_use.range) {
            continue;
        }
        let Some((message, new)) =
            field_change(changes, major, &field_use.models, field_use.name, renamed, &own, false)
        else {
            continue;
        };
        let violation = reporter.report(rule, field_use.range.start(), message);
        if let Some(new) = new {
            violation.fix = rename_in(ctx.source, field_use.range, field_use.name, new)
                .map(|edit| Fix::safe(format!("Use `{new}`"), vec![edit]));
        }
    }
}

/// Replaces `old` inside the source of `range`, once.
fn rename_in(source: &str, range: TextRange, old: &str, new: &str) -> Option<Edit> {
    let start = range.start().to_usize();
    let text = source.get(start..range.end().to_usize())?;
    let offset = text.find(old)?;
    Some(Edit::replace(start + offset, start + offset + old.len(), new))
}

/// Fields of data records whose value is a model name.
const MODEL_FIELDS: &[&str] = &["model", "res_model", "src_model", "binding_model"];

/// `//field[@name='partner_id']` in an xpath.
static XPATH_FIELD: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"field\[@name\s*=\s*['"](\w+)['"]"#).unwrap());

fn check_xml_fields(ctx: &XmlContext, reporter: &mut XmlReporter, rule: &Rule, major: u16, renamed: bool) {
    let Some(changes) = changes(major) else { return };
    let own = own_fields(Some(ctx.module));
    for file in ctx.files {
        // The element to report, the models, and the range of the name.
        let mut uses: Vec<(roxmltree::Node, String, std::ops::Range<usize>)> = Vec::new();
        // The fields of a view's model: not inside a sub-view of a field.
        for (record, arch) in view_archs(file) {
            let Some(model) = child_field(record, "model").and_then(|m| m.text()).map(str::trim) else {
                continue;
            };
            for node in arch.descendants().filter(|n| n.is_element()) {
                let in_subview = node
                    .ancestors()
                    .skip(1)
                    .take_while(|a| *a != arch)
                    .any(|a| is(a, "field") && a.attribute("position").is_none());
                if in_subview {
                    continue;
                }
                if is(node, "field") {
                    if let Some(name) = node.attribute_node("name") {
                        uses.push((node, model.to_string(), name.range_value()));
                    }
                } else if is(node, "xpath") {
                    if let Some(expr) = node.attribute_node("expr") {
                        let start = expr.range_value().start;
                        // The raw value: entities would shift the offsets.
                        let raw = file.source.get(expr.range_value()).unwrap_or_default();
                        for captures in XPATH_FIELD.captures_iter(raw) {
                            let name = captures.get(1).expect("group");
                            uses.push((node, model.to_string(), start + name.start()..start + name.end()));
                        }
                    }
                }
            }
        }
        // The fields of data records.
        for record in data_elements(file, &["record"]) {
            let Some(model) = record.attribute("model") else {
                continue;
            };
            for field in record.children().filter(|c| is(*c, "field")) {
                if let Some(name) = field.attribute_node("name") {
                    uses.push((field, model.to_string(), name.range_value()));
                }
            }
        }
        for (node, model, range) in uses {
            let Some(name) = file.source.get(range.clone()) else {
                continue;
            };
            let Some((message, new)) = field_change(changes, major, &[model], name, renamed, &own, true) else {
                continue;
            };
            let violation = reporter.report(rule, file, node.range().start, message);
            if let Some(new) = new {
                violation.fix = Some(Fix::safe(
                    format!("Use `{new}`"),
                    vec![Edit::replace(range.start, range.end, new)],
                ));
            }
        }
    }
}

fn check_xml(ctx: &XmlContext, reporter: &mut XmlReporter, rule: &Rule, major: u16, renamed: bool) {
    check_xml_fields(ctx, reporter, rule, major, renamed);
    let Some(changes) = changes(major) else { return };
    let own = own_models(Some(ctx.module));
    for file in ctx.files {
        let Some(doc) = &file.doc else { continue };
        for node in doc.descendants().filter(|n| n.is_element()) {
            // `model="..."` of records and the like, or the text of a
            // `<field name="model">` of a record.
            let range = if ["record", "function", "delete", "report"].iter().any(|t| is(node, t)) {
                node.attribute_node("model").map(|a| a.range_value())
            } else if is(node, "field")
                && node.attribute("name").is_some_and(|n| MODEL_FIELDS.contains(&n))
                && ["type", "ref", "eval"].iter().all(|a| node.attribute(*a).is_none())
                && node.parent_element().is_some_and(|p| is(p, "record"))
            {
                text_range(node)
            } else {
                None
            };
            let Some(range) = range else { continue };
            let Some(text) = file.source.get(range.clone()) else {
                continue;
            };
            let model = text.trim();
            if own.contains(model) {
                continue;
            }
            let Some((message, new)) = model_change(changes, major, model, renamed) else {
                continue;
            };
            let violation = reporter.report(rule, file, node.range().start, message);
            if let Some(new) = new {
                let start = range.start + text.find(model).unwrap_or(0);
                violation.fix = Some(Fix::safe(
                    format!("Use `{new}`"),
                    vec![Edit::replace(start, start + model.len(), new)],
                ));
            }
        }
    }
}

macro_rules! version_rules {
    ($renamed:ident, $removed:ident, $major:literal) => {
        pub const $renamed: Rule = Rule {
            code: concat!("U", $major, "90"),
            name: concat!("upgrade-renamed-", $major),
            summary: concat!(
                "A model or field that Odoo ",
                $major,
                ".0 renamed, or a model it merged into another."
            ),
            doc: concat!(
                r#"
## What it does

Reports what Odoo "#,
                $major,
                r#".0 renamed:

- models, also those merged into another: in strings of the Python code
  (`env["..."]`, `_inherit`, comodels) and in the `model` attributes and
  model fields of data files;
- fields: on records whose model the code shows (`self` in a model,
  `env["..."]`, variables and loops over them): attribute access, the
  values of `write()` and `create()`, domains of `search()`, paths of
  `mapped()` and `filtered()`, `@api.depends()` and `related=`; in data
  files the fields of views (not of sub-views), of `xpath` expressions and
  of records.

## Why is this bad?

The old name no longer exists: `env["..."]` raises a `KeyError`, an
`_inherit` stops the registry from loading, a view or data file fails to
install, and Python code fails where it reads the field.

## Fix safety

Safe for a rename: the new name replaces the old one. A merged model is
reported without a fix, as its fields and behaviour changed too.

## Limitations

The data comes from the migration scripts of
[OpenUpgrade](https://github.com/OCA/OpenUpgrade), so it covers Odoo's own
modules. Docstrings, migration scripts, version guards
(`if "x" in self.env:`), and models and fields the module defines itself
are skipped, as are the names other rules report (`groups_id`, ...).
"#
            ),
            check: Check::PythonXml(
                |ctx, reporter| check_python(ctx, reporter, &$renamed, $major, true),
                |ctx, reporter| check_xml(ctx, reporter, &$renamed, $major, true),
            ),
            min_odoo: Some(OdooVersion::new($major, 0)),
            max_odoo: None,
        };

        pub const $removed: Rule = Rule {
            code: concat!("U", $major, "91"),
            name: concat!("upgrade-removed-", $major),
            summary: concat!("A model or field that Odoo ", $major, ".0 removed."),
            doc: concat!(
                r#"
## What it does

Reports the models and fields that Odoo "#,
                $major,
                r#".0 removed, in the places
listed for the renames (U"#,
                $major,
                r#"90).

## Why is this bad?

The name no longer exists: `env["..."]` raises a `KeyError`, an `_inherit`
stops the registry from loading, a view or data file fails to install, and
Python code fails where it reads the field. The feature moved elsewhere or
went away; there is no automatic fix.

## Limitations

The data comes from the analysis of
[OpenUpgrade](https://github.com/OCA/OpenUpgrade), checked against Odoo's
sources, so it covers Odoo's own modules. Docstrings, migration scripts,
version guards, models and fields the module defines itself, and names
other rules report are skipped.
"#
            ),
            check: Check::PythonXml(
                |ctx, reporter| check_python(ctx, reporter, &$removed, $major, false),
                |ctx, reporter| check_xml(ctx, reporter, &$removed, $major, false),
            ),
            min_odoo: Some(OdooVersion::new($major, 0)),
            max_odoo: None,
        };
    };
}

version_rules!(RENAMED_15, REMOVED_15, 15);
version_rules!(RENAMED_16, REMOVED_16, 16);
version_rules!(RENAMED_17, REMOVED_17, 17);
version_rules!(RENAMED_18, REMOVED_18, 18);
version_rules!(RENAMED_19, REMOVED_19, 19);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_parses() {
        for major in 15..=19 {
            let changes = changes(major).unwrap();
            assert!(
                !changes.models.is_empty() || !changes.removed_models.is_empty(),
                "{major}"
            );
        }
        assert_eq!(changes(18).unwrap().models.get("pos.combo"), Some(&"product.combo"));
    }
}
