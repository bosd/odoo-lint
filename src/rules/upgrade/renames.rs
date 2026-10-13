//! What each Odoo version renamed and removed (U1590, U1591, ... U1991):
//! models, from the migration scripts and analysis of OCA/OpenUpgrade, kept
//! in `data/v<major>.tsv` (see `scripts/gen_upgrade_data.py`).

use super::{text_range, version_guards};
use crate::checker::{PythonContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::rules::xml::{is, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use crate::visit::{walk, Node};
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

/// Replaces `old` inside the source of `range`, once.
fn rename_in(source: &str, range: TextRange, old: &str, new: &str) -> Option<Edit> {
    let start = range.start().to_usize();
    let text = source.get(start..range.end().to_usize())?;
    let offset = text.find(old)?;
    Some(Edit::replace(start + offset, start + offset + old.len(), new))
}

/// Fields of data records whose value is a model name.
const MODEL_FIELDS: &[&str] = &["model", "res_model", "src_model", "binding_model"];

fn check_xml(ctx: &XmlContext, reporter: &mut XmlReporter, rule: &Rule, major: u16, renamed: bool) {
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
            summary: concat!("A model that Odoo ", $major, ".0 renamed or merged into another."),
            doc: concat!(
                r#"
## What it does

Reports the names of models that Odoo "#,
                $major,
                r#".0 renamed, or merged into another model: in strings of the
Python code (`env["..."]`, `_inherit`, comodels) and in the `model`
attributes and model fields of data files.

## Why is this bad?

The old model no longer exists: `env["..."]` raises a `KeyError`, an
`_inherit` stops the registry from loading, and a data file fails to install.

## Fix safety

Safe for a rename: the new name replaces the old one. A merged model is
reported without a fix, as its fields and behaviour changed too.

## Limitations

The data comes from the migration scripts of
[OpenUpgrade](https://github.com/OCA/OpenUpgrade), so it covers Odoo's own
modules. Docstrings, migration scripts, version guards
(`if "x" in self.env:`) and models the module defines itself are skipped.
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
            summary: concat!("A model that Odoo ", $major, ".0 removed."),
            doc: concat!(
                r#"
## What it does

Reports the names of models that Odoo "#,
                $major,
                r#".0 removed: in strings of the Python code
and in the `model` attributes and model fields of data files.

## Why is this bad?

The model no longer exists: `env["..."]` raises a `KeyError`, an `_inherit`
stops the registry from loading, and a data file fails to install. The
feature moved elsewhere or went away; there is no automatic fix.

## Limitations

The data comes from the analysis of
[OpenUpgrade](https://github.com/OCA/OpenUpgrade), checked against Odoo's
sources, so it covers Odoo's own modules. Docstrings, migration scripts,
version guards and models the module defines itself are skipped.
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
