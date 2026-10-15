//! Changes of Odoo's JavaScript framework that break a module's assets:
//! `patch()` and the field registry in 17.0, the `rpc` service and the
//! legacy `Widget` in 18.0. Checked on the parsed source (oxc).

use crate::checker::{JsContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::rules::{Check, Rule};
use oxc_ast::ast::{Argument, CallExpression, Expression, ImportDeclaration};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use ruff_text_size::TextSize;

const ODOO_17: Option<OdooVersion> = Some(OdooVersion::new(17, 0));
const ODOO_18: Option<OdooVersion> = Some(OdooVersion::new(18, 0));

pub const PATCH_NAME: Rule = Rule {
    code: "U1722",
    name: "upgrade-js-patch-name",
    summary: "`patch()` is called with a patch name, dropped in Odoo 17.0.",
    doc: r#"
## What it does

Reports `patch(target, "name", extension)`.

## Why is this bad?

Odoo 17.0 changed the signature to `patch(target, extension)`: the name is
gone. With three arguments the string is taken as the extension and the
patch does nothing useful.

## Fix safety

Safe: the name argument goes.
"#,
    check: Check::Js(check),
    min_odoo: ODOO_17,
    max_odoo: None,
};

pub const UNPATCH: Rule = Rule {
    code: "U1723",
    name: "upgrade-js-unpatch",
    summary: "`unpatch()`, removed in Odoo 17.0.",
    doc: r#"
## What it does

Reports calls to `unpatch()` and its import from `@web/core/utils/patch`.

## Why is this bad?

Odoo 17.0 removed `unpatch`: `patch()` returns a function that undoes the
patch. Keep that function and call it instead.
"#,
    check: Check::Js(check),
    min_odoo: ODOO_17,
    max_odoo: None,
};

pub const FIELD_REGISTRY_COMPONENT: Rule = Rule {
    code: "U1724",
    name: "upgrade-js-field-registry-component",
    summary: "A component is added to the field registry directly, not in a descriptor as Odoo 17.0 expects.",
    doc: r#"
## What it does

Reports `registry.category("fields").add("name", SomeField)` where the
value is a class.

## Why is this bad?

From Odoo 17.0 the field registry holds descriptors:
`{component: SomeField, supportedTypes: [...], extractProps: ...}`. The
web client reads `.component`, so a bare class breaks every view using the
widget. The static `supportedTypes`, `displayName` and `extractProps` of
the class move into the descriptor, `extractProps` with a new signature;
there is no automatic fix.
"#,
    check: Check::Js(check),
    min_odoo: ODOO_17,
    max_odoo: None,
};

pub const RPC_SERVICE: Rule = Rule {
    code: "U1812",
    name: "upgrade-js-rpc-service",
    summary: "The `rpc` service, removed in Odoo 18.0.",
    doc: r#"
## What it does

Reports `useService("rpc")` and `services.rpc`.

## Why is this bad?

Odoo 18.0 removed the `rpc` service; the component fails to start. Import
the function instead:

```js
import { rpc } from "@web/core/network/rpc";
```
"#,
    check: Check::Js(check),
    min_odoo: ODOO_18,
    max_odoo: None,
};

pub const LEGACY_WIDGET: Rule = Rule {
    code: "U1813",
    name: "upgrade-js-legacy-widget",
    summary: "The legacy `Widget` is imported, removed in Odoo 18.0.",
    doc: r#"
## What it does

Reports imports of `@web/legacy/js/core/widget`.

## Why is this bad?

Odoo 18.0 removed the legacy `Widget` class: the module fails to load.
Rewrite the widget as an OWL component; on the website, `publicWidget`
remains until Odoo 20.0.
"#,
    check: Check::Js(check),
    min_odoo: ODOO_18,
    max_odoo: None,
};

fn offset(span: oxc_span::Span) -> TextSize {
    TextSize::from(span.start)
}

/// The string value of an argument, when it is a literal.
fn string_argument<'a>(argument: &'a Argument) -> Option<&'a str> {
    match argument.as_expression()? {
        Expression::StringLiteral(literal) => Some(literal.value.as_str()),
        _ => None,
    }
}

