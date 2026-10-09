//! References across modules, checked against the addons path: XML ids
//! (ODOO005) and models (ODOO006) that do not exist where a module can see
//! them. Silent without the dependencies of a module (see `addons-path`).

use crate::index::{closure, module_index, Closure, ModuleIndex, Position};
use crate::manifest::MANIFEST_FILE_NAMES;
use crate::rules::module::{ModuleContext, ModuleReporter};
use crate::rules::{Check, Rule};
use regex::Regex;
use ruff_python_ast::Expr;
use ruff_text_size::Ranged;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

pub const XML_ID_NOT_FOUND: Rule = Rule {
    code: "ODOO005",
    name: "xml-id-not-found",
    summary: "A reference to an XML id that does not exist, comes from outside `depends`, or is loaded later.",
    doc: r#"
## What it does

Checks the XML ids a module refers to: `ref="..."`, `parent` and `action`
of menus, `inherit_id`, `groups`, `t-call`, `%(...)d`, `ref('...')` in
`eval`, the `:id` columns of CSV files, records that override another
module's (`id="base.main_company"`), and `env.ref()` and `has_group()` in
Python.

It reports an id that does not exist, one from a module that `depends` does
not reach, and one of the module itself that is defined in a later data
file (or further down, or only in demo data).

## Why is this bad?

Odoo resolves the reference while it loads the record: the module fails to
install with "External ID not found". Outside `depends`, it works only on a
database where the other module happens to be installed. In Python, the
error comes when the code runs.

## Configuration

Like [ODOO004](ODOO004.md), the check needs the dependencies, Odoo's
included, in `addons-path`; a module whose dependencies cannot all be found
is not checked. `env.ref(..., raise_if_not_found=False)` is not reported.
"#,
    check: Check::Module(check_xml_ids),
    min_odoo: None,
    max_odoo: None,
};

pub const MODEL_NOT_FOUND: Rule = Rule {
    code: "ODOO006",
    name: "model-not-found",
    summary: "A model that no module of `depends` defines.",
    doc: r#"
## What it does

Reports models that no module the module's `depends` reach defines (with
`_name`): `_inherit` and `_inherits` parents in Python, and the models of
records, views, actions and `<function>`s in data files.

## Why is this bad?

Odoo refuses to load the module ("Model ... does not exist"). When another
module of the addons path defines the model, the message names it: a
missing dependency.

## Configuration

Like [ODOO004](ODOO004.md), the check needs the dependencies in
`addons-path`.
"#,
    check: Check::Module(check_models),
    min_odoo: None,
    max_odoo: None,
};

/// The closure of the module, when its dependencies are all known.
fn context(ctx: &ModuleContext) -> Option<(Closure, Vec<PathBuf>, Arc<ModuleIndex>)> {
    let local_base = ctx
        .module
        .path
        .parent()
        .is_some_and(|dir| dir.join("base").join("__manifest__.py").is_file());
    if ctx.settings.addons_path.is_empty() && !local_base {
        return None;
    }
    let closure = closure(ctx.module, &ctx.settings.addons_path)?;
    let mut dirs: Vec<PathBuf> = ctx.module.path.parent().map(PathBuf::from).into_iter().collect();
    dirs.extend(ctx.settings.addons_path.iter().cloned());
    let own = module_index(&ctx.module.path)?;
    Some((closure, dirs, own))
}

/// The module `name` anywhere in `dirs`.
fn find_module(dirs: &[PathBuf], name: &str) -> Option<Arc<ModuleIndex>> {
    dirs.iter()
        .map(|dir| dir.join(name))
        .find(|path| MANIFEST_FILE_NAMES.iter().any(|m| path.join(m).is_file()))
        .and_then(|path| module_index(&path))
}

// --- References ---------------------------------------------------------------

/// A reference to an XML id, and where it is.
struct Reference {
    id: String,
    path: PathBuf,
    line: usize,
    /// Load position, for references resolved while the data loads; `None`
    /// for those resolved later (`t-call`, Python).
    position: Option<Position>,
    /// Looked up only when used (`t-call`, `groups` in QWeb templates,
    /// Python): code may guard it, so only an id that does not exist in a
    /// module it can see is reported, not one from outside `depends`.
    soft: bool,
}

