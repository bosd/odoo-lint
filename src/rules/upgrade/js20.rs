//! OWL 3, which Odoo 20.0 ships, as seen from a module's assets: the
//! component props schema, the hooks it no longer exports, `useEffect`'s new
//! meaning, and the template directives the compatibility layer renames.
//! Checked against Odoo 20.0's `web/static/lib/owl` and
//! `web/static/src/owl2/owl3_compatibility_layer.js`.

use crate::checker::{JsContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::rules::xml::{at, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use oxc_ast::ast::{
    Argument, CallExpression, Class, ClassElement, IdentifierReference, ImportDeclaration, ImportDeclarationSpecifier,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::{GetSpan, Span};
use roxmltree::Node;
use ruff_text_size::TextSize;
use std::collections::HashMap;

const ODOO_20: Option<OdooVersion> = Some(OdooVersion::new(20, 0));

pub const STATIC_PROPS: Rule = Rule {
    code: "U2031",
    name: "upgrade-owl3-static-props",
    summary: "A component declares `static props` or `static defaultProps`, which Odoo 20.0 refuses.",
    doc: r#"
## What it does

Reports `static props` and `static defaultProps` in classes.

## Why is this bad?

OWL 3 ignores them, and Odoo 20.0's `Component` throws when it finds them:
"... defines a static "props" or "defaultProps", which Owl 3 ignores".
Declare the schema with `useProps` instead:

```js
import { Component, t, useProps } from "@odoo/owl";

class Card extends Component {
    props = useProps({ title: t.string(), count: t.number().optional(0) });
}
```

There is no automatic fix: the schema has a new syntax.
"#,
    check: Check::Js(check),
    min_odoo: ODOO_20,
    max_odoo: None,
};

pub const REMOVED_HOOKS: Rule = Rule {
    code: "U2032",
    name: "upgrade-owl3-removed-hooks",
    summary: "A hook imported from `@odoo/owl` that OWL 3 (Odoo 20.0) no longer has.",
    doc: r#"
## What it does

Reports imports of `useState`, `reactive`, `useRef`, `useExternalListener`,
`onRendered` and `useChildSubEnv` from `@odoo/owl`.

## Why is this bad?

OWL 3 dropped them and Odoo 20.0's compatibility layer does not bring them
back: the import is `undefined` and the component fails when it is set up.

## Fix safety

Safe where the replacement is mechanical, as Odoo 20.0's own migration
does it: `useState(x)` and a one-argument `reactive(x)` become `proxy(x)`,
`useExternalListener` becomes `useListener`. `useRef` becomes a signal
(`root = signal.ref()` with `t-ref="this.root"`); `onRendered` and
`useChildSubEnv` need a look.
"#,
    check: Check::Js(check),
    min_odoo: ODOO_20,
    max_odoo: None,
};

pub const USE_EFFECT: Rule = Rule {
    code: "U2033",
    name: "upgrade-owl3-use-effect",
    summary: "`useEffect(fn, dependencies)`, whose meaning OWL 3 (Odoo 20.0) changed.",
    doc: r#"
## What it does

Reports `useEffect` from `@odoo/owl` called with a dependencies function.

## Why is this bad?

OWL 3's `useEffect(fn)` runs again when the signals `fn` reads change; it
takes no dependencies. Odoo 20.0's compatibility layer keeps the OWL 2
behaviour as `useLayoutEffect`.

## Fix safety

Safe: when every `useEffect` of the file has dependencies, it becomes
`useLayoutEffect`, as Odoo 20.0's own migration does.
"#,
    check: Check::Js(check),
    min_odoo: ODOO_20,
    max_odoo: None,
};

pub const TEMPLATE_DIRECTIVES: Rule = Rule {
    code: "U2034",
    name: "upgrade-owl3-template-directives",
    summary: "An OWL template uses `t-portal`, `t-esc` or a `t-ref` name, which OWL 3 (Odoo 20.0) changed.",
    doc: r#"
## What it does

Reports in the OWL templates of the module's assets:

- `t-portal`, which Odoo 20.0's compatibility layer provides as
  `t-custom-portal`;
- `t-esc`, removed from OWL 3 (`t-out`);
- `t-ref="name"`: OWL 3 refs are signals, `t-ref="this.name"` with
  `name = signal.ref()` in the component.

## Fix safety

Safe for `t-portal` and `t-esc`; a `t-ref` needs its component changed too.
"#,
    check: Check::Xml(check_templates),
    min_odoo: ODOO_20,
    max_odoo: None,
};

/// Hooks OWL 3 dropped, and their mechanical replacement.
const REMOVED: &[(&str, Option<&str>)] = &[
    ("useState", Some("proxy")),
    ("reactive", Some("proxy")),
    ("useRef", None),
    ("useExternalListener", Some("useListener")),
    ("onRendered", None),
    ("useChildSubEnv", None),
];

/// An `import { name } from "@odoo/owl"`.
struct OwlImport {
    local: String,
    imported: String,
    span: Span,
    /// The specifiers of the same declaration, to delete one with its comma.
    siblings: Vec<Span>,
}

#[derive(Default)]
struct Facts {
    imports: Vec<OwlImport>,
    /// Name -> the references to it.
    references: HashMap<String, Vec<Span>>,
    /// Name -> the calls of it: their span and argument count.
    calls: HashMap<String, Vec<(Span, usize)>>,
    static_props: Vec<(Span, &'static str)>,
}

impl<'a> Visit<'a> for Facts {
    fn visit_import_declaration(&mut self, import: &ImportDeclaration<'a>) {
        if import.source.value == "@odoo/owl" {
            let specifiers: Vec<&ImportDeclarationSpecifier> = import.specifiers.iter().flatten().collect();
            let siblings: Vec<Span> = specifiers.iter().map(|s| s.span()).collect();
            for specifier in specifiers {
                if let ImportDeclarationSpecifier::ImportSpecifier(s) = specifier {
                    self.imports.push(OwlImport {
                        local: s.local.name.to_string(),
                        imported: s.imported.name().to_string(),
                        span: s.span,
                        siblings: siblings.clone(),
                    });
                }
            }
        }
        walk::walk_import_declaration(self, import);
    }

    fn visit_identifier_reference(&mut self, id: &IdentifierReference<'a>) {
        self.references.entry(id.name.to_string()).or_default().push(id.span);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let oxc_ast::ast::Expression::Identifier(id) = &call.callee {
            let args = call
                .arguments
                .iter()
                .filter(|a| !matches!(a, Argument::SpreadElement(_)))
                .count();
            self.calls
                .entry(id.name.to_string())
                .or_default()
                .push((call.span, args));
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        for element in &class.body.body {
            if let ClassElement::PropertyDefinition(property) = element {
                if !property.r#static {
                    continue;
                }
                for name in ["props", "defaultProps"] {
                    if property.key.is_specific_static_name(name) {
                        self.static_props
                            .push((property.span, if name == "props" { "props" } else { "defaultProps" }));
                    }
                }
            }
        }
        walk::walk_class(self, class);
    }
}

fn offset(span: Span) -> TextSize {
    TextSize::from(span.start)
}

/// Deletes the specifier `span` of an import, with its comma.
fn delete_specifier(span: Span, siblings: &[Span]) -> Edit {
    let i = siblings.iter().position(|s| *s == span).unwrap_or(0);
    match (siblings.get(i + 1), i.checked_sub(1).map(|p| siblings[p])) {
        (Some(next), _) => Edit::delete(span.start as usize, next.start as usize),
        (None, Some(previous)) => Edit::delete(previous.end as usize, span.end as usize),
        (None, None) => Edit::replace(span.start as usize, span.end as usize, ""),
    }
}

/// Renames the imported hook `import` (not aliased) to `new` and every use
/// of it, or drops it when `new` is imported already.
fn rename_hook(import: &OwlImport, new: &str, facts: &Facts) -> Vec<Edit> {
    let mut edits = Vec::new();
    if facts.imports.iter().any(|i| i.local == new) {
        edits.push(delete_specifier(import.span, &import.siblings));
    } else {
        edits.push(Edit::replace(import.span.start as usize, import.span.end as usize, new));
    }
    for span in facts.references.get(&import.local).into_iter().flatten() {
        edits.push(Edit::replace(span.start as usize, span.end as usize, new));
    }
    edits
}

fn check(ctx: &JsContext, reporter: &mut Reporter) {
    let mut facts = Facts::default();
    facts.visit_program(ctx.program);
    for (span, name) in &facts.static_props {
        reporter.report(
            &STATIC_PROPS,
            offset(*span),
            format!("`static {name}` is refused by Odoo 20.0's components; declare the props with `useProps`"),
        );
    }
    for import in &facts.imports {
        let aliased = import.local != import.imported;
        if let Some((name, replacement)) = REMOVED.iter().find(|(n, _)| *n == import.imported) {
            let violation = reporter.report(
                &REMOVED_HOOKS,
                offset(import.span),
                match replacement {
                    Some(new) => format!("OWL 3 (Odoo 20.0) has no `{name}`; use `{new}`"),
                    None => format!("OWL 3 (Odoo 20.0) has no `{name}`"),
                },
            );
            let calls = facts.calls.get(&import.local).map(Vec::as_slice).unwrap_or_default();
            // `reactive(x, callback)` has no one-to-one replacement.
            let mechanical = *name != "reactive" || calls.iter().all(|(_, args)| *args <= 1);
            if let (Some(new), false, true) = (replacement, aliased, mechanical) {
                violation.fix = Some(Fix::safe(format!("Use `{new}`"), rename_hook(import, new, &facts)));
            }
        }
        if import.imported == "useEffect" {
            let calls = facts.calls.get(&import.local).map(Vec::as_slice).unwrap_or_default();
            let with_dependencies: Vec<Span> = calls.iter().filter(|(_, args)| *args >= 2).map(|(s, _)| *s).collect();
            let all = !calls.is_empty() && with_dependencies.len() == calls.len();
            for (i, span) in with_dependencies.iter().enumerate() {
                let violation = reporter.report(
                    &USE_EFFECT,
                    offset(*span),
                    "OWL 3's `useEffect` takes no dependencies; Odoo 20.0 keeps this behaviour as `useLayoutEffect`",
                );
                if i == 0 && all && !aliased {
                    violation.fix = Some(Fix::safe(
                        "Use `useLayoutEffect`",
                        rename_hook(import, "useLayoutEffect", &facts),
                    ));
                }
            }
        }
    }
}

/// A bare name: `t-ref="root"`, not an expression such as `this.root`.
fn is_bare_name(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && value.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn check_templates(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files.iter().filter(|f| f.section == "assets") {
        let Some(root) = file.root() else { continue };
        let is_template = |t: &Node| t.attribute("t-name").is_some() || t.attribute("t-inherit").is_some();
        let templates: Vec<Node> = std::iter::once(root)
            .chain(root.children().filter(Node::is_element))
            .filter(|t| is_template(t))
            .collect();
        for template in templates {
            for node in template.descendants().filter(Node::is_element) {
                for attribute in node.attributes() {
                    let rename = match attribute.name() {
                        "t-portal" => Some("t-custom-portal"),
                        "t-esc" if node.attribute("t-out").is_none() => Some("t-out"),
                        _ => None,
                    };
                    if let Some(new) = rename {
                        let start = attribute.range().start;
                        reporter
                            .report(
                                &TEMPLATE_DIRECTIVES,
                                file,
                                at(file, node),
                                format!("`{}` is `{new}` in Odoo 20.0 (OWL 3)", attribute.name()),
                            )
                            .fix = Some(Fix::safe(
                            format!("Use `{new}`"),
                            vec![Edit::replace(start, start + attribute.name().len(), new)],
                        ));
                    }
                    if attribute.name() == "t-ref" && is_bare_name(attribute.value()) {
                        reporter.report(
                            &TEMPLATE_DIRECTIVES,
                            file,
                            at(file, node),
                            format!(
                                "`t-ref=\"{0}\"`: OWL 3 refs are signals, `t-ref=\"this.{0}\"` with `{0} = signal.ref()`",
                                attribute.value()
                            ),
                        );
                    }
                }
                // `<attribute name="t-portal">` / `name="t-esc"` in an extension.
                if node.tag_name().name() == "attribute" {
                    if let Some(name) = node.attributes().find(|a| a.name() == "name") {
                        let new = match name.value() {
                            "t-portal" => Some("t-custom-portal"),
                            "t-esc" => Some("t-out"),
                            _ => None,
                        };
                        if let Some(new) = new {
                            let range = name.range_value();
                            reporter
                                .report(
                                    &TEMPLATE_DIRECTIVES,
                                    file,
                                    at(file, node),
                                    format!("`{}` is `{new}` in Odoo 20.0 (OWL 3)", name.value()),
                                )
                                .fix = Some(Fix::safe(
                                format!("Use `{new}`"),
                                vec![Edit::replace(range.start, range.end, new)],
                            ));
                        }
                    }
                }
            }
        }
    }
}
