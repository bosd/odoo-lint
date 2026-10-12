//! ODOO013: one2many and many2many fields in views whose comodel only
//! administrators may read. The web client reads the comodel's records as
//! the user who opens the view; any other user gets an AccessError.

use crate::index::Closure;
use crate::rules::model_methods::{closure_for, lands_in_field, node_model};
use crate::rules::upgrade::view_archs;
use crate::rules::widgets::{view_type, VIEW_TAGS};
use crate::rules::xml::{at, child_field, is, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use roxmltree::Node;

pub const RESTRICTED_COMODEL: Rule = Rule {
    code: "ODOO013",
    name: "view-field-restricted-comodel",
    summary: "A view shows a one2many or many2many field whose comodel only administrators may read, without `groups`.",
    doc: r#"
## What it does

Reports a one2many or many2many field in a view when every internal user
may read the view's model, but only administrators may read the field's
comodel: every
access right (`ir.model.access`) that grants reading it is for a group that
implies Access Rights (`base.group_erp_manager`), such as Settings. A
typical case is a many2many to `ir.model.fields` shown as tags.

The field is left alone when it is restricted with `groups`: on the field in
the view, on an element around it, or in its definition in Python.

## Why is this bad?

The web client reads the comodel's records (their `display_name`, or the
columns of the embedded list) as the user who opens the view. Any user outside the privileged group gets "Access
Error: You are not allowed to access ..." on opening the record. Nothing
shows it earlier: the module installs, and the administrator who tests it
may read everything.

`invisible` does not help: the field is still read. Only `groups` takes it
out of the view for those users.

## Configuration

The access rights come from the modules `depends` reaches, so a module that
tightens them is taken into account; the check needs the dependencies,
Odoo's included, in `addons-path`. Models a project restricts in another
way can be added:

```toml
[tool.odoo-lint.rules.view-field-restricted-comodel]
models = ["ir.config_parameter"]
```

## Limits

A many2one is not reported: Odoo reads its `display_name` as superuser.
A field with `groups` counts as restricted, whichever groups they are.
"#,
    check: Check::Xml(check_restricted),
    min_odoo: None,
    max_odoo: None,
};

/// Whether `node` or an element around it in the arch has `groups`.
fn gated(arch: Node, node: Node) -> bool {
    node.ancestors()
        .take_while(|a| *a != arch)
        .any(|a| a.attribute("groups").is_some_and(|g| !g.trim().is_empty()))
}

/// Methods through which a model serves reads its access rights deny, as
/// `hr.employee` does from `hr.employee.public`.
const ACCESS_OVERRIDES: &[&str] = &[
    "fetch",
    "read",
    "_read",
    "check_access",
    "check_access_rights",
    "_check_access",
];

fn overrides_access(closure: &Closure, model: &str) -> bool {
    closure
        .modules()
        .iter()
        .flat_map(|m| m.classes.iter())
        .filter(|c| c.name.as_deref() == Some(model) || (c.name.is_none() && c.inherit.iter().any(|i| i == model)))
        .any(|c| c.methods.iter().any(|m| ACCESS_OVERRIDES.contains(&m.as_str())))
}

fn restricted(closure: &Closure, extra: &[String], model: &str) -> bool {
    (extra.iter().any(|m| m == model) || closure.only_admins_read(model)) && !overrides_access(closure, model)
}

fn check_restricted(ctx: &XmlContext, reporter: &mut XmlReporter) {
    let Some(closure) = closure_for(ctx.module, ctx.settings) else {
        return;
    };
    let extra: Vec<String> = ctx
        .settings
        .config
        .rules()
        .view_field_restricted_comodel
        .as_ref()
        .and_then(|c| c.models.clone())
        .unwrap_or_default();
    let views = closure.views();
    for file in ctx.files {
        for (record, arch) in view_archs(file) {
            let Some(model) = child_field(record, "model").and_then(|m| m.text()).map(str::trim) else {
                continue;
            };
            // A view for some groups only, or of a model users cannot read.
            if child_field(record, "groups_id").is_some()
                || child_field(record, "group_ids").is_some()
                || closure.users_can_read(model) != Some(true)
            {
                continue;
            }
            let inherit_id = child_field(record, "inherit_id").and_then(|f| f.attribute("ref"));
            let extension = inherit_id.is_some();
            let parent_type = inherit_id.and_then(|id| {
                let id = if id.contains('.') {
                    id.to_string()
                } else {
                    format!("{}.{id}", ctx.module.name)
                };
                view_type(&views, &id)
            });
            for field in arch.descendants().filter(|n| is(*n, "field")) {
                if field.attribute("position").is_some() || gated(arch, field) {
                    continue;
                }
                if extension && lands_in_field(arch, field) {
                    continue;
                }
                let view = field
                    .ancestors()
                    .take_while(|a| *a != arch)
                    .find(|a| VIEW_TAGS.iter().any(|t| is(*a, t)))
                    .map(|v| v.tag_name().name().to_string())
                    .or_else(|| parent_type.clone());
                // A search view reads the comodel only when the user searches it.
                if view.as_deref().is_none_or(|v| v == "search") {
                    continue;
                }
                let Some(name) = field.attribute("name") else { continue };
                let Some(target) = node_model(&closure, arch, field, model) else {
                    continue;
                };
                let Some(fields) = closure.fields(&target) else {
                    continue;
                };
                let Some(definition) = fields.get(name) else { continue };
                // A many2one's `display_name` is read as superuser.
                let Some(comodel) = definition.comodel.as_deref().filter(|_| definition.is_x2many()) else {
                    continue;
                };
                if definition.groups.is_some() || !restricted(&closure, &extra, comodel) {
                    continue;
                }
                let mut message = format!(
                    "Field `{name}` shows `{comodel}`, which only privileged groups may read; \
                     other users get an AccessError. Restrict it with `groups`"
                );
                if field.attribute("invisible").is_some() {
                    message.push_str(" (`invisible` does not stop the web client from reading it)");
                }
                reporter.report(&RESTRICTED_COMODEL, file, at(file, field), message);
            }
        }
    }
}