static EVAL_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bref\(\s*['"]([\w.]+)['"](\s*,\s*(?:raise_if_not_found\s*=\s*)?False)?"#).unwrap());
/// `%(module.action)d` in button names and the like; `%(city)s` is a
/// format string (address formats), not a reference.
static ACTION_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"%\(([\w.]+)\)d").unwrap());

fn line_of(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].matches('\n').count() + 1
}

/// The references of an XML data file.
fn xml_references(path: &Path, rank: usize, text: &str, out: &mut Vec<Reference>) {
    let Ok(doc) = roxmltree::Document::parse(text) else {
        return;
    };
    for node in doc.descendants().filter(|n| n.is_element()) {
        let offset = node.range().start;
        let at = |id: &str, soft: bool| Reference {
            id: id.trim().to_string(),
            path: path.to_path_buf(),
            line: line_of(text, offset),
            position: (!soft).then_some(Position { rank, offset }),
            soft,
        };
        let tag = node.tag_name().name();
        // QWeb: `<template>` content and qweb views are rendered on demand.
        let in_qweb = node
            .ancestors()
            .skip(1)
            .any(|a| a.tag_name().name() == "template" || (a.tag_name().name() == "templates"));
        for attribute in node.attributes() {
            let value = attribute.value();
            match attribute.name() {
                "ref" => out.push(at(value, false)),
                "inherit_id" if tag == "template" => out.push(at(value, false)),
                "parent" | "action" if tag == "menuitem" => out.push(at(value, false)),
                "groups" => {
                    for group in value.split(',') {
                        let group = group.trim().trim_start_matches('!');
                        if !group.is_empty() {
                            out.push(at(group, in_qweb || tag == "template"));
                        }
                    }
                }
                // `t-call="JournalTop"` calls a `t-name` of the same view.
                "t-call" if value.contains('.') && !value.contains(['{', '%', '(']) => out.push(at(value, true)),
                "eval" => {
                    // `ref(..., False)` is optional: `None` when missing.
                    for captures in EVAL_REF.captures_iter(value).filter(|c| c.get(2).is_none()) {
                        out.push(at(&captures[1], false));
                    }
                }
                _ => {}
            }
            for captures in ACTION_REF.captures_iter(value) {
                out.push(at(&captures[1], false));
            }
        }
    }
}

/// The references of a CSV data file: its `:id` and `/id` columns.
fn csv_references(path: &Path, rank: usize, text: &str, out: &mut Vec<Reference>) {
    let Ok(records) = crate::rules::module::read_csv(text) else {
        return;
    };
    let mut rows = records.into_iter();
    let Some(header) = rows.next() else { return };
    let columns: Vec<usize> = header
        .fields
        .iter()
        .enumerate()
        .filter(|(_, name)| name.ends_with(":id") || name.ends_with("/id"))
        .map(|(i, _)| i)
        .collect();
    for row in rows {
        for &column in &columns {
            let Some(value) = row.fields.get(column) else { continue };
            for id in value.split(',').map(str::trim).filter(|v| !v.is_empty()) {
                out.push(Reference {
                    id: id.to_string(),
                    path: path.to_path_buf(),
                    line: row.line,
                    position: Some(Position { rank, offset: row.line }),
                    soft: false,
                });
            }
        }
    }
}

