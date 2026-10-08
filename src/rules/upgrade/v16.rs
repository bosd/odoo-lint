//! Changes of Odoo 16.0: view group checks, the asset bundles that went
//! away, translations in JSONB, and the reworked `odoo.http`.

use super::data_elements;
use crate::checker::{ManifestContext, PythonContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::rules::python::{classes, methods, source_of};
use crate::rules::xml::{at, child_field, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use crate::semantic::func_name;
use crate::visit::{walk, Node};
use ruff_python_ast::{Expr, Stmt};
use ruff_text_size::{Ranged, TextRange};

const ODOO_16: Option<OdooVersion> = Some(OdooVersion::new(16, 0));

fn rename(range: TextRange, new: &str) -> Edit {
    Edit::replace(range.start().to_usize(), range.end().to_usize(), new)
}

// --- XML ------------------------------------------------------------------

pub const XML_EXTENSION_GROUPS: Rule = Rule {
    code: "U1601",
    name: "upgrade-extension-view-groups",
    summary: "An extension view sets `groups_id`, rejected since Odoo 16.0.",
    doc: r#"
## What it does

Reports `groups_id` on views that extend another (`inherit_id` set, `mode`
not `primary`), and `groups` on `<template inherit_id=...>`.

## Why is this bad?

Odoo 16.0 rejects groups on extension views of every type (15.0 did so
for QWeb views only): the module fails to install. Put `groups="..."` on
the elements the view adds instead, or make the view `primary` if it
should be a separate view.
"#,
    check: Check::Xml(check_extension_groups),
    min_odoo: ODOO_16,
    max_odoo: None,
};

fn check_extension_groups(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for record in data_elements(file, &["record"]).filter(|r| r.attribute("model") == Some("ir.ui.view")) {
            let primary = child_field(record, "mode").and_then(|m| m.text()).map(str::trim) == Some("primary");
            if child_field(record, "inherit_id").is_none() || primary {
                continue;
            }
            if let Some(groups) = child_field(record, "groups_id") {
                reporter.report(
                    &XML_EXTENSION_GROUPS,
                    file,
                    at(file, groups),
                    "Extension views cannot have groups since Odoo 16.0; set `groups` on the added elements",
                );
            }
        }
        for template in data_elements(file, &["template"]) {
            if template.attribute("inherit_id").is_some()
                && template.attribute("groups").is_some()
                && template.attribute("primary") != Some("True")
            {
                reporter.report(
                    &XML_EXTENSION_GROUPS,
                    file,
                    at(file, template),
                    "Extension views cannot have groups since Odoo 16.0; set `groups` on the added elements",
                );
            }
        }
    }
}

pub const XML_HTML_FIELD_TYPE: Rule = Rule {
    code: "U1602",
    name: "upgrade-html-field-type",
    summary: "`body_html` of a mail template loaded with `type=\"xml\"`, deprecated in Odoo 16.0.",
    doc: r#"
## What it does

Reports `<field name="body_html" type="xml">` in `mail.template` records.

## Why is this bad?

Odoo 16.0 serialises HTML fields as HTML and warns about `type="xml"`
("HTML field is declared as type=xml"): self-closing tags such as `<br/>`
and empty elements can come out differently.

## Fix safety

Safe: the type becomes `html`.
"#,
    check: Check::Xml(check_html_field_type),
    min_odoo: ODOO_16,
    max_odoo: None,
};

fn check_html_field_type(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for record in data_elements(file, &["record"]).filter(|r| r.attribute("model") == Some("mail.template")) {
            let Some(field) = child_field(record, "body_html") else {
                continue;
            };
            let Some(kind) = field.attributes().find(|a| a.name() == "type" && a.value() == "xml") else {
                continue;
            };
            reporter
                .report(
                    &XML_HTML_FIELD_TYPE,
                    file,
                    at(file, field),
                    "HTML fields take `type=\"html\"` since Odoo 16.0",
                )
                .fix = Some(Fix::safe(
                "Use `type=\"html\"`",
                vec![Edit::replace(kind.range_value().start, kind.range_value().end, "html")],
            ));
        }
    }
}