struct Checker<'r, 'c> {
    reporter: &'r mut Reporter<'c>,
}

impl<'a> Visit<'a> for Checker<'_, '_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        let callee = &call.callee;
        let args = &call.arguments;
        if callee.is_specific_id("patch") && args.len() >= 3 && string_argument(&args[1]).is_some() {
            let violation = self.reporter.report(
                &PATCH_NAME,
                offset(call.span),
                "`patch()` takes no name since Odoo 17.0: `patch(target, extension)`",
            );
            violation.fix = Some(Fix::safe(
                "Remove the patch name",
                vec![Edit::delete(
                    args[1].span().start as usize,
                    args[2].span().start as usize,
                )],
            ));
        }
        if callee.is_specific_id("unpatch") {
            self.reporter.report(
                &UNPATCH,
                offset(call.span),
                "`unpatch()` was removed in Odoo 17.0; call the function `patch()` returns",
            );
        }
        // `registry.category("fields").add("name", SomeField)`
        if let Expression::StaticMemberExpression(member) = callee {
            if member.property.name == "add" && args.len() >= 2 {
                if let Expression::CallExpression(category) = &member.object {
                    let is_fields = matches!(&category.callee, Expression::StaticMemberExpression(m) if m.property.name == "category")
                        && category.arguments.first().and_then(string_argument) == Some("fields");
                    let class = match args[1].as_expression() {
                        Some(Expression::Identifier(id)) => id.name.chars().next().is_some_and(char::is_uppercase),
                        Some(Expression::ClassExpression(_)) => true,
                        _ => false,
                    };
                    if is_fields && class {
                        self.reporter.report(
                            &FIELD_REGISTRY_COMPONENT,
                            offset(call.span),
                            "The field registry takes a descriptor since Odoo 17.0: `{component: ..., supportedTypes: [...]}`",
                        );
                    }
                }
            }
        }
        if callee.is_specific_id("useService") && args.first().and_then(string_argument) == Some("rpc") {
            self.reporter.report(
                &RPC_SERVICE,
                offset(call.span),
                "The `rpc` service was removed in Odoo 18.0; `import { rpc } from \"@web/core/network/rpc\"`",
            );
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_static_member_expression(&mut self, member: &oxc_ast::ast::StaticMemberExpression<'a>) {
        // `this.env.services.rpc`, `env.services.rpc`
        let on_services = matches!(&member.object, Expression::StaticMemberExpression(m) if m.property.name == "services")
            || member.object.is_specific_id("services");
        if member.property.name == "rpc" && on_services {
            self.reporter.report(
                &RPC_SERVICE,
                offset(member.span),
                "The `rpc` service was removed in Odoo 18.0; `import { rpc } from \"@web/core/network/rpc\"`",
            );
        }
        walk::walk_static_member_expression(self, member);
    }

    fn visit_import_declaration(&mut self, import: &ImportDeclaration<'a>) {
        let source = import.source.value.as_str();
        if source == "@web/legacy/js/core/widget" {
            self.reporter.report(
                &LEGACY_WIDGET,
                offset(import.span),
                "The legacy `Widget` was removed in Odoo 18.0; rewrite it as an OWL component",
            );
        }
        if source == "@web/core/utils/patch" {
            let imports_unpatch = import.specifiers.iter().flatten().any(|s| s.local().name == "unpatch");
            if imports_unpatch {
                self.reporter.report(
                    &UNPATCH,
                    offset(import.span),
                    "`unpatch` was removed from `@web/core/utils/patch` in Odoo 17.0",
                );
            }
        }
        walk::walk_import_declaration(self, import);
    }
}

fn check(ctx: &JsContext, reporter: &mut Reporter) {
    Checker { reporter }.visit_program(ctx.program);
}