/// `env.ref("module.name")` and `has_group("module.name")` in the module's
/// Python files, outside tests.
fn python_references(module: &ModuleIndex, out: &mut Vec<Reference>) {
    let files = walkdir::WalkDir::new(&module.path)
        .into_iter()
        .filter_entry(|e| {
            !(e.file_type().is_dir()
                && matches!(
                    e.file_name().to_str(),
                    Some("tests" | "migrations" | "upgrades" | "static" | "__pycache__")
                ))
        })
        .flatten()
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "py"));
    for file in files {
        let Ok(source) = std::fs::read_to_string(file.path()) else {
            continue;
        };
        let Ok(parsed) = ruff_python_parser::parse_module(&source) else {
            continue;
        };
        crate::visit::walk(parsed.suite(), |node, _| {
            let crate::visit::Node::Expr(Expr::Call(call)) = node else {
                return;
            };
            let Expr::Attribute(func) = &*call.func else { return };
            let method = func.attr.as_str();
            let receiver_is_env = matches!(&*func.value, Expr::Attribute(a) if a.attr.as_str() == "env")
                || matches!(&*func.value, Expr::Name(n) if n.id.as_str() == "env");
            let ids: Vec<String> = match method {
                "ref" if receiver_is_env => {
                    let optional = call.arguments.keywords.iter().any(|k| {
                        k.arg.as_ref().is_some_and(|a| a.as_str() == "raise_if_not_found")
                            && !matches!(&k.value, Expr::BooleanLiteral(b) if b.value)
                    }) || call
                        .arguments
                        .args
                        .get(1)
                        .is_some_and(|a| !matches!(a, Expr::BooleanLiteral(b) if b.value));
                    if optional {
                        return;
                    }
                    call.arguments
                        .args
                        .first()
                        .and_then(|a| a.as_string_literal_expr())
                        .map(|s| s.value.to_str().to_string())
                        .into_iter()
                        .collect()
                }
                "has_group" | "has_groups" | "user_has_groups" => call
                    .arguments
                    .args
                    .first()
                    .and_then(|a| a.as_string_literal_expr())
                    .map(|s| {
                        s.value
                            .to_str()
                            .split(',')
                            .map(|g| g.trim().trim_start_matches('!').to_string())
                            .collect()
                    })
                    .unwrap_or_default(),
                _ => return,
            };
            for id in ids.into_iter().filter(|id| id.contains('.')) {
                out.push(Reference {
                    id,
                    path: file.path().to_path_buf(),
                    line: line_of(&source, call.start().to_usize()),
                    position: None,
                    soft: true,
                });
            }
        });
    }
}

/// Ids Odoo creates for every module or category, and exported ids.
fn always_exists(module: &str, name: &str) -> bool {
    module == "__export__"
        || (module == "base" && (name.starts_with("module_") || name.starts_with("lang_")))
        || name.starts_with("selection__")
        || name.starts_with("constraint_")
}

fn check_xml_ids(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let Some((closure, dirs, own)) = context(ctx) else {
        return;
    };
    let mut references = Vec::new();
    for file in &own.data_files {
        let Ok(text) = std::fs::read_to_string(&file.path) else {
            continue;
        };
        match file
            .path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .as_deref()
        {
            Some("xml") => xml_references(&file.path, file.rank, &text, &mut references),
            Some("csv") => csv_references(&file.path, file.rank, &text, &mut references),
            _ => {}
        }
    }
    python_references(&own, &mut references);
    for reference in references {
        let id = if reference.id.contains('.') {
            reference.id.clone()
        } else {
            format!("{}.{}", own.name, reference.id)
        };
        let Some((module, name)) = id.split_once('.') else {
            continue;
        };
        if name.is_empty() || always_exists(module, name) || name.contains(['{', '%', ' ']) {
            continue;
        }
        // Odoo also names the parent records of delegated models:
        // `product_4` creates `product_4_product_template`.
        let delegated = || {
            name.match_indices('_').any(|(i, _)| {
                name[i + 1..].contains('_') && closure.xml_id(&format!("{module}.{}", &name[..i])).is_some()
            })
        };
        let message = match closure.xml_id(&id) {
            None if delegated() => None,
            // Created by this module: it must come before the reference.
            Some(_) if closure.xml_id_elsewhere(&id, &own.name) => None,
            Some(defined) => match (own.xml_ids.get(&id), reference.position) {
                (Some(own_defined), Some(used)) if *own_defined == defined && defined > used => {
                    Some(match own.data_files.iter().find(|f| f.rank == defined.rank) {
                        Some(file) if defined.rank != used.rank => format!(
                            "XML id `{id}` is defined in `{}`, which Odoo loads later",
                            file.path.strip_prefix(&own.path).unwrap_or(&file.path).display()
                        ),
                        _ => format!("XML id `{id}` is defined further down in this file; Odoo loads it later"),
                    })
                }
                _ => None,
            },
            None if reference.soft => {
                // Only when the module is reachable and lacks the id.
                closure.module(module).map(|_| format!("XML id `{id}` does not exist"))
            }
            None => {
                let elsewhere = find_module(&dirs, module);
                match elsewhere {
                    _ if closure.module(module).is_some() => Some(format!("XML id `{id}` does not exist")),
                    Some(target) if target.xml_ids.contains_key(&id) => Some(format!(
                        "XML id `{id}` comes from `{module}`, which `depends` does not reach"
                    )),
                    Some(_) => Some(format!(
                        "XML id `{id}` does not exist, and `{module}` is not reached by `depends` either"
                    )),
                    // A module outside the addons path: nothing to tell.
                    None => None,
                }
            }
        };
        if let Some(message) = message {
            reporter.report(&XML_ID_NOT_FOUND, &reference.path, reference.line, message);
        }
    }
}

