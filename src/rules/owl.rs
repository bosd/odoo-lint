//! OWL001-OWL003: OWL templates, the `<templates>` files of a module's
//! assets. OWL 2 (Odoo 16.0 and later) compiles them in the browser; these
//! are the mistakes its compiler or runtime only reports there.

use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::rules::xml::{at, is, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use crate::xml::XmlFile;
use roxmltree::Node;

const ODOO_16: Option<OdooVersion> = Some(OdooVersion::new(16, 0));

pub const FOREACH_WITHOUT_KEY: Rule = Rule {
    code: "OWL001",
    name: "owl-foreach-without-key",
    summary: "A `t-foreach` in an OWL template has no `t-key`.",
    doc: r#"
## What it does

Reports elements with `t-foreach` and no `t-key` in the OWL templates of the
module's assets.

## Why is this bad?

OWL 2 refuses to compile the template: "Directive t-foreach should always
be used with a t-key!". The component fails where it renders, in the
browser only. Server-side QWeb (views, reports, website templates) does not
need `t-key` and is not checked.

## Fix safety

Unsafe: adds `t-key="<t-as>_index"`, which compiles but keys by position.
A unique value of the items (`line.id`) lets OWL keep their state when the
list changes.
"#,
    check: Check::Xml(check),
    min_odoo: ODOO_16,
    max_odoo: None,
};

pub const T_RAW: Rule = Rule {
    code: "OWL002",
    name: "owl-t-raw",
    summary: "An OWL template uses `t-raw`, deprecated in OWL 2 for `t-out`.",
    doc: r#"
## What it does

Reports `t-raw` in the OWL templates of the module's assets.

## Why is this bad?

OWL 2 renders it as `t-out` and logs a warning on every compilation: a
value not wrapped in `markup()` is escaped, so HTML built as a plain string
shows as text. OWL 3 drops it.

## Fix safety

Unsafe: `t-raw` becomes `t-out`; wrap the value in `markup()` in the
component where it holds HTML.
"#,
    check: Check::Xml(check),
    min_odoo: ODOO_16,
    max_odoo: None,
};

pub const HANDLER_NOT_A_FUNCTION: Rule = Rule {
    code: "OWL003",
    name: "owl-handler-not-a-function",
    summary: "A `t-on-*` handler in an OWL template is a statement or a call, not a function.",
    doc: r#"
## What it does

Reports `t-on-*` attributes whose value assigns (`state.open = true`,
`state.count++`) or calls (`onClick(line)`) instead of naming a function
(`onClick`, `props.onSelect`, `() => this.onClick(line)`).

## Why is this bad?

OWL 2 evaluates the value when the template renders and calls the result on
the event. An assignment or a call runs at every render, and the click
then fails with "Invalid handler (expected a function, ...)". OWL 1 ran the
expression on the event; templates ported from it keep that form.

## Example

```xml
<button t-on-click="selectLine(line)">Select</button>
```

Use instead:

```xml
<button t-on-click="() => this.selectLine(line)">Select</button>
```
"#,
    check: Check::Xml(check),
    min_odoo: ODOO_16,
    max_odoo: None,
};

/// The OWL templates of the assets: the `<t t-name>` children of a
/// `<templates>` root. Before 17.0, when the legacy QWeb engine still
/// rendered templates, only those marked `owl="1"`.
fn owl_templates<'a, 'input>(file: &'a XmlFile<'input>, legacy_engine: bool) -> Vec<Node<'a, 'input>> {
    let Some(root) = file.root().filter(|r| is(*r, "templates")) else {
        return Vec::new();
    };
    root.children()
        .filter(|t| t.is_element() && (t.attribute("t-name").is_some() || t.attribute("t-inherit").is_some()))
        .filter(|t| !legacy_engine || t.attribute("owl") == Some("1"))
        .collect()
}

/// Whether a `t-on-*` value assigns or calls rather than evaluating to a
/// function: an arrow function, a name or a property path pass, as does
/// `fn.bind(...)`.
fn not_a_function(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.contains("=>") {
        return false;
    }
    let assigns = value.contains("++")
        || value.contains("--")
        || value.char_indices().any(|(i, c)| {
            c == '=' && !value[..i].ends_with(['=', '!', '<', '>']) && !value[i + 1..].starts_with(['=', '>'])
        });
    let calls = value.ends_with(')') && !value.contains(".bind(");
    assigns || calls
}

fn check(ctx: &XmlContext, reporter: &mut XmlReporter) {
    let version = ctx.settings.manifest_version_of(Some(ctx.module));
    let legacy_engine = version.is_some_and(|v| v < OdooVersion::new(17, 0));
    for file in ctx.files.iter().filter(|f| f.section == "assets") {
        for template in owl_templates(file, legacy_engine) {
            for node in template.descendants().filter(Node::is_element) {
                if node.attribute("t-foreach").is_some() && node.attribute("t-key").is_none() {
                    let violation = reporter.report(
                        &FOREACH_WITHOUT_KEY,
                        file,
                        at(file, node),
                        "`t-foreach` without `t-key`: OWL does not compile the template",
                    );
                    if let Some(name) = node.attribute("t-as").filter(|a| !a.is_empty()) {
                        let foreach = node.attributes().find(|a| a.name() == "t-foreach").expect("checked");
                        let quote = if file.source[..foreach.range_value().start].ends_with('\'') {
                            '\''
                        } else {
                            '"'
                        };
                        let end = foreach.range_value().end + 1;
                        violation.fix = Some(Fix::unsafe_(
                            format!("Add `t-key=\"{name}_index\"`"),
                            vec![Edit::insert(end, format!(" t-key={quote}{name}_index{quote}"))],
                        ));
                    }
                }
                if let Some(raw) = node.attributes().find(|a| a.name() == "t-raw") {
                    let violation = reporter.report(
                        &T_RAW,
                        file,
                        at(file, node),
                        "`t-raw` is deprecated in OWL 2; use `t-out` with `markup()`",
                    );
                    if node.attribute("t-out").is_none() {
                        let start = raw.range().start;
                        violation.fix = Some(Fix::unsafe_(
                            "Use `t-out`",
                            vec![Edit::replace(start, start + 5, "t-out")],
                        ));
                    }
                }
                for handler in node.attributes().filter(|a| a.name().starts_with("t-on-")) {
                    if not_a_function(handler.value()) {
                        reporter.report(
                            &HANDLER_NOT_A_FUNCTION,
                            file,
                            at(file, node),
                            format!(
                                "`{}=\"{}\"` runs at every render; OWL needs a function, such as `() => ...`",
                                handler.name(),
                                handler.value()
                            ),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handlers() {
        for ok in [
            "onClick",
            "props.onSelect",
            "() => this.select(line)",
            "(ev) => state.x = ev.target.value",
            "onClick.bind(this, 1)",
        ] {
            assert!(!not_a_function(ok), "{ok}");
        }
        for bad in [
            "state.open = !state.open",
            "state.count++",
            "selectLine(line)",
            "props.onClose()",
        ] {
            assert!(not_a_function(bad), "{bad}");
        }
        assert!(!not_a_function("state.a == 1 and b"));
    }
}
