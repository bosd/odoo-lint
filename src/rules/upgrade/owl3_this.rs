//! U2036: OWL 3 (Odoo 20.0) renders a component's template with a context of
//! `{ this: component }` and the template's own variables: `state.x` or
//! `onClick` reads nothing, it has to be `this.state.x`, `this.onClick`.
//! The expressions of the component templates of a module's assets are
//! parsed (oxc) to find the names they read from the component.

use crate::assets::{module_assets, template_locals, Assets};
use crate::fix::{Edit, Fix};
use crate::index::closure;
use crate::odoo_version::OdooVersion;
use crate::rules::xml::{at, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use oxc_ast::ast::{ArrowFunctionExpression, BindingIdentifier, Function, IdentifierReference, ObjectProperty};
use oxc_ast_visit::{walk, Visit};
use oxc_syntax::scope::ScopeFlags;
use roxmltree::Node;
use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

pub const TEMPLATE_THIS: Rule = Rule {
    code: "U2036",
    name: "upgrade-owl3-template-this",
    summary: "A component template reads the component without `this.`, which OWL 3 (Odoo 20.0) requires.",
    doc: r#"
## What it does

Reports expressions in the templates of components (the `static template`
of a component class, and extensions of those templates) that read a name
neither the template defines (`t-set`, `t-as`, `t-slot-scope`, a parameter
of an arrow function) nor OWL provides (`Math`, `Date`, `console`, ...).

## Why is this bad?

OWL 3 renders a component's template with `{ this: component }` and the
template's own variables, no longer with the component itself: `state.open`,
`props.record` or `t-on-click="onClick"` read `undefined`. Odoo 20.0 writes
`this.state.open`, `this.props.record`, `t-on-click="this.onClick"`.

## Fix safety

Safe: `this.` goes before each such name (`{a}` becomes `{a: this.a}`).
Templates that other templates `t-call` get their names from the caller and
are not checked; neither are expressions that do not parse.
"#,
    check: Check::Xml(check),
    min_odoo: Some(OdooVersion::new(20, 0)),
    max_odoo: None,
};

/// Names OWL 3's template compiler leaves alone (`RESERVED_WORDS`), and the
/// values Odoo 20.0's applications provide.
static GLOBALS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "true",
        "false",
        "NaN",
        "null",
        "undefined",
        "console",
        "window",
        "Math",
        "RegExp",
        "Array",
        "Object",
        "Date",
        "__globals__",
        "Portal",
        "this",
    ]
    .into_iter()
    .collect()
});

/// QWeb's word operators, with JavaScript ones of the same length so that
/// offsets stay put.
const WORD_OPERATORS: &[(&str, &str)] = &[
    ("and", "&& "),
    ("or", "||"),
    ("gte", ">= "),
    ("lte", "<= "),
    ("gt", "> "),
    ("lt", "< "),
];

/// `expr` with QWeb's word operators replaced, outside strings.
fn javascript(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::with_capacity(expr.len());
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' && i + 1 < bytes.len() {
                out.push_str(&expr[i..i + 2]);
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            out.push(c as char);
            i += 1;
            continue;
        }
        if matches!(c, b'\'' | b'"' | b'`') {
            quote = Some(c);
            out.push(c as char);
            i += 1;
            continue;
        }
        let word_start =
            i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || matches!(bytes[i - 1], b'_' | b'$' | b'.'));
        if word_start && c.is_ascii_lowercase() {
            let end = expr[i..]
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'))
                .map_or(expr.len(), |n| i + n);
            if let Some((_, js)) = WORD_OPERATORS.iter().find(|(word, _)| *word == &expr[i..end]) {
                out.push_str(js);
                i = end;
                continue;
            }
        }
        // Copy one character (UTF-8 safe).
        let ch = expr[i..].chars().next().expect("in bounds");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// A name an expression reads: its offset in the expression, and whether it
/// is a shorthand property (`{a}`).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Free {
    pub start: usize,
    pub name: String,
    pub shorthand: bool,
}

#[derive(Default)]
struct Names {
    bound: Vec<HashSet<String>>,
    free: Vec<Free>,
    shorthand: HashSet<u32>,
}

impl Names {
    fn is_bound(&self, name: &str) -> bool {
        self.bound.iter().any(|scope| scope.contains(name))
    }
}

#[derive(Default)]
struct Bindings(HashSet<String>);

impl<'a> Visit<'a> for Bindings {
    fn visit_binding_identifier(&mut self, id: &BindingIdentifier<'a>) {
        self.0.insert(id.name.to_string());
    }
}

