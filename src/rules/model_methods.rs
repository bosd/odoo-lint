//! Methods named in fields and views, checked against the models of the
//! addons path: ODOO010 for `compute=`, `inverse=` and the like, ODOO011
//! for the buttons of views. Silent without the dependencies of a module
//! (see `addons-path`) and for models with methods the index cannot know.

use crate::checker::{ModuleInfo, PythonContext, Reporter};
use crate::index::{class_models, closure, Closure};
use crate::rules::python::{classes, field_definitions};
use crate::rules::upgrade::view_archs;
use crate::rules::xml::{at, child_field, is, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use crate::settings::Settings;
use roxmltree::Node;
use ruff_text_size::Ranged;

pub const FIELD_METHOD_NOT_FOUND: Rule = Rule {
    code: "ODOO010",
    name: "field-method-not-found",
    summary: "A field names a method (`compute=`, `inverse=`, ...) that its model does not have.",
    doc: r#"
## What it does

Reports the methods named by string in a field definition, `compute`,
`inverse`, `search`, `group_expand` and a `selection` method, that the
model does not have, as the modules the module's `depends` reach define it,
Odoo's own model methods included.

## Why is this bad?

The field fails as soon as Odoo computes, writes or searches it: an
`AttributeError` at install for a stored computed field with records, else
when it is used. It typically happens after a method was renamed or moved
to a module that is not in `depends`, or when a field was copied from
another model without its method.

## Configuration

Like [ODOO004](ODOO004.md), the check needs the code of the dependencies,
Odoo's included, in `addons-path`. A model with methods odoo-lint cannot
see (a Python mixin outside the addons, a parent outside the closure) is not
checked.
"#,
    check: Check::Python(check_field_methods),
    min_odoo: None,
    max_odoo: None,
};

pub const BUTTON_METHOD_NOT_FOUND: Rule = Rule {
    code: "ODOO011",
    name: "button-method-not-found",
    summary: "A view's `<button type=\"object\">` calls a method its model does not have.",
    doc: r#"
## What it does

Reports `<button type="object" name="...">` in views whose method the
view's model does not have, as the modules the module's `depends` reach
define it. Buttons in an embedded list or form are checked against that
field's comodel.

## Why is this bad?

Odoo installs the view, but the button fails when clicked: "The method ...
does not exist on the model ...". It typically happens after a method was
renamed, or when it comes from a module that is not in `depends`.

## Configuration

Like [ODOO004](ODOO004.md), the check needs the code of the dependencies,
Odoo's included, in `addons-path`. A model with methods odoo-lint cannot
see is not checked.
"#,
    check: Check::Xml(check_button_methods),
    min_odoo: None,
    max_odoo: None,
};

/// The closure of `module` when the checks across modules can run.
pub(crate) fn closure_for(module: &ModuleInfo, settings: &Settings) -> Option<Closure> {
    let local_base = module
        .path
        .parent()
        .is_some_and(|dir| dir.join("base").join("__manifest__.py").is_file());
    if settings.addons_path.is_empty() && !local_base {
        return None;
    }
    closure(module, &settings.addons_path)
}

/// Field keywords that name a method of the model.
const METHOD_KEYWORDS: &[&str] = &["compute", "inverse", "search", "group_expand"];

fn check_field_methods(ctx: &PythonContext, reporter: &mut Reporter) {
    let Some(module) = ctx.module else { return };
    let Some(closure) = closure_for(module, ctx.settings) else {
        return;
    };
    for class in classes(ctx.parsed.suite()) {
        if ctx.semantic.odoo_model_kind(class).is_none() {
            continue;
        }
        let models = class_models(class);
        let Some(model) = models.first() else { continue };
        let Some(methods) = closure.methods(model) else {
            continue;
        };
        for (assign, call) in field_definitions(class) {
            let mut named: Vec<(&str, &ruff_python_ast::Expr)> = call
                .arguments
                .keywords
                .iter()
                .filter_map(|k| {
                    let keyword = k.arg.as_ref()?.as_str();
                    METHOD_KEYWORDS.contains(&keyword).then_some((keyword, &k.value))
                })
                .collect();
            // `fields.Selection("_selection_values")` or `selection="..."`.
            if crate::semantic::func_name(&call.func) == "Selection" {
                let selection = call
                    .arguments
                    .keywords
                    .iter()
                    .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "selection"))
                    .map(|k| &k.value)
                    .or_else(|| call.arguments.args.first());
                named.extend(selection.map(|value| ("selection", value)));
            }
            for (keyword, value) in named {
                let Some(method) = value.as_string_literal_expr().map(|s| s.value.to_str()) else {
                    continue;
                };
                if method.is_empty() || methods.contains(method) {
                    continue;
                }
                let field = assign
                    .targets
                    .first()
                    .and_then(|t| t.as_name_expr())
                    .map(|n| n.id.to_string())
                    .unwrap_or_default();
                reporter.report(
                    &FIELD_METHOD_NOT_FOUND,
                    value.start(),
                    format!("Method `{method}` (`{keyword}` of `{field}`) does not exist on `{model}`"),
                );
            }
        }
    }
}

