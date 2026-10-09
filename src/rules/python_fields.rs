//! ODOO007: field names in Python that the model does not have, checked
//! against the models of the addons path. Silent without the dependencies
//! of a module (see `addons-path`).

use crate::checker::{PythonContext, Reporter};
use crate::index::{closure, modules_defining, Closure};
use crate::rules::python::{classes, field_definitions, methods};
use crate::rules::{Check, Rule};
use crate::semantic::func_name;
use crate::visit::{walk, Node};
use ruff_python_ast::{Expr, Stmt, StmtClassDef};
use ruff_text_size::{Ranged, TextSize};
use std::path::PathBuf;

pub const PYTHON_FIELD_NOT_FOUND: Rule = Rule {
    code: "ODOO007",
    name: "python-field-not-found",
    summary: "Python code names a field the model does not have.",
    doc: r#"
## What it does

Checks the field names and paths (`partner_id.country_id.code`) in model
classes against the models of the addons path, following relational fields
to their comodel:

- `@api.depends`, `@api.onchange` and `@api.constrains`;
- `related=`, `currency_field=` and the inverse field of a `One2many`;
- the field names of a field's `domain` (on its comodel);
- `_rec_name` and `_order`;
- `mapped()`, `filtered()` and `sorted()` with a string on `self`.

## Why is this bad?

Odoo refuses most of these when it loads the model ("Field x does not
exist", or an invalid `related` or `_order`), and the rest fail when the
code runs. A field defined in a module outside `depends` works only on a
database where that module happens to be installed: the message names it.

## Configuration

Like [ODOO004](ODOO004.md), the check needs the dependencies, Odoo's
included, in `addons-path`; a module whose dependencies cannot all be found
is not checked, and a path through a model the index does not know stops
there without a report.
"#,
    check: Check::Python(check_python_fields),
    min_odoo: None,
    max_odoo: None,
};

fn literal(expr: &Expr) -> Option<&str> {
    expr.as_string_literal_expr().map(|s| s.value.to_str())
}

fn class_value<'a>(class: &'a StmtClassDef, name: &str) -> Option<&'a Expr> {
    class.body.iter().rev().find_map(|stmt| match stmt {
        Stmt::Assign(assign)
            if assign
                .targets
                .iter()
                .any(|t| matches!(t, Expr::Name(n) if n.id.as_str() == name)) =>
        {
            Some(&*assign.value)
        }
        _ => None,
    })
}

/// The models a class defines or extends: its `_name`, or every model of
/// its `_inherit`.
fn class_models(class: &StmtClassDef) -> Vec<String> {
    if let Some(name) = class_value(class, "_name").and_then(literal) {
        return vec![name.to_string()];
    }
    match class_value(class, "_inherit") {
        Some(Expr::List(list)) => list.elts.iter().filter_map(literal).map(str::to_string).collect(),
        Some(Expr::Tuple(tuple)) => tuple.elts.iter().filter_map(literal).map(str::to_string).collect(),
        Some(other) => literal(other).map(str::to_string).into_iter().collect(),
        None => Vec::new(),
    }
}

/// A path that does not resolve: the missing field and the model it is
/// missing on. `None` when the path resolves, or when it goes through a
/// model or a field the index cannot follow.
fn missing(closure: &Closure, model: &str, path: &str) -> Option<(String, String)> {
    let segments: Vec<&str> = path.split('.').collect();
    if segments
        .iter()
        .any(|s| s.is_empty() || !s.chars().all(|c| c.is_alphanumeric() || c == '_'))
    {
        return None;
    }
    // Studio fields (`x_...`) live in the database only.
    if segments.iter().any(|s| s.starts_with("x_")) {
        return None;
    }
    let mut current = model.to_string();
    for (i, segment) in segments.iter().enumerate() {
        let fields = closure.fields(&current)?;
        let Some(field) = fields.get(*segment) else {
            return Some((segment.to_string(), current));
        };
        if i + 1 == segments.len() {
            return None;
        }
        current = field.comodel.clone()?;
    }
    None
}

struct Checker<'a, 'r> {
    closure: Closure,
    dirs: Vec<PathBuf>,
    reporter: &'a mut Reporter<'r>,
}

impl Checker<'_, '_> {
    /// Checks `path` on any of `models`: reported only when it resolves on
    /// none of them.
    fn path(&mut self, models: &[String], path: &str, at: TextSize) {
        let known: Vec<&String> = models.iter().filter(|m| self.closure.fields(m).is_some()).collect();
        if known.is_empty() || known.len() < models.len() {
            return;
        }
        let mut failures = Vec::new();
        for model in &known {
            match missing(&self.closure, model, path) {
                Some(failure) => failures.push(failure),
                None => return,
            }
        }
        let Some((field, model)) = failures.into_iter().next() else {
            return;
        };
        let defining = modules_defining(&self.dirs, &model, &field);
        let hint = if defining.is_empty() {
            String::new()
        } else {
            format!(
                "; it is defined in `{}`, which `depends` does not reach",
                defining.join("`, `")
            )
        };
        self.reporter.report(
            &PYTHON_FIELD_NOT_FOUND,
            at,
            format!("Field `{field}` does not exist on `{model}`{hint}"),
        );
    }