// --- Models -------------------------------------------------------------------

/// The modules in `dirs` that define `model` with `_name`.
fn modules_defining_model(dirs: &[PathBuf], model: &str) -> Vec<String> {
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !MANIFEST_FILE_NAMES.iter().any(|m| path.join(m).is_file()) {
                continue;
            }
            let Some(module) = module_index(&path) else { continue };
            if module.classes.iter().any(|c| c.name.as_deref() == Some(model)) && !found.contains(&module.name) {
                found.push(module.name.clone());
            }
        }
    }
    found.sort();
    found
}

fn model_message(dirs: &[PathBuf], model: &str) -> String {
    let modules = modules_defining_model(dirs, model);
    if modules.is_empty() {
        format!("Model `{model}` does not exist in the modules `depends` reaches")
    } else {
        format!(
            "Model `{model}` comes from `{}`, which `depends` does not reach",
            modules.join("`, `")
        )
    }
}

/// Fields of data records whose value is a model name.
const MODEL_FIELDS: &[&str] = &["model", "res_model", "src_model", "binding_model"];

fn check_models(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let Some((closure, dirs, own)) = context(ctx) else {
        return;
    };
    for class in &own.classes {
        let parents = class.inherit.iter().chain(class.delegates.iter());
        for parent in parents {
            if class.name.as_deref() == Some(parent.as_str()) || closure.defines(parent) {
                continue;
            }
            reporter.report(&MODEL_NOT_FOUND, &class.file, class.line, model_message(&dirs, parent));
        }
    }
    for file in &own.data_files {
        if file.path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("xml")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file.path) else {
            continue;
        };
        let Ok(doc) = roxmltree::Document::parse(&text) else {
            continue;
        };
        for node in doc.descendants().filter(|n| n.is_element()) {
            let tag = node.tag_name().name();
            let model = match tag {
                "record" | "function" | "delete" | "report" => node.attribute("model"),
                // `<field name="model">sale.order</field>` of a view or an
                // action, outside archs.
                "field"
                    if node.attribute("name").is_some_and(|n| MODEL_FIELDS.contains(&n))
                        && node.attribute("type").is_none()
                        && node.attribute("ref").is_none()
                        && node.attribute("eval").is_none()
                        && node.parent_element().is_some_and(|p| p.tag_name().name() == "record") =>
                {
                    node.text().map(str::trim)
                }
                _ => None,
            };
            let Some(model) = model.filter(|m| !m.is_empty() && m.contains('.') && !m.contains(' ')) else {
                continue;
            };
            if !closure.defines(model) {
                reporter.report(
                    &MODEL_NOT_FOUND,
                    &file.path,
                    line_of(&text, node.range().start),
                    model_message(&dirs, model),
                );
            }
        }
    }
}