// --- Manifest -------------------------------------------------------------

pub const ASSETS_QWEB: Rule = Rule {
    code: "U1603",
    name: "upgrade-assets-qweb",
    summary: "The manifest adds templates to `web.assets_qweb`, removed in Odoo 16.0.",
    doc: r#"
## What it does

Reports the `web.assets_qweb` bundle in the manifest's `assets`.

## Why is this bad?

Odoo 16.0 removed the bundle: its templates are silently never loaded, and
the client fails with "Missing template". Templates go in
`web.assets_backend` (or the bundle of the code that uses them).

## Fix safety

Safe when the manifest has no `web.assets_backend` yet: the bundle is
renamed. Otherwise move the entries by hand.
"#,
    check: Check::Manifest(check_assets_qweb),
    min_odoo: ODOO_16,
    max_odoo: None,
};

fn check_assets_qweb(ctx: &ManifestContext, reporter: &mut Reporter) {
    let Some(Expr::Dict(assets)) = ctx.manifest.get("assets") else {
        return;
    };
    let key_of = |name: &str| {
        assets.items.iter().find_map(|item| {
            let key = item.key.as_ref()?;
            (key.as_string_literal_expr()?.value.to_str() == name).then_some(key)
        })
    };
    let Some(key) = key_of("web.assets_qweb") else { return };
    let fix = key_of("web.assets_backend").is_none().then(|| {
        let quote = source_of(ctx.source, key).chars().next().unwrap_or('"');
        Fix::safe(
            "Use `web.assets_backend`",
            vec![rename(key.range(), &format!("{quote}web.assets_backend{quote}"))],
        )
    });
    reporter
        .report(
            &ASSETS_QWEB,
            key.start(),
            "`web.assets_qweb` was removed in Odoo 16.0; its templates are not loaded",
        )
        .fix = fix;
}

const REMOVED_BUNDLES: &[&str] = &[
    "web.assets_common_minimal",
    "web.assets_common_lazy",
    "web._assets_common_styles",
    "web._assets_common_scripts",
];

pub const REMOVED_BUNDLES_RULE: Rule = Rule {
    code: "U1604",
    name: "upgrade-removed-asset-bundles-16",
    summary: "The manifest uses an asset bundle removed in Odoo 16.0.",
    doc: r#"
## What it does

Reports `web.assets_common_minimal`, `web.assets_common_lazy`,
`web._assets_common_styles` and `web._assets_common_scripts`, as bundles
and as `include` targets in the manifest's `assets`.

## Why is this bad?

Odoo 16.0 removed them: their files are silently never loaded. Choose
the bundle that now holds what you need, usually `web.assets_backend` or
`web.assets_frontend`.
"#,
    check: Check::Manifest(check_removed_bundles),
    min_odoo: ODOO_16,
    max_odoo: None,
};

fn check_removed_bundles(ctx: &ManifestContext, reporter: &mut Reporter) {
    let Some(Expr::Dict(assets)) = ctx.manifest.get("assets") else {
        return;
    };
    let mut report = |expr: &Expr| {
        let Some(name) = expr.as_string_literal_expr().map(|s| s.value.to_str()) else {
            return;
        };
        if REMOVED_BUNDLES.contains(&name) {
            reporter.report(
                &REMOVED_BUNDLES_RULE,
                expr.start(),
                format!("The `{name}` bundle was removed in Odoo 16.0"),
            );
        }
    };
    for item in &assets.items {
        if let Some(key) = &item.key {
            report(key);
        }
        let Expr::List(entries) = &item.value else { continue };
        for entry in &entries.elts {
            if let Expr::Tuple(tuple) = entry {
                if let [_, target, ..] = tuple.elts.as_slice() {
                    report(target);
                }
            }
        }
    }
}

pub const MANIFEST_QWEB: Rule = Rule {
    code: "U1605",
    name: "upgrade-manifest-qweb",
    summary: "The manifest lists templates under `qweb`, which Odoo ignores.",
    doc: r#"
## What it does

Reports the manifest key `qweb`.

## Why is this bad?

Odoo no longer reads it (15.0 already ignored it): the templates are never
loaded. Add them to a bundle in `assets`, usually `web.assets_backend`.
"#,
    check: Check::Manifest(check_manifest_qweb),
    min_odoo: ODOO_16,
    max_odoo: None,
};