    fn class(&mut self, class: &StmtClassDef) {
        let models = class_models(class);
        // A mixin's methods may name fields of the models that inherit it.
        if models.is_empty() || models.iter().any(|m| self.closure.is_abstract(m)) {
            return;
        }
        // Decorators naming fields.
        for method in methods(class) {
            for decorator in &method.decorator_list {
                let Expr::Call(call) = &decorator.expression else {
                    continue;
                };
                if !matches!(func_name(&call.func), "depends" | "onchange" | "constrains") {
                    continue;
                }
                for arg in call.arguments.args.iter() {
                    if let Some(path) = literal(arg) {
                        self.path(&models, path, arg.start());
                    }
                }
            }
        }
        // Field parameters.
        for (_, call) in field_definitions(class) {
            let kind = match &*call.func {
                Expr::Attribute(attribute) => attribute.attr.to_string(),
                _ => continue,
            };
            let keyword = |name: &str| {
                call.arguments
                    .keywords
                    .iter()
                    .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == name))
                    .map(|k| &k.value)
            };
            if let Some(value) = keyword("related") {
                if let Some(path) = literal(value) {
                    self.path(&models, path, value.start());
                }
            }
            if let Some(value) = keyword("currency_field") {
                if let Some(path) = literal(value) {
                    self.path(&models, path, value.start());
                }
            }
            let comodel = keyword("comodel_name")
                .or_else(|| call.arguments.args.first())
                .and_then(literal)
                .filter(|_| matches!(kind.as_str(), "Many2one" | "One2many" | "Many2many"));
            let Some(comodel) = comodel.map(|c| vec![c.to_string()]) else {
                continue;
            };
            if kind == "One2many" {
                let inverse = keyword("inverse_name").or_else(|| call.arguments.args.get(1));
                if let Some(value) = inverse {
                    if let Some(path) = literal(value) {
                        self.path(&comodel, path, value.start());
                    }
                }
            }
            // A literal domain names fields of the comodel.
            if let Some(Expr::List(domain)) = keyword("domain") {
                for term in &domain.elts {
                    let first = match term {
                        Expr::Tuple(t) => t.elts.first(),
                        Expr::List(l) => l.elts.first(),
                        _ => None,
                    };
                    if let Some(first) = first {
                        if let Some(path) = literal(first) {
                            self.path(&comodel, path, first.start());
                        }
                    }
                }
            }
        }
        // `_rec_name` and `_order`.
        if let Some(value) = class_value(class, "_rec_name") {
            if let Some(name) = literal(value) {
                self.path(&models, name, value.start());
            }
        }
        if let Some(value) = class_value(class, "_order") {
            if let Some(order) = literal(value).filter(|o| !o.contains(['(', '"'])) {
                for part in order.split(',') {
                    if let Some(field) = part.split_whitespace().next() {
                        self.path(&models, field, value.start());
                    }
                }
            }
        }
        // `self.mapped("a.b")`, `self.filtered("a")`, `self.sorted("a")`.
        let mut calls = Vec::new();
        for method in methods(class) {
            walk(&method.body, |node, _| {
                let Node::Expr(Expr::Call(call)) = node else { return };
                let Expr::Attribute(attribute) = &*call.func else {
                    return;
                };
                if !matches!(attribute.attr.as_str(), "mapped" | "filtered" | "sorted")
                    || !matches!(&*attribute.value, Expr::Name(n) if n.id.as_str() == "self")
                {
                    return;
                }
                if let Some(arg) = call.arguments.args.first() {
                    if let Some(path) = literal(arg) {
                        calls.push((path.to_string(), arg.start()));
                    }
                }
            });
        }
        for (path, at) in calls {
            self.path(&models, &path, at);
        }
    }
}

fn check_python_fields(ctx: &PythonContext, reporter: &mut Reporter) {
    let Some(module) = ctx.module else { return };
    let local_base = module
        .path
        .parent()
        .is_some_and(|dir| dir.join("base").join("__manifest__.py").is_file());
    if ctx.settings.addons_path.is_empty() && !local_base {
        return;
    }
    let Some(closure) = closure(module, &ctx.settings.addons_path) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = module.path.parent().map(PathBuf::from).into_iter().collect();
    dirs.extend(ctx.settings.addons_path.iter().cloned());
    let mut checker = Checker {
        closure,
        dirs,
        reporter,
    };
    for class in classes(ctx.parsed.suite()) {
        if ctx.semantic.odoo_model_kind(class).is_none() {
            continue;
        }
        checker.class(class);
    }
}