impl<'a> Visit<'a> for Names {
    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        let mut params = Bindings::default();
        params.visit_formal_parameters(&arrow.params);
        self.bound.push(params.0);
        walk::walk_arrow_function_expression(self, arrow);
        self.bound.pop();
    }

    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        let mut params = Bindings::default();
        params.visit_formal_parameters(&function.params);
        self.bound.push(params.0);
        walk::walk_function(self, function, flags);
        self.bound.pop();
    }

    fn visit_object_property(&mut self, property: &ObjectProperty<'a>) {
        if property.shorthand {
            self.shorthand.insert(property.value_span_start());
        }
        walk::walk_object_property(self, property);
    }

    fn visit_identifier_reference(&mut self, id: &IdentifierReference<'a>) {
        let name = id.name.as_str();
        if !self.is_bound(name) {
            self.free.push(Free {
                start: id.span.start as usize,
                name: name.to_string(),
                shorthand: self.shorthand.contains(&id.span.start),
            });
        }
    }
}

trait ValueStart {
    fn value_span_start(&self) -> u32;
}

impl ValueStart for ObjectProperty<'_> {
    fn value_span_start(&self) -> u32 {
        oxc_span::GetSpan::span(&self.value).start
    }
}

/// The names `expr` reads that are neither in `locals` nor globals; `None`
/// when it does not parse.
pub(crate) fn free_names(expr: &str, locals: &HashSet<String>) -> Option<Vec<Free>> {
    if expr.trim().is_empty() {
        return Some(Vec::new());
    }
    let source = javascript(expr);
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::mjs())
        .parse_expression()
        .ok()?;
    let mut names = Names::default();
    names.visit_expression(&parsed);
    Some(
        names
            .free
            .into_iter()
            .filter(|f| !locals.contains(&f.name) && !GLOBALS.contains(f.name.as_str()))
            .collect(),
    )
}

/// The parts of a `t-attf-` value that are expressions: `{{ ... }}` and
/// `#{ ... }`, with their offsets.
fn format_parts(value: &str) -> Vec<(usize, &str)> {
    let mut parts = Vec::new();
    let mut rest = 0;
    while let Some(found) = value[rest..].find(['{', '#']) {
        let i = rest + found;
        let (open, close) = if value[i..].starts_with("{{") {
            (2, "}}")
        } else if value[i..].starts_with("#{") {
            (2, "}")
        } else {
            rest = i + 1;
            continue;
        };
        let Some(end) = value[i + open..].find(close) else {
            break;
        };
        parts.push((i + open, &value[i + open..i + open + end]));
        rest = i + open + end + close.len();
    }
    parts
}

