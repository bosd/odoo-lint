//! ODOO015-ODOO017: names the web client looks up in the assets of the
//! module and its dependencies: field widgets of views, and OWL templates
//! named by components and by other templates.

use crate::assets::{has_field_widget, module_assets, Assets};
use crate::checker::{JsContext, ModuleInfo, Reporter};
use crate::index::closure;
use crate::odoo_version::OdooVersion;
use crate::rules::upgrade::view_archs;
use crate::rules::xml::{at, is, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use oxc_ast::ast::{Expression, PropertyDefinition};
use oxc_ast_visit::{walk, Visit};
use roxmltree::Node;
use ruff_text_size::TextSize;
use std::path::PathBuf;
use std::sync::Arc;

const ODOO_16: Option<OdooVersion> = Some(OdooVersion::new(16, 0));

pub const WIDGET_NOT_FOUND: Rule = Rule {
    code: "ODOO015",
    name: "view-widget-not-found",
    summary: "A view uses a `widget` that no JavaScript of the module or its dependencies registers.",
    doc: r#"
## What it does

Reports `<field widget="...">` and `<widget name="...">` in views when no
module the view's module can see registers that name in the `fields` or
`view_widgets` registry of its JavaScript
(`registry.category("fields").add("name", ...)`, also for one view type,
`"list.name"`).

## Why is this bad?

The view loads, but the widget is not there: Odoo 17 and later log a warning
in the browser and show the field with its type's default widget, so the
intended behaviour (avatars, a status bar, a phone link) silently goes.
Usually a dependency is missing from the manifest, or the widget was renamed
in a newer Odoo.

## Limitations

Runs with an [addons path](../configuration.md#addons-path) that includes
Odoo's sources with their `static` folders, or in an Odoo source tree.
Search views are skipped: their `widget` names are not registry keys.
"#,
    check: Check::Xml(check_widgets),
    min_odoo: ODOO_16,
    max_odoo: None,
};

pub const TEMPLATE_NOT_FOUND: Rule = Rule {
    code: "ODOO016",
    name: "owl-template-not-found",
    summary: "A component's `static template` names an OWL template no module it can see defines.",
    doc: r#"
## What it does

Reports `static template = "name"` in a component class when no `t-name`
in the `static/src` templates of the module or its dependencies defines
that name.

## Why is this bad?

OWL fails to render the component: "Missing template: ...". Usually a
typo, a template that was renamed, or a dependency missing from the
manifest.

## Limitations

Runs with an addons path that includes Odoo's sources with their `static`
folders, or in an Odoo source tree.
"#,
    check: Check::Js(check_component_templates),
    min_odoo: ODOO_16,
    max_odoo: None,
};

pub const TEMPLATE_REFERENCE_NOT_FOUND: Rule = Rule {
    code: "ODOO017",
    name: "owl-template-reference-not-found",
    summary: "An OWL template inherits from or calls a template no module it can see defines.",
    doc: r#"
## What it does

Reports `t-inherit="name"` and `t-call="name"` in the OWL templates of the
module's assets when no module it can see defines a template `name`.

## Why is this bad?

An extension of a missing template fails to load the assets bundle; a
`t-call` of one fails where it renders. Usually the parent template was
renamed in a newer Odoo, or a dependency is missing from the manifest.
"#,
    check: Check::Xml(check_template_references),
    min_odoo: ODOO_16,
    max_odoo: None,
};

/// The assets of `module` and every module it can see, when Odoo's own
/// web client is among them with its JavaScript; `None` otherwise, as
/// every name would then look missing.
fn visible_assets(module: &ModuleInfo, addons_path: &[PathBuf]) -> Option<Vec<Arc<Assets>>> {
    let local_base = module
        .path
        .parent()
        .is_some_and(|dir| dir.join("base").join("__manifest__.py").is_file());
    if addons_path.is_empty() && !local_base {
        return None;
    }
    let closure = closure(module, addons_path)?;
    let web = closure.modules().iter().find(|m| m.name == "web")?;
    if module_assets(&web.path).is_empty() {
        return None;
    }
    Some(closure.modules().iter().map(|m| module_assets(&m.path)).collect())
}

fn check_widgets(ctx: &XmlContext, reporter: &mut XmlReporter) {
    let Some(assets) = visible_assets(ctx.module, &ctx.settings.addons_path) else {
        return;
    };
    for file in ctx.files {
        for (_, arch) in view_archs(file) {
            let Some(root) = arch.children().find(Node::is_element) else {
                continue;
            };
            if is(root, "search") {
                continue;
            }
            for node in arch.descendants().filter(Node::is_element) {
                if is(node, "field") {
                    let Some(widget) = node.attribute("widget").filter(|w| !w.is_empty()) else {
                        continue;
                    };
                    if !has_field_widget(assets.iter().map(|a| a.as_ref()), widget) {
                        reporter.report(
                            &WIDGET_NOT_FOUND,
                            file,
                            at(file, node),
                            format!("No module this one depends on registers the field widget `{widget}`"),
                        );
                    }
                } else if is(node, "widget") {
                    let Some(name) = node.attribute("name") else { continue };
                    if !assets.iter().any(|a| a.view_widgets.contains(name)) {
                        reporter.report(
                            &WIDGET_NOT_FOUND,
                            file,
                            at(file, node),
                            format!("No module this one depends on registers the view widget `{name}`"),
                        );
                    }
                }
            }
        }
    }
}

fn check_template_references(ctx: &XmlContext, reporter: &mut XmlReporter) {
    let Some(assets) = visible_assets(ctx.module, &ctx.settings.addons_path) else {
        return;
    };
    let known = |name: &str| assets.iter().any(|a| a.templates.contains(name));
    for file in ctx.files.iter().filter(|f| f.section == "assets") {
        let Some(root) = file.root() else { continue };
        for node in root.descendants().filter(Node::is_element) {
            for attribute in ["t-inherit", "t-call"] {
                let Some(name) = node.attribute(attribute) else {
                    continue;
                };
                // Dynamic names (`t-call="{{ ... }}"`) are not checked.
                if name.is_empty() || name.contains(['{', '(', ' ']) || known(name) {
                    continue;
                }
                reporter.report(
                    &TEMPLATE_REFERENCE_NOT_FOUND,
                    file,
                    at(file, node),
                    format!("`{attribute}=\"{name}\"`: no module this one depends on defines that template"),
                );
            }
        }
    }
}

struct Templates<'r, 'c> {
    known: &'r dyn Fn(&str) -> bool,
    reporter: &'r mut Reporter<'c>,
}

impl<'a> Visit<'a> for Templates<'_, '_> {
    fn visit_property_definition(&mut self, property: &PropertyDefinition<'a>) {
        let is_template = property.r#static && property.key.is_specific_static_name("template");
        if let (true, Some(Expression::StringLiteral(name))) = (is_template, &property.value) {
            // An empty name is set on purpose: a subclass or `setup` sets it.
            if !name.value.is_empty() && !(self.known)(name.value.as_str()) {
                self.reporter.report(
                    &TEMPLATE_NOT_FOUND,
                    TextSize::from(property.span.start),
                    format!(
                        "`static template = \"{}\"`: no module this one depends on defines that template",
                        name.value
                    ),
                );
            }
        }
        walk::walk_property_definition(self, property);
    }
}

fn check_component_templates(ctx: &JsContext, reporter: &mut Reporter) {
    let Some(module) = ctx.module else { return };
    let Some(assets) = visible_assets(module, &ctx.settings.addons_path) else {
        return;
    };
    let known = |name: &str| assets.iter().any(|a| a.templates.contains(name));
    Templates {
        known: &known,
        reporter,
    }
    .visit_program(ctx.program);
}