/// The model the buttons under `node` act on: the view's model, through
/// the relational fields whose sub-views (or `<groupby>`) `node` is in.
pub(crate) fn node_model(closure: &Closure, arch: Node, node: Node, model: &str) -> Option<String> {
    let mut via: Vec<&str> = node
        .ancestors()
        .skip(1)
        .take_while(|a| *a != arch)
        // A list's `<groupby>` shows the records of the field's comodel.
        .filter(|a| is(*a, "field") || is(*a, "groupby"))
        .filter_map(|a| a.attribute("name"))
        .collect();
    via.reverse();
    let mut current = model.to_string();
    for step in via {
        current = closure.fields(&current)?.get(step)?.comodel.clone()?;
    }
    Some(current)
}

/// Whether `button`, in the arch of an extension, may end up in a field's
/// sub-view: under a field or a `<groupby>`, or inserted by a spec that
/// points into a field, a list or a nested form.
pub(crate) fn lands_in_field(arch: Node, button: Node) -> bool {
    button.ancestors().skip(1).take_while(|a| *a != arch).any(|a| {
        if is(a, "field") || is(a, "groupby") {
            return true;
        }
        let Some(expr) = a.attribute("expr").filter(|_| is(a, "xpath")) else {
            return false;
        };
        let rest = expr
            .strip_prefix("//form")
            .or_else(|| expr.strip_prefix("/form"))
            .unwrap_or(expr);
        ["field", "list", "tree", "kanban", "form"]
            .iter()
            .any(|tag| rest.contains(tag))
    })
}

fn check_button_methods(ctx: &XmlContext, reporter: &mut XmlReporter) {
    let Some(closure) = closure_for(ctx.module, ctx.settings) else {
        return;
    };
    for file in ctx.files {
        for (record, arch) in view_archs(file) {
            let Some(model) = child_field(record, "model").and_then(|m| m.text()).map(str::trim) else {
                continue;
            };
            let extension = child_field(record, "inherit_id").is_some();
            for button in arch.descendants().filter(|n| is(*n, "button")) {
                if button.attribute("type") != Some("object") {
                    continue;
                }
                let Some(name) = button.attribute("name").map(str::trim) else {
                    continue;
                };
                if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                // In an extension, a button may land in a sub-view the
                // extension cannot see: one under a field, or inserted by a
                // spec on or into a field.
                if extension && lands_in_field(arch, button) {
                    continue;
                }
                let Some(target) = node_model(&closure, arch, button, model) else {
                    continue;
                };
                let Some(methods) = closure.methods(&target) else {
                    continue;
                };
                if !methods.contains(name) {
                    reporter.report(
                        &BUTTON_METHOD_NOT_FOUND,
                        file,
                        at(file, button),
                        format!("Method `{name}` does not exist on `{target}`"),
                    );
                }
            }
        }
    }
}