fn check_manifest_qweb(ctx: &ManifestContext, reporter: &mut Reporter) {
    if let Some((key, _)) = ctx.manifest.entry("qweb") {
        reporter.report(
            &MANIFEST_QWEB,
            key.start(),
            "`qweb` is ignored; add the templates to `assets`",
        );
    }
}

// --- Python ---------------------------------------------------------------

pub const IR_TRANSLATION: Rule = Rule {
    code: "U1606",
    name: "upgrade-ir-translation",
    summary: "Python code uses `ir.translation`, removed in Odoo 16.0.",
    doc: r#"
## What it does

Reports the model name `ir.translation` in Python.

## Why is this bad?

Odoo 16.0 stores translations in the field itself (JSONB): the model is
gone and `env["ir.translation"]` raises `KeyError`. Read and write with
`record.with_context(lang=...)`, or use `update_field_translations` and
`get_field_translations`.
"#,
    check: Check::Python(check_ir_translation),
    min_odoo: ODOO_16,
    max_odoo: None,
};

fn check_ir_translation(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(expr @ Expr::StringLiteral(literal)) = node else {
            return;
        };
        if literal.value.to_str() == "ir.translation" {
            reporter.report(
                &IR_TRANSLATION,
                expr.start(),
                "`ir.translation` was removed in Odoo 16.0",
            );
        }
    });
}

pub const REQUEST_API: Rule = Rule {
    code: "U1607",
    name: "upgrade-request-api",
    summary: "`request.jsonrequest`, or an assignment to `request.context`/`request.uid`, removed in Odoo 16.0.",
    doc: r#"
## What it does

Reports `request.jsonrequest` and assignments to `request.context`,
`request.uid` and `request.env`.

## Why is this bad?

Odoo 16.0 reworked `odoo.http`: `jsonrequest` is gone and the setters raise
`NotImplementedError`. Use `request.get_json_data()`,
`request.update_context(**values)` and `request.update_env(user=...)`.

## Fix safety

Safe for `request.jsonrequest`, which becomes `request.get_json_data()`.
"#,
    check: Check::Python(check_request_api),
    min_odoo: ODOO_16,
    max_odoo: None,
};

fn is_request(expr: &Expr) -> bool {
    matches!(expr, Expr::Name(n) if n.id.as_str() == "request")
}

fn check_request_api(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| match node {
        Node::Expr(Expr::Attribute(attribute))
            if is_request(&attribute.value) && attribute.attr.as_str() == "jsonrequest" =>
        {
            reporter
                .report(
                    &REQUEST_API,
                    attribute.start(),
                    "`request.jsonrequest` was removed in Odoo 16.0; use `request.get_json_data()`",
                )
                .fix = Some(Fix::safe(
                "Use `get_json_data()`",
                vec![rename(attribute.attr.range(), "get_json_data()")],
            ));
        }
        Node::Stmt(Stmt::Assign(assign)) => {
            for target in &assign.targets {
                let Expr::Attribute(attribute) = target else { continue };
                let name = attribute.attr.as_str();
                if !is_request(&attribute.value) || !matches!(name, "context" | "uid" | "env") {
                    continue;
                }
                let new = if name == "context" {
                    "request.update_context(**values)"
                } else {
                    "request.update_env(...)"
                };
                reporter.report(
                    &REQUEST_API,
                    target.start(),
                    format!("`request.{name}` cannot be assigned since Odoo 16.0; use `{new}`"),
                );
            }
        }
        _ => {}
    });
}

pub const BINARY_CONTENT: Rule = Rule {
    code: "U1608",
    name: "upgrade-binary-content",
    summary: "`ir.http.binary_content()`, removed in Odoo 16.0.",
    doc: r#"
## What it does

Reports calls to `binary_content()`.

## Why is this bad?

Odoo 16.0 removed `ir.http.binary_content`: the call raises
`AttributeError`. Use `env["ir.binary"]._get_stream_from(...)` (or
`_get_image_stream_from`) and `stream.get_response()`.
"#,
    check: Check::Python(check_binary_content),
    min_odoo: ODOO_16,
    max_odoo: None,
};