/// The value of an attribute as the parser decodes it, and for each of its
/// bytes the offset of the byte in the source it comes from.
fn decode(raw: &str) -> Option<(String, Vec<usize>)> {
    let mut out = String::new();
    let mut map = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        if raw.as_bytes()[i] == b'&' {
            let end = raw[i..].find(';')? + i;
            let entity = &raw[i + 1..end];
            let ch = match entity {
                "lt" => '<',
                "gt" => '>',
                "amp" => '&',
                "quot" => '"',
                "apos" => '\'',
                _ if entity.starts_with("#x") => char::from_u32(u32::from_str_radix(&entity[2..], 16).ok()?)?,
                _ if entity.starts_with('#') => char::from_u32(entity[1..].parse().ok()?)?,
                _ => return None,
            };
            for _ in 0..ch.len_utf8() {
                map.push(i);
            }
            out.push(ch);
            i = end + 1;
            continue;
        }
        let ch = raw[i..].chars().next()?;
        for k in 0..ch.len_utf8() {
            map.push(i + k);
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    map.push(raw.len());
    Some((out, map))
}

/// Whether the attribute `name` of `node` holds an expression; `Some(true)`
/// for a format string (`t-attf-`).
fn expression_kind(node: Node, name: &str) -> Option<bool> {
    const EXPRESSIONS: &[&str] = &[
        "t-if",
        "t-elif",
        "t-out",
        "t-esc",
        "t-raw",
        "t-foreach",
        "t-key",
        "t-value",
        "t-props",
        "t-component",
        "t-tag",
        "t-model",
        "t-att",
    ];
    // `t-ref="root"` names a ref; U2034 turns it into a signal.
    if name.starts_with("t-attf-") {
        return Some(true);
    }
    if EXPRESSIONS.contains(&name) || name.starts_with("t-att-") || name.starts_with("t-on-") {
        return Some(false);
    }
    // The props of a component, and of a `t-slot`; `.translate` ones are text.
    let tag = node.tag_name().name();
    let component = tag.chars().next().is_some_and(char::is_uppercase);
    let slot = tag == "t" && node.attribute("t-slot").is_some();
    // `id` is meant as text (`id="o_panel"`), as in OWL 2, where it read
    // nothing either: not an upgrade matter.
    ((component || slot) && !name.starts_with("t-") && !name.ends_with(".translate") && !matches!(name, "slot" | "id"))
        .then_some(false)
}

fn check(ctx: &XmlContext, reporter: &mut XmlReporter) {
    // The templates components render, in this module and the ones it sees.
    let mut assets: Vec<Arc<Assets>> = vec![module_assets(&ctx.module.path)];
    if let Some(closure) = closure(ctx.module, &ctx.settings.addons_path) {
        assets.extend(closure.modules().iter().map(|m| module_assets(&m.path)));
    }
    // A template others `t-call` gets variables from its caller.
    let is_component = |name: &str| {
        assets.iter().any(|a| a.component_templates.contains(name))
            && !assets.iter().any(|a| a.called_templates.contains(name))
    };
    for file in ctx.files.iter().filter(|f| f.section == "assets") {
        let Some(root) = file.root() else { continue };
        let templates = std::iter::once(root)
            .chain(root.children().filter(Node::is_element))
            .filter(|t| {
                let extension = t.attribute("t-inherit-mode") == Some("extension");
                match (t.attribute("t-name"), t.attribute("t-inherit")) {
                    (_, Some(parent)) if extension => is_component(parent),
                    (Some(name), _) => is_component(name),
                    _ => false,
                }
            });
        for template in templates {
            // The template's own variables, and those of the templates it
            // extends or inherits from: an extension's content lands there.
            let mut locals = template_locals(template);
            let mut next = template.attribute("t-inherit").map(str::to_string);
            let mut seen = HashSet::new();
            while let Some(name) = next.take().filter(|n| seen.insert(n.clone())) {
                for a in &assets {
                    if let Some(found) = a.template_locals.get(&name) {
                        locals.extend(found.iter().cloned());
                    }
                    if let Some(parent) = a.parents.get(&name) {
                        next = Some(parent.clone());
                    }
                }
            }
            if let Some(name) = template.attribute("t-name") {
                for a in &assets {
                    if let Some(found) = a.template_locals.get(name) {
                        locals.extend(found.iter().cloned());
                    }
                }
            }
            for node in template.descendants().filter(Node::is_element) {
                // An inheritance spec (`position=`) or an `xpath` selects.
                if node.attribute("position").is_some() || node.tag_name().name() == "xpath" {
                    continue;
                }
                for attribute in node.attributes() {
                    let Some(format) = expression_kind(node, attribute.name()) else {
                        continue;
                    };
                    let range = attribute.range_value();
                    let Some(raw) = file.source.get(range.clone()) else {
                        continue;
                    };
                    let Some((value, map)) = decode(raw) else { continue };
                    let parts: Vec<(usize, &str)> = if format {
                        format_parts(&value)
                    } else {
                        vec![(0, value.as_str())]
                    };
                    let mut edits = Vec::new();
                    let mut names = Vec::new();
                    let mut parsed = true;
                    for (offset, expr) in parts {
                        let Some(free) = free_names(expr, &locals) else {
                            parsed = false;
                            break;
                        };
                        for f in free {
                            let at_source = range.start + map[offset + f.start];
                            edits.push(if f.shorthand {
                                Edit::insert(at_source + f.name.len(), format!(": this.{}", f.name))
                            } else {
                                Edit::insert(at_source, "this.")
                            });
                            if !names.contains(&f.name) {
                                names.push(f.name);
                            }
                        }
                    }
                    if !parsed || edits.is_empty() {
                        continue;
                    }
                    let shown: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
                    reporter
                        .report(
                            &TEMPLATE_THIS,
                            file,
                            at(file, node),
                            format!(
                                "`{}` reads {} from the component: OWL 3 needs `this.`",
                                attribute.name(),
                                shown.join(", ")
                            ),
                        )
                        .fix = Some(Fix::safe("Add `this.`", edits));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(expr: &str, locals: &[&str]) -> Vec<String> {
        let locals: HashSet<String> = locals.iter().map(|s| s.to_string()).collect();
        free_names(expr, &locals).unwrap().into_iter().map(|f| f.name).collect()
    }

    #[test]
    fn free_names_of_expressions() {
        assert_eq!(names("state.open and props.record", &[]), vec!["state", "props"]);
        assert_eq!(names("this.state.open", &[]), Vec::<String>::new());
        assert_eq!(names("line.qty gt 1", &["line"]), Vec::<String>::new());
        assert_eq!(names("(ev) => this.select(ev, item)", &[]), vec!["item"]);
        assert_eq!(names("Math.max(a, 'and b')", &[]), vec!["a"]);
        assert_eq!(names("{ value, other: x.y }", &[]), vec!["value", "x"]);
        assert!(free_names("a b c", &HashSet::new()).is_none());
    }

    #[test]
    fn format_strings_and_entities() {
        assert_eq!(
            format_parts("o_{{ state.mode }} #{cls} x"),
            vec![(4, " state.mode "), (21, "cls")]
        );
        let (value, map) = decode("a &amp;&amp; b").unwrap();
        assert_eq!(value, "a && b");
        assert_eq!(map[5], 13);
    }
}
