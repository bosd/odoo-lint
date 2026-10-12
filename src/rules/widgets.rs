//! ODOO012: fields in views shown without the widget that suits them, a
//! review comment as a check. Opt-in; needs the field types from the
//! addons path.

use crate::fix::{Edit, Fix};
use crate::index::Closure;
use crate::rules::model_methods::{closure_for, lands_in_field, node_model};
use crate::rules::upgrade::view_archs;
use crate::rules::xml::{at, child_field, is, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use roxmltree::Node;

pub const FIELD_WITHOUT_WIDGET: Rule = Rule {
    code: "ODOO012",
    name: "view-field-without-widget",
    summary: "A view shows a field without the widget that suits it.",
    doc: r#"
## What it does

Reports fields in form and list views that have no `widget` while one
suits their type, the way a reviewer would:

| Field | Widget |
| --- | --- |
| `Many2many` (without an embedded list or form) | `many2many_tags` |
| `Many2one` to `res.users` | `many2one_avatar_user` |
| `Selection` `state` in a form's `<header>` | `statusbar` |
| `Selection` `priority` | `priority` |
| `Integer` `color` | `color_picker` |
| `Char`/`Text` `domain` or `*_domain` | `domain` |

Hidden fields (`invisible="1"`, `column_invisible="1"`) are left alone.

## Why is this bad?

It works, but reads worse: a many2many as a list of ids, a state as a plain
selection, a domain as raw text. Odoo's own views use these widgets.

## Configuration

Opt-in: select it (`select = ["ALL", "ODOO012"]`), as it is a style
review rather than an error. It needs the field types of the dependencies,
Odoo's included, in `addons-path`.

## Fix safety

Unsafe: the fix adds the `widget` attribute, which changes how the view
looks; a domain widget may want `options="{'model': ...}"` as well.
"#,
    check: Check::Xml(check_widgets),
    min_odoo: None,
    max_odoo: None,
};

/// The widget that suits a field, if any.
fn suggestion(closure: &Closure, model: &str, field: Node) -> Option<&'static str> {
    let name = field.attribute("name")?;
    let fields = closure.fields(model)?;
    let definition = fields.get(name)?;
    let comodel = definition.comodel.as_deref();
    match definition.kind.as_str() {
        // An embedded list or form is a choice of its own.
        "Many2many" if !field.children().any(|c| c.is_element()) => Some("many2many_tags"),
        "Many2one" if comodel == Some("res.users") => Some("many2one_avatar_user"),
        "Selection" if name == "state" && field.ancestors().any(|a| is(a, "header")) => Some("statusbar"),
        "Selection" if name == "priority" => Some("priority"),
        "Integer" if name == "color" => Some("color_picker"),
        "Char" | "Text" if name == "domain" || name.ends_with("_domain") => Some("domain"),
        _ => None,
    }
}

const VIEW_TAGS: &[&str] = &["form", "list", "tree", "search", "kanban", "graph", "pivot", "calendar"];

/// The type of the view `id` (its arch's root tag), following the views it
/// extends.
fn view_type(views: &std::collections::HashMap<String, (&crate::index::View, usize)>, id: &str) -> Option<String> {
    let mut current = id.to_string();
    for _ in 0..16 {
        let (view, _) = views.get(&current)?;
        match &view.inherit_id {
            Some(parent) => current = parent.clone(),
            None => {
                let doc = roxmltree::Document::parse(view.arch.as_deref()?).ok()?;
                let root = doc.root_element();
                let root = if root.has_tag_name("__arch__") {
                    root.children().find(|c| c.is_element())?
                } else {
                    root
                };
                return Some(root.tag_name().name().to_string());
            }
        }
    }
    None
}

fn hidden(field: Node) -> bool {
    ["invisible", "column_invisible"].iter().any(|a| {
        field
            .attribute(*a)
            .is_some_and(|v| matches!(v.trim(), "1" | "True" | "true"))
    })
}

fn check_widgets(ctx: &XmlContext, reporter: &mut XmlReporter) {
    let Some(closure) = closure_for(ctx.module, ctx.settings) else {
        return;
    };
    let views = closure.views();
    for file in ctx.files {
        for (record, arch) in view_archs(file) {
            let Some(model) = child_field(record, "model").and_then(|m| m.text()).map(str::trim) else {
                continue;
            };
            let inherit_id = child_field(record, "inherit_id").and_then(|f| f.attribute("ref"));
            let extension = inherit_id.is_some();
            // The type of the view an extension changes: its root's tag.
            let parent_type = inherit_id.and_then(|id| {
                let id = if id.contains('.') {
                    id.to_string()
                } else {
                    format!("{}.{id}", ctx.module.name)
                };
                view_type(&views, &id)
            });
            for field in arch.descendants().filter(|n| is(*n, "field")) {
                if field.attribute("widget").is_some() || field.attribute("position").is_some() || hidden(field) {
                    continue;
                }
                if extension && lands_in_field(arch, field) {
                    continue;
                }
                // Search views and kanban templates show fields differently.
                let view = field
                    .ancestors()
                    .take_while(|a| *a != arch)
                    .find(|a| VIEW_TAGS.iter().any(|t| is(*a, t)))
                    .map(|v| v.tag_name().name().to_string())
                    .or_else(|| parent_type.clone());
                if !matches!(view.as_deref(), Some("form" | "list" | "tree")) {
                    continue;
                }
                let Some(target) = node_model(&closure, arch, field, model) else {
                    continue;
                };
                let Some(widget) = suggestion(&closure, &target, field) else {
                    continue;
                };
                let name = field.attribute("name").unwrap_or_default();
                let violation = reporter.report(
                    &FIELD_WITHOUT_WIDGET,
                    file,
                    at(file, field),
                    format!("Field `{name}` has no widget; `widget=\"{widget}\"` suits it"),
                );
                if let Some(attribute) = field.attributes().find(|a| a.name() == "name") {
                    // After the closing quote of `name="..."`.
                    let end = attribute.range_value().end + 1;
                    violation.fix = Some(Fix::unsafe_(
                        format!("Add `widget=\"{widget}\"`"),
                        vec![Edit::insert(end, format!(" widget=\"{widget}\""))],
                    ));
                }
            }
        }
    }
}