fn check_binary_content(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        if matches!(&*call.func, Expr::Attribute(a) if a.attr.as_str() == "binary_content") {
            reporter.report(
                &BINARY_CONTENT,
                call.start(),
                "`binary_content()` was removed in Odoo 16.0; use `ir.binary`",
            );
        }
    });
}

pub const SEARCH_ARGS: Rule = Rule {
    code: "U1609",
    name: "upgrade-search-args",
    summary: "`search(args=...)`, renamed `domain=` in Odoo 16.0.",
    doc: r#"
## What it does

Reports the `args` keyword in calls to `search` and `search_count`.

## Why is this bad?

Odoo 16.0 renamed the parameter `domain`: the keyword call raises
`TypeError`.

## Fix safety

Safe: the keyword is renamed.
"#,
    check: Check::Python(check_search_args),
    min_odoo: ODOO_16,
    max_odoo: None,
};

fn check_search_args(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        if !matches!(func_name(&call.func), "search" | "search_count") || !matches!(&*call.func, Expr::Attribute(_)) {
            return;
        }
        for keyword in &call.arguments.keywords {
            let Some(arg) = keyword.arg.as_ref().filter(|a| a.as_str() == "args") else {
                continue;
            };
            reporter
                .report(
                    &SEARCH_ARGS,
                    keyword.start(),
                    "`search(args=)` is `domain=` since Odoo 16.0",
                )
                .fix = Some(Fix::safe("Rename to `domain`", vec![rename(arg.range(), "domain")]));
        }
    });
}

pub const OSV_QUERY: Rule = Rule {
    code: "U1610",
    name: "upgrade-osv-query",
    summary: "`odoo.osv.query`, moved to `odoo.tools.query` in Odoo 16.0.",
    doc: r#"
## What it does

Reports imports from `odoo.osv.query`.

## Why is this bad?

Odoo 16.0 moved the module to `odoo.tools.query`: the import fails.

## Fix safety

Safe: the module is renamed.
"#,
    check: Check::Python(check_osv_query),
    min_odoo: ODOO_16,
    max_odoo: None,
};

fn check_osv_query(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Stmt(Stmt::ImportFrom(import)) = node else {
            return;
        };
        let Some(module) = import.module.as_ref().filter(|m| m.as_str() == "odoo.osv.query") else {
            return;
        };
        if import.level != 0 {
            return;
        }
        reporter
            .report(
                &OSV_QUERY,
                import.start(),
                "`odoo.osv.query` is `odoo.tools.query` since Odoo 16.0",
            )
            .fix = Some(Fix::safe(
            "Import from `odoo.tools.query`",
            vec![rename(module.range(), "odoo.tools.query")],
        ));
    });
}

pub const FIELDS_VIEW_GET: Rule = Rule {
    code: "U1611",
    name: "upgrade-fields-view-get",
    summary: "A `fields_view_get` override, no longer called by the web client since Odoo 16.0.",
    doc: r#"
## What it does

Reports `fields_view_get`, `_fields_view_get` and `load_views` overrides in
models.

## Why is this bad?

Since Odoo 16.0 the web client calls `get_views`: these overrides silently
stop changing the views, and 17.0 removes the methods. Override
`get_view`/`_get_view` (which return `{arch, id, model}`) or `get_views`
instead.
"#,
    check: Check::Python(check_fields_view_get),
    min_odoo: ODOO_16,
    max_odoo: None,
};

fn check_fields_view_get(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        if ctx.semantic.odoo_model_kind(class).is_none() {
            continue;
        }
        for method in
            methods(class).filter(|m| matches!(m.name.as_str(), "fields_view_get" | "_fields_view_get" | "load_views"))
        {
            reporter.report(
                &FIELDS_VIEW_GET,
                method.name.start(),
                format!(
                    "`{}` overrides are not called since Odoo 16.0; override `get_view`",
                    method.name
                ),
            );
        }
    }
}
