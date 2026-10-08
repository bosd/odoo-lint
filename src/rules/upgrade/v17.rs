//! Changes of Odoo 17.0: the view `attrs`/`states` rewrite, removed data
//! shortcuts, and the ORM methods deprecated in 16.0 and removed in 17.0.

use super::attrs::{attrs_to_expressions, escape_attribute, states_to_invisible};
use super::{data_elements, record_field, view_archs};
use crate::checker::{ManifestContext, PythonContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::rules::python::calls::searched_model;
use crate::rules::python::{classes, field_definitions, methods, source_of};
use crate::rules::xml::{at, delete_element, is, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use crate::semantic::func_name;
use crate::visit::{walk, Node};
use crate::xml::XmlFile;
use regex::Regex;
use roxmltree::Attribute;
use ruff_python_ast::{Expr, Stmt};
use ruff_text_size::{Ranged, TextRange};
use std::collections::HashSet;
use std::sync::LazyLock;

const ODOO_17: Option<OdooVersion> = Some(OdooVersion::new(17, 0));

fn rename(range: TextRange, new: &str) -> Edit {
    Edit::replace(range.start().to_usize(), range.end().to_usize(), new)
}

/// Deletes an attribute and the whitespace before it.
fn delete_attribute(file: &XmlFile, attribute: &Attribute) -> Edit {
    let range = attribute.range();
    let start = file.source[..range.start]
        .trim_end_matches([' ', '\t', '\n', '\r'])
        .len();
    Edit::delete(start, range.end)
}

/// Renames an attribute, keeping its value.
fn rename_attribute(attribute: &Attribute, new: &str) -> Edit {
    let start = attribute.range().start;
    Edit::replace(start, start + attribute.name().len(), new)
}

fn is_list(node: roxmltree::Node) -> bool {
    is(node, "tree") || is(node, "list")
}

/// A field or button of a list view, outside its `<header>`: where Odoo
/// 16 read a plain `invisible` as `column_invisible`.
fn is_list_column(node: roxmltree::Node) -> bool {
    (is(node, "field") || is(node, "button"))
        && node
            .ancestors()
            .skip(1)
            .find(|a| is(*a, "header") || VIEW_TYPES.iter().any(|t| is(*a, t)))
            .is_some_and(is_list)
}

fn constant(value: &str) -> Option<bool> {
    match value.trim() {
        "1" | "True" | "true" => Some(true),
        "0" | "False" | "false" | "" => Some(false),
        _ => None,
    }
}

// --- XML ------------------------------------------------------------------

pub const XML_ATTRS: Rule = Rule {
    code: "U1701",
    name: "upgrade-attrs-states",
    summary: "A view uses `attrs` or `states`, rejected since Odoo 17.0.",
    doc: r#"
## What it does

Reports `attrs` and `states` attributes in views, and `<attribute
name="attrs">`/`<attribute name="states">` in inherited views.

## Why is this bad?

Odoo 17.0 replaced them by Python expressions in `invisible`, `readonly`,
`required` and `column_invisible`: a view with `attrs` or `states` fails to
install ("Since 17.0, the "attrs" and "states" attributes are no longer
used").

## Example

```xml
<field name="partner_id" attrs="{'readonly': [('state', '!=', 'draft')]}"/>
<button name="action_confirm" states="draft,sent"/>
```

Use instead:

```xml
<field name="partner_id" readonly="state != 'draft'"/>
<button name="action_confirm" invisible="state not in ('draft', 'sent')"/>
```

## Fix safety

Safe for domains of `=`, `!=`, `<`, `>`, `<=`, `>=`, `in` and `not in`
terms combined with `&`, `|` and `!`, and for `states`. An existing
constant attribute is merged as Odoo 16 did: a true one wins over the
domain, a false one is replaced, and `states` is appended to the
`invisible` domain, as Odoo 16 did. A plain `invisible` on a list column
becomes `column_invisible`, which is what it meant in 16.0.

Other operators (`ilike`, `child_of`…) need a person.

`<attribute name="attrs">` becomes one `<attribute>` per modifier, an
unsafe fix: in 16.0 the override replaced all of the parent's `attrs`, in
17.0 it replaces only the modifiers it names, so a `readonly` the parent
had stays.
"#,
    check: Check::Xml(check_attrs),
    min_odoo: ODOO_17,
    max_odoo: None,
};

/// The edits that replace `attrs`/`states` on `node`, or `None` when it
/// needs a person.
fn attrs_fix(file: &XmlFile, node: roxmltree::Node) -> Option<Vec<Edit>> {
    let attrs = node.attributes().find(|a| a.name() == "attrs");
    let states = node.attributes().find(|a| a.name() == "states");
    let modifiers = attrs_to_expressions(attrs.as_ref().map(|a| a.value()), states.as_ref().map(|s| s.value()))?;

    let mut edits = Vec::new();
    let mut new = Vec::new();
    for (key, expression) in modifiers {
        let Some(plain) = node.attributes().find(|a| a.name() == key) else {
            new.push(format!("{key}=\"{}\"", escape_attribute(&expression)));
            continue;
        };
        if key == "invisible" && is_list_column(node) {
            // A plain `invisible` hid the column in 16.0.
            if node.attribute("column_invisible").is_none() {
                edits.push(rename_attribute(&plain, "column_invisible"));
                new.push(format!("invisible=\"{}\"", escape_attribute(&expression)));
                continue;
            }
            return None;
        }
        match constant(plain.value())? {
            true => {}
            false => edits.push(Edit::replace(
                plain.range_value().start,
                plain.range_value().end,
                escape_attribute(&expression),
            )),
        }
    }

    let (first, second) = match (&attrs, &states) {
        (Some(a), Some(s)) => (a, Some(s)),
        (Some(a), None) => (a, None),
        (None, Some(s)) => (s, None),
        (None, None) => return None,
    };
    if new.is_empty() {
        edits.push(delete_attribute(file, first));
    } else {
        edits.push(Edit::replace(first.range().start, first.range().end, new.join(" ")));
    }
    if let Some(second) = second {
        edits.push(delete_attribute(file, second));
    }
    Some(edits)
}

/// `<attribute name="attrs">` as one `<attribute>` per modifier. Unsafe:
/// in 16.0 it replaced all of the parent's `attrs`, now only the modifiers
/// it names.
fn attribute_override_fix(file: &XmlFile, node: roxmltree::Node) -> Option<Vec<Edit>> {
    let siblings = node.parent_element()?.children().filter(|c| is(*c, "attribute"));
    if siblings
        .filter(|c| matches!(c.attribute("name"), Some("attrs" | "states")))
        .count()
        > 1
    {
        return None;
    }
    let text = node.text().unwrap_or_default().trim().to_string();
    let modifiers = match node.attribute("name")? {
        "attrs" if text.is_empty() => Vec::new(),
        "attrs" => attrs_to_expressions(Some(&text), None)?,
        _ => vec![("invisible".to_string(), states_to_invisible(&text)?)],
    };
    if modifiers.iter().any(|(key, _)| {
        node.parent_element().is_some_and(|p| {
            p.children()
                .any(|c| is(c, "attribute") && c.attribute("name") == Some(key.as_str()))
        })
    }) {
        return None;
    }
    let range = node.range();
    let line_start = file.source[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let indent = &file.source[line_start..range.start];
    if !indent.trim().is_empty() {
        return None;
    }
    if modifiers.is_empty() {
        return Some(vec![delete_element(file, node)]);
    }
    let elements: Vec<String> = modifiers
        .iter()
        .map(|(key, expression)| {
            format!(
                "<attribute name=\"{key}\">{}</attribute>",
                expression.replace('&', "&amp;").replace('<', "&lt;")
            )
        })
        .collect();
    Some(vec![Edit::replace(
        range.start,
        range.end,
        elements.join(&format!("\n{indent}")),
    )])
}

fn check_attrs(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for (_, arch) in view_archs(file) {
            for node in arch.descendants().filter(|n| n.is_element()) {
                if is(node, "attribute") && matches!(node.attribute("name"), Some("attrs" | "states")) {
                    reporter
                        .report(
                            &XML_ATTRS,
                            file,
                            at(file, node),
                            "`attrs`/`states` are rejected since Odoo 17.0; set `invisible`, `readonly` or `required` instead",
                        )
                        .fix = attribute_override_fix(file, node)
                        .map(|edits| Fix::unsafe_("Override each attribute instead", edits));
                    continue;
                }
                if node.attribute("attrs").is_none() && node.attribute("states").is_none() {
                    continue;
                }
                reporter
                    .report(
                        &XML_ATTRS,
                        file,
                        at(file, node),
                        "`attrs`/`states` are rejected since Odoo 17.0; use Python expressions",
                    )
                    .fix = attrs_fix(file, node).map(|edits| Fix::safe("Use Python expressions", edits));
            }
        }
    }
}

pub const XML_LIST_INVISIBLE: Rule = Rule {
    code: "U1702",
    name: "upgrade-list-column-invisible",
    summary: "A list column hidden with `invisible`, which only hides the cells since Odoo 17.0.",
    doc: r#"
## What it does

Reports `invisible="1"` on fields of list views.

## Why is this bad?

Up to 16.0, `invisible` on a list field hid the column. Since 17.0 it hides
the cells only: the empty column shows. Use `column_invisible`.

## Fix safety

Safe for a constant: the attribute is renamed `column_invisible`.
"#,
    check: Check::Xml(check_list_invisible),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_list_invisible(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for (_, arch) in view_archs(file) {
            for node in arch.descendants().filter(|n| is_list_column(*n)) {
                if node.attribute("column_invisible").is_some() {
                    continue;
                }
                let Some(invisible) = node.attributes().find(|a| a.name() == "invisible") else {
                    continue;
                };
                if constant(invisible.value()) != Some(true) {
                    continue;
                }
                reporter
                    .report(
                        &XML_LIST_INVISIBLE,
                        file,
                        at(file, node),
                        "`invisible` hides only the cells of a list since Odoo 17.0; use `column_invisible`",
                    )
                    .fix = Some(Fix::safe(
                    "Use `column_invisible`",
                    vec![rename_attribute(&invisible, "column_invisible")],
                ));
            }
        }
    }
}

pub const XML_SHORTCUT_TAGS: Rule = Rule {
    code: "U1703",
    name: "upgrade-report-act-window-tags",
    summary: "`<report>` or `<act_window>`, removed in Odoo 17.0.",
    doc: r#"
## What it does

Reports the `<report>` and `<act_window>` shortcut elements in data files.

## Why is this bad?

Odoo 17.0 removed both shortcuts: the data file fails to load. Declare a
`<record model="ir.actions.report">` or `<record
model="ir.actions.act_window">` instead (`string` becomes `name`, `name`
becomes `report_name`, `menu`/`binding_model_id` the binding).
"#,
    check: Check::Xml(check_shortcut_tags),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_shortcut_tags(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in data_elements(file, &["report", "act_window"]) {
            let tag = node.tag_name().name();
            reporter.report(
                &XML_SHORTCUT_TAGS,
                file,
                at(file, node),
                format!("`<{tag}>` was removed in Odoo 17.0; use a `<record>`"),
            );
        }
    }
}

pub const XML_QUICK_ADD: Rule = Rule {
    code: "U1704",
    name: "upgrade-calendar-quick-add",
    summary: "A calendar view uses `quick_add`, renamed `quick_create` in Odoo 17.0.",
    doc: r#"
## What it does

Reports `quick_add` on `<calendar>` views.

## Why is this bad?

Odoo 17.0 renamed it `quick_create`: the view fails to validate.

## Fix safety

Safe: the attribute is renamed.
"#,
    check: Check::Xml(check_quick_add),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_quick_add(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for (_, arch) in view_archs(file) {
            for node in arch.descendants().filter(|n| is(*n, "calendar")) {
                let Some(attribute) = node.attributes().find(|a| a.name() == "quick_add") else {
                    continue;
                };
                let fix = node.attribute("quick_create").is_none().then(|| {
                    Fix::safe(
                        "Rename to `quick_create`",
                        vec![rename_attribute(&attribute, "quick_create")],
                    )
                });
                reporter
                    .report(
                        &XML_QUICK_ADD,
                        file,
                        at(file, node),
                        "`quick_add` is `quick_create` since Odoo 17.0",
                    )
                    .fix = fix;
            }
        }
    }
}

static ACTIVE_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(^|[^'"\w.])(active_ids|active_id|active_model)\b"#).unwrap());

/// Attributes evaluated as expressions in views.
const EXPRESSION_ATTRIBUTES: &[&str] = &[
    "invisible",
    "readonly",
    "required",
    "column_invisible",
    "domain",
    "context",
    "filter_domain",
    "attrs",
];

pub const XML_ACTIVE_ID: Rule = Rule {
    code: "U1705",
    name: "upgrade-view-active-id",
    summary: "A view expression uses `active_id` directly, deprecated in 17.0 and rejected in 18.0.",
    doc: r#"
## What it does

Reports `active_id`, `active_ids` and `active_model` used as names in view
expressions (`invisible`, `domain`, `context`…).

## Why is this bad?

Odoo 17.0 deprecated them in views; 18.0 rejects the view ("field
active_id does not exist"). Read them from the context.

## Fix safety

Safe: `active_id` becomes `context.get('active_id')`.
"#,
    check: Check::Xml(check_active_id),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_active_id(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for (_, arch) in view_archs(file) {
            for node in arch.descendants().filter(|n| n.is_element()) {
                for attribute in node.attributes().filter(|a| EXPRESSION_ATTRIBUTES.contains(&a.name())) {
                    let range = attribute.range_value();
                    let Some(text) = file.source.get(range.clone()) else {
                        continue;
                    };
                    if !ACTIVE_ID.is_match(text) {
                        continue;
                    }
                    let new = ACTIVE_ID.replace_all(text, "${1}context.get('${2}')");
                    reporter
                        .report(
                            &XML_ACTIVE_ID,
                            file,
                            at(file, node),
                            "`active_id` in views is deprecated since Odoo 17.0 (rejected in 18.0); use `context.get('active_id')`",
                        )
                        .fix = Some(Fix::safe("Read it from the context", vec![Edit::replace(range.start, range.end, new.into_owned())]));
                }
            }
        }
    }
}

pub const XML_SERVER_ACTION_LINES: Rule = Rule {
    code: "U1706",
    name: "upgrade-server-action-lines",
    summary: "A server action uses `fields_lines`/`ir.server.object.lines`, removed in Odoo 17.0.",
    doc: r#"
## What it does

Reports `fields_lines` in `ir.actions.server` records and records of
`ir.server.object.lines`.

## Why is this bad?

Odoo 17.0 replaced the lines by fields on the action itself
(`update_path`, `update_field_id`, `value`, `evaluation_type`): the data
file fails to load.
"#,
    check: Check::Xml(check_server_action_lines),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_server_action_lines(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in file.elements() {
            let lines = record_field(node, &["ir.actions.server", "base.automation"], "fields_lines");
            let model = is(node, "record") && node.attribute("model") == Some("ir.server.object.lines");
            if lines || model {
                reporter.report(
                    &XML_SERVER_ACTION_LINES,
                    file,
                    at(file, node),
                    "Server action lines were removed in Odoo 17.0; set `update_path` and `value` on the action",
                );
            }
        }
    }
}

pub const XML_FIELD_PARENT: Rule = Rule {
    code: "U1707",
    name: "upgrade-view-field-parent",
    summary: "A view record sets `field_parent`, removed in Odoo 17.0.",
    doc: r#"
## What it does

Reports `<field name="field_parent">` in `ir.ui.view` records.

## Why is this bad?

Odoo 17.0 removed the field: the data file fails to load ("Invalid field").

## Fix safety

Safe: the field is removed; it had no effect in recent versions.
"#,
    check: Check::Xml(check_field_parent),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_field_parent(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for field in file
            .elements()
            .filter(|f| record_field(*f, &["ir.ui.view"], "field_parent"))
        {
            reporter
                .report(
                    &XML_FIELD_PARENT,
                    file,
                    at(file, field),
                    "`field_parent` was removed in Odoo 17.0",
                )
                .fix = Some(Fix::safe("Remove the field", vec![delete_element(file, field)]));
        }
    }
}

const VIEW_TYPES: &[&str] = &[
    "form",
    "tree",
    "list",
    "kanban",
    "search",
    "graph",
    "pivot",
    "calendar",
    "gantt",
    "activity",
    "cohort",
    "map",
    "hierarchy",
    "grid",
];

/// The QWeb directives Odoo 17.0 allows in kanban views.
fn kanban_directive(name: &str) -> bool {
    matches!(
        name,
        "t-name"
            | "t-esc"
            | "t-out"
            | "t-set"
            | "t-value"
            | "t-if"
            | "t-elif"
            | "t-else"
            | "t-foreach"
            | "t-as"
            | "t-key"
            | "t-call"
            | "t-debug"
            | "t-translation"
    ) || name.starts_with("t-att")
}

pub const XML_VIEW_DIRECTIVES: Rule = Rule {
    code: "U1708",
    name: "upgrade-view-qweb-directives",
    summary: "A view uses a QWeb directive Odoo 17.0 forbids in views.",
    doc: r#"
## What it does

Reports QWeb directives that Odoo 17.0 no longer accepts in view archs:
in kanban views anything but `t-name`, `t-esc`, `t-out`, `t-set`,
`t-value`, `t-if`/`t-elif`/`t-else`, `t-foreach`/`t-as`, `t-key`,
`t-att*`, `t-call`, `t-debug` and `t-translation` (such as `t-raw`,
`t-on-*`, `t-ref`), and in other views anything but `t-translation`.

## Why is this bad?

The view fails to install: "Forbidden owl directive used in arch".

## Fix safety

`t-raw` becomes `t-out`, an unsafe fix: `t-out` escapes a value that is
not marked safe HTML. Other directives need a person.
"#,
    check: Check::Xml(check_view_directives),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_view_directives(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for (_, arch) in view_archs(file) {
            for node in arch.descendants().filter(|n| n.is_element()) {
                let Some(view) = node
                    .ancestors()
                    .take_while(|a| *a != arch)
                    .find(|a| VIEW_TYPES.iter().any(|t| is(*a, t)))
                else {
                    continue;
                };
                let kanban = is(view, "kanban");
                for attribute in node.attributes().filter(|a| a.name().starts_with("t-")) {
                    let name = attribute.name();
                    if name == "t-translation" || (kanban && kanban_directive(name)) {
                        continue;
                    }
                    let fix = (kanban && name == "t-raw")
                        .then(|| Fix::unsafe_("Use `t-out`", vec![rename_attribute(&attribute, "t-out")]));
                    reporter
                        .report(
                            &XML_VIEW_DIRECTIVES,
                            file,
                            at(file, node),
                            format!(
                                "`{name}` is forbidden in `<{}>` views since Odoo 17.0",
                                view.tag_name().name()
                            ),
                        )
                        .fix = fix;
                }
            }
        }
    }
}

// --- Python ---------------------------------------------------------------

pub const NAME_GET: Rule = Rule {
    code: "U1709",
    name: "upgrade-name-get",
    summary: "`name_get`, no longer used for display names since Odoo 17.0.",
    doc: r#"
## What it does

Reports `name_get` overrides in models and calls to `name_get()`.

## Why is this bad?

Since Odoo 17.0 `display_name` is computed by `_compute_display_name`,
which no longer calls `name_get`: an override silently stops changing how
records are shown. 18.0 removed `name_get`.

Override `_compute_display_name` (with `@api.depends`) and read
`record.display_name`.

## Fix safety

Safe for `record.name_get()[0][1]`, which becomes `record.display_name`.
"#,
    check: Check::Python(check_name_get),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_name_get(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        if ctx.semantic.odoo_model_kind(class).is_none() {
            continue;
        }
        for method in methods(class).filter(|m| m.name.as_str() == "name_get") {
            reporter.report(
                &NAME_GET,
                method.name.start(),
                "`name_get` overrides are ignored since Odoo 17.0; override `_compute_display_name`",
            );
        }
    }
    // `x.name_get()[0][1]` first, so its call is not reported twice.
    let mut fixed = HashSet::new();
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Subscript(outer)) = node else {
            return;
        };
        let Expr::Subscript(inner) = &*outer.value else { return };
        let Expr::Call(call) = &*inner.value else { return };
        let Expr::Attribute(attribute) = &*call.func else {
            return;
        };
        let index = |e: &Expr| source_of(ctx.source, e).trim().to_string();
        if attribute.attr.as_str() != "name_get"
            || !call.arguments.is_empty()
            || index(&inner.slice) != "0"
            || index(&outer.slice) != "1"
        {
            return;
        }
        fixed.insert(call.start());
        let receiver = source_of(ctx.source, &*attribute.value);
        reporter
            .report(
                &NAME_GET,
                call.start(),
                "`name_get()` is deprecated since Odoo 17.0; read `display_name`",
            )
            .fix = Some(Fix::safe(
            "Use `display_name`",
            vec![rename(outer.range(), &format!("{receiver}.display_name"))],
        ));
    });
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        if matches!(&*call.func, Expr::Attribute(a) if a.attr.as_str() == "name_get") && !fixed.contains(&call.start())
        {
            reporter.report(
                &NAME_GET,
                call.start(),
                "`name_get()` is deprecated since Odoo 17.0; read `display_name`",
            );
        }
    });
}

pub const NAME_SEARCH_SIGNATURE: Rule = Rule {
    code: "U1710",
    name: "upgrade-name-search-signature",
    summary: "A `_name_search` override with the 16.0 signature (`args`, `name_get_uid`).",
    doc: r#"
## What it does

Reports `_name_search` methods with an `args` or `name_get_uid`
parameter.

## Why is this bad?

Odoo 17.0 changed the signature to `_name_search(name, domain, operator,
limit, order)`, returning a query instead of ids: the override gets a
`TypeError` or returns the wrong type. 18.0 replaces it by
`_search_display_name`.
"#,
    check: Check::Python(check_name_search_signature),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_name_search_signature(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        for method in methods(class).filter(|m| m.name.as_str() == "_name_search") {
            if method.parameters.includes("args") || method.parameters.includes("name_get_uid") {
                reporter.report(
                    &NAME_SEARCH_SIGNATURE,
                    method.name.start(),
                    "`_name_search(name, domain, operator, limit, order)` is the signature since Odoo 17.0",
                );
            }
        }
    }
}

pub const SEARCH_COUNT: Rule = Rule {
    code: "U1711",
    name: "upgrade-search-count-true",
    summary: "`search(..., count=True)`, removed in Odoo 17.0.",
    doc: r#"
## What it does

Reports `count=` in calls to `search` and `_search`.

## Why is this bad?

Odoo 17.0 removed the parameter: the call raises `TypeError`.

## Fix safety

Safe for `search(domain, count=True)` with at most a `limit` besides: it
becomes `search_count(domain)`, keeping the limit.
"#,
    check: Check::Python(check_search_count),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_search_count(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(attribute) = &*call.func else {
            return;
        };
        if !matches!(attribute.attr.as_str(), "search" | "_search") {
            return;
        }
        let keywords = &call.arguments.keywords;
        let Some(count) = keywords
            .iter()
            .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "count"))
        else {
            return;
        };
        let only_limit = keywords.iter().all(|k| {
            k.arg
                .as_ref()
                .is_some_and(|a| matches!(a.as_str(), "count" | "limit" | "domain"))
        });
        let literal_true = matches!(&count.value, Expr::BooleanLiteral(b) if b.value);
        let fix = (attribute.attr.as_str() == "search" && only_limit && literal_true && call.arguments.args.len() <= 1)
            .then(|| {
                let mut edits = vec![rename(attribute.attr.range(), "search_count")];
                let all: Vec<TextRange> = call
                    .arguments
                    .args
                    .iter()
                    .map(Ranged::range)
                    .chain(keywords.iter().map(Ranged::range))
                    .collect();
                let position = all
                    .iter()
                    .position(|r| *r == count.range())
                    .expect("count is an argument");
                if all.len() == 1 {
                    edits.push(rename(count.range(), "[]"));
                } else if position > 0 {
                    edits.push(Edit::delete(all[position - 1].end().to_usize(), count.end().to_usize()));
                } else {
                    edits.push(Edit::delete(count.start().to_usize(), all[1].start().to_usize()));
                }
                Fix::safe("Use `search_count`", edits)
            });
        reporter
            .report(
                &SEARCH_COUNT,
                count.start(),
                "`count=` was removed from `search` in Odoo 17.0; use `search_count`",
            )
            .fix = fix;
    });
}

pub const REMOVED_RECORDSET_METHODS: Rule = Rule {
    code: "U1712",
    name: "upgrade-removed-recordset-methods",
    summary: "A recordset method deprecated in 16.0 and removed in 17.0.",
    doc: r#"
## What it does

Reports calls to `flush()`, `invalidate_cache()`, `recompute()`,
`refresh()`, `get_xml_id()`, `fields_get_keys()` and `fields_view_get()`
on `self` or records. (`fields_view_get` overrides are U1611.)

## Why is this bad?

Odoo 17.0 removed them: the call raises `AttributeError`.

## Fix safety

Safe without arguments: `flush()` and `recompute()` become
`env.flush_all()`, `invalidate_cache()` and `refresh()` become
`env.invalidate_all()`, `get_xml_id()` becomes `get_external_id()` and
`x.fields_get_keys()` becomes `list(x._fields)`. With arguments (field
names, ids), choose between the model and recordset variants by hand.
"#,
    check: Check::Python(check_removed_recordset_methods),
    min_odoo: ODOO_17,
    max_odoo: None,
};

/// Whether `expr` is `self`, a chain of attributes from `self`, or a model
/// from `env[...]`: not some other object with a `flush()`.
fn is_records(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == "self",
        Expr::Attribute(attribute) => attribute.attr.as_str() != "env" && is_records(&attribute.value),
        Expr::Subscript(subscript) => matches!(&*subscript.value, Expr::Attribute(a) if a.attr.as_str() == "env"),
        Expr::Call(call) => matches!(&*call.func, Expr::Attribute(a) if is_records(&a.value)),
        _ => false,
    }
}

fn check_removed_recordset_methods(ctx: &PythonContext, reporter: &mut Reporter) {
    // `fields_view_get` overrides are U1611: they stopped working in 16.0.
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(attribute) = &*call.func else {
            return;
        };
        let name = attribute.attr.as_str();
        let new = match name {
            "flush" | "recompute" => "env.flush_all",
            "invalidate_cache" | "refresh" => "env.invalidate_all",
            "get_xml_id" => "get_external_id",
            "fields_get_keys" | "fields_view_get" => "",
            _ => return,
        };
        if !is_records(&attribute.value) {
            return;
        }
        let fix = call.arguments.is_empty().then_some(()).and_then(|()| match name {
            "fields_view_get" => None,
            "fields_get_keys" => Some(Fix::safe(
                "Use `list(x._fields)`",
                vec![rename(
                    call.range(),
                    &format!("list({}._fields)", source_of(ctx.source, &*attribute.value)),
                )],
            )),
            _ => Some(Fix::safe(
                format!("Use `{new}()`"),
                vec![rename(attribute.attr.range(), new)],
            )),
        });
        reporter
            .report(
                &REMOVED_RECORDSET_METHODS,
                call.start(),
                format!("`{name}()` was removed in Odoo 17.0"),
            )
            .fix = fix;
    });
}

pub const FIELD_STATES: Rule = Rule {
    code: "U1713",
    name: "upgrade-field-states",
    summary: "A field uses `states=`, ignored since Odoo 17.0.",
    doc: r#"
## What it does

Reports the `states` parameter of field definitions.

## Why is this bad?

Odoo 17.0 ignores it (with a warning): fields that were read-only in some
states become editable. Set `readonly="state != 'draft'"` (or the like) on
the field in the views instead.
"#,
    check: Check::Python(check_field_states),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_field_states(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        for (_, call) in field_definitions(class) {
            if let Some(keyword) = call
                .arguments
                .keywords
                .iter()
                .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "states"))
            {
                reporter.report(
                    &FIELD_STATES,
                    keyword.start(),
                    "The field `states` parameter is ignored since Odoo 17.0; set `readonly`/`invisible` in the views",
                );
            }
        }
    }
}

pub const SAVEPOINT_CASE: Rule = Rule {
    code: "U1714",
    name: "upgrade-savepoint-case",
    summary: "`SavepointCase`/`HttpSavepointCase`, removed in Odoo 17.0.",
    doc: r#"
## What it does

Reports `SavepointCase` and `HttpSavepointCase` in tests.

## Why is this bad?

Odoo 17.0 removed the aliases (deprecated since 15.0): the import fails.
`TransactionCase` and `HttpCase` behave the same.

## Fix safety

Safe: every use and import of the names is renamed.
"#,
    check: Check::Python(check_savepoint_case),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn savepoint_rename(name: &str) -> Option<&'static str> {
    match name {
        "SavepointCase" => Some("TransactionCase"),
        "HttpSavepointCase" => Some("HttpCase"),
        _ => None,
    }
}

fn check_savepoint_case(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let (range, name) = match node {
            Node::Expr(Expr::Name(name)) => (name.range(), name.id.as_str()),
            Node::Expr(Expr::Attribute(attribute)) => (attribute.attr.range(), attribute.attr.as_str()),
            Node::Stmt(Stmt::ImportFrom(import)) => {
                for alias in &import.names {
                    if let Some(new) = savepoint_rename(alias.name.as_str()) {
                        reporter
                            .report(
                                &SAVEPOINT_CASE,
                                alias.start(),
                                format!("`{}` was removed in Odoo 17.0; use `{new}`", alias.name),
                            )
                            .fix = Some(Fix::safe(format!("Use `{new}`"), vec![rename(alias.name.range(), new)]));
                    }
                }
                return;
            }
            _ => return,
        };
        let Some(new) = savepoint_rename(name) else { return };
        reporter
            .report(
                &SAVEPOINT_CASE,
                range.start(),
                format!("`{name}` was removed in Odoo 17.0; use `{new}`"),
            )
            .fix = Some(Fix::safe(format!("Use `{new}`"), vec![rename(range, new)]));
    });
}

pub const OLD_EXCEPTIONS: Rule = Rule {
    code: "U1715",
    name: "upgrade-old-exceptions",
    summary: "`odoo.exceptions.Warning` or `except_orm`, removed in Odoo 17.0.",
    doc: r#"
## What it does

Reports `Warning` and `except_orm` imported from `odoo.exceptions`, and
`exceptions.Warning`.

## Why is this bad?

Odoo 17.0 removed both: the import fails. Use `UserError`.

## Fix safety

Safe for `exceptions.Warning`, which becomes `exceptions.UserError`.
Imports are reported only: `Warning` is also a Python builtin.
"#,
    check: Check::Python(check_old_exceptions),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_old_exceptions(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| match node {
        Node::Stmt(Stmt::ImportFrom(import))
            if import.level == 0 && import.module.as_ref().is_some_and(|m| m.as_str() == "odoo.exceptions") =>
        {
            for alias in import
                .names
                .iter()
                .filter(|a| matches!(a.name.as_str(), "Warning" | "except_orm"))
            {
                reporter.report(
                    &OLD_EXCEPTIONS,
                    alias.start(),
                    format!(
                        "`odoo.exceptions.{}` was removed in Odoo 17.0; use `UserError`",
                        alias.name
                    ),
                );
            }
        }
        Node::Expr(Expr::Attribute(attribute))
            if matches!(attribute.attr.as_str(), "Warning" | "except_orm")
                && matches!(&*attribute.value, Expr::Name(n) if n.id.as_str() == "exceptions") =>
        {
            let fix = (attribute.attr.as_str() == "Warning")
                .then(|| Fix::safe("Use `UserError`", vec![rename(attribute.attr.range(), "UserError")]));
            reporter
                .report(
                    &OLD_EXCEPTIONS,
                    attribute.start(),
                    format!(
                        "`exceptions.{}` was removed in Odoo 17.0; use `UserError`",
                        attribute.attr
                    ),
                )
                .fix = fix;
        }
        _ => {}
    });
}

pub const ONCHANGE_DOMAIN: Rule = Rule {
    code: "U1716",
    name: "upgrade-onchange-domain",
    summary: "An onchange returns a `domain`, ignored since Odoo 17.0.",
    doc: r#"
## What it does

Reports `@api.onchange` methods that return a dict with a `domain` key.

## Why is this bad?

Odoo 17.0 ignores the key without a warning (16.0 logged one): the
field's choices are no longer restricted. Put the domain on the field, or
compute it into a field the view's `domain` uses.
"#,
    check: Check::Python(check_onchange_domain),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_onchange_domain(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        for method in methods(class) {
            let onchange = method.decorator_list.iter().any(|d| match &d.expression {
                Expr::Call(call) => func_name(&call.func) == "onchange",
                _ => false,
            });
            if !onchange {
                continue;
            }
            walk(&method.body, |node, _| {
                let Node::Stmt(Stmt::Return(ret)) = node else { return };
                let Some(Expr::Dict(dict)) = ret.value.as_deref() else {
                    return;
                };
                let domain = dict.items.iter().find(|item| {
                    item.key
                        .as_ref()
                        .and_then(Expr::as_string_literal_expr)
                        .is_some_and(|k| k.value.to_str() == "domain")
                });
                if let Some(item) = domain {
                    reporter.report(
                        &ONCHANGE_DOMAIN,
                        item.key.as_ref().expect("found by key").start(),
                        "Onchange `domain` results are ignored since Odoo 17.0; set the domain on the field",
                    );
                }
            });
        }
    }
}

pub const NORECOMPUTE: Rule = Rule {
    code: "U1717",
    name: "upgrade-norecompute",
    summary: "`env.norecompute()`, a no-op since Odoo 17.0.",
    doc: r#"
## What it does

Reports `norecompute()` calls.

## Why is this bad?

Odoo 17.0 made it a deprecated no-op: code relying on it to delay
recomputations no longer does. Remove the `with` and keep its body.
"#,
    check: Check::Python(check_norecompute),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_norecompute(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        if matches!(&*call.func, Expr::Attribute(a) if a.attr.as_str() == "norecompute") {
            reporter.report(
                &NORECOMPUTE,
                call.start(),
                "`norecompute()` does nothing since Odoo 17.0",
            );
        }
    });
}

pub const IR_DEFAULT_GET: Rule = Rule {
    code: "U1718",
    name: "upgrade-ir-default-get",
    summary: "`ir.default.get()`, renamed `_get()` in Odoo 17.0.",
    doc: r#"
## What it does

Reports `self.env["ir.default"].get(...)`.

## Why is this bad?

Odoo 17.0 removed the public `get` (deprecated in 16.0): the call raises
`AttributeError`.

## Fix safety

Safe: `get` becomes `_get`, with the same arguments.
"#,
    check: Check::Python(check_ir_default_get),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_ir_default_get(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(attribute) = &*call.func else {
            return;
        };
        if attribute.attr.as_str() != "get" || searched_model(&call.func) != Some("ir.default") {
            return;
        }
        reporter
            .report(
                &IR_DEFAULT_GET,
                call.start(),
                "`ir.default.get()` is `_get()` since Odoo 17.0",
            )
            .fix = Some(Fix::safe("Use `_get()`", vec![rename(attribute.attr.range(), "_get")]));
    });
}

pub const READ_GROUP_SIGNATURE: Rule = Rule {
    code: "U1719",
    name: "upgrade-private-read-group",
    summary: "`_read_group` called with the 16.0 signature (`lazy`, `orderby`, fields).",
    doc: r#"
## What it does

Reports `_read_group(...)` calls with `lazy=`, `orderby=` or `fields=`.

## Why is this bad?

Odoo 17.0 changed `_read_group` to `_read_group(domain, groupby,
aggregates, having, offset, limit, order)`, returning tuples instead of
dicts: the call fails or its result is misread. Use the new signature, or
the public `read_group` for the old dicts.
"#,
    check: Check::Python(check_read_group_signature),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_read_group_signature(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        if !matches!(&*call.func, Expr::Attribute(a) if a.attr.as_str() == "_read_group") {
            return;
        }
        let old = call.arguments.keywords.iter().any(|k| {
            k.arg
                .as_ref()
                .is_some_and(|a| matches!(a.as_str(), "lazy" | "orderby" | "fields"))
        });
        if old {
            reporter.report(
                &READ_GROUP_SIGNATURE,
                call.start(),
                "`_read_group` takes `(domain, groupby, aggregates, …)` and returns tuples since Odoo 17.0",
            );
        }
    });
}

// --- Manifest -------------------------------------------------------------

pub const OPENERP_MANIFEST: Rule = Rule {
    code: "U1720",
    name: "upgrade-openerp-manifest",
    summary: "The manifest is `__openerp__.py`, deprecated in 17.0 and not found by 19.0.",
    doc: r#"
## What it does

Reports modules whose manifest is `__openerp__.py`.

## Why is this bad?

Odoo 17.0 warns about it, and 19.0 only looks for `__manifest__.py`: the
module is not found at all. Rename the file.
"#,
    check: Check::Manifest(check_openerp_manifest),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_openerp_manifest(ctx: &ManifestContext, reporter: &mut Reporter) {
    if ctx.file_path.ends_with("__openerp__.py") {
        reporter.report(
            &OPENERP_MANIFEST,
            0.into(),
            "`__openerp__.py` is deprecated since Odoo 17.0; rename it `__manifest__.py`",
        );
    }
}

const REMOVED_BUNDLES: &[&str] = &[
    "web.assets_common",
    "web.assets_backend_prod_only",
    "web.frontend_legacy",
];

pub const REMOVED_BUNDLES_RULE: Rule = Rule {
    code: "U1721",
    name: "upgrade-removed-asset-bundles",
    summary: "The manifest adds files to an asset bundle removed in Odoo 17.0.",
    doc: r#"
## What it does

Reports the `assets` bundles `web.assets_common`,
`web.assets_backend_prod_only` and `web.frontend_legacy`.

## Why is this bad?

Odoo 17.0 removed them: the files are silently never loaded. Use
`web.assets_backend` and/or `web.assets_frontend`.
"#,
    check: Check::Manifest(check_removed_bundles),
    min_odoo: ODOO_17,
    max_odoo: None,
};

fn check_removed_bundles(ctx: &ManifestContext, reporter: &mut Reporter) {
    let Some(Expr::Dict(assets)) = ctx.manifest.get("assets") else {
        return;
    };
    for item in &assets.items {
        let Some(key) = item.key.as_ref() else { continue };
        let Some(bundle) = key.as_string_literal_expr().map(|s| s.value.to_str()) else {
            continue;
        };
        if REMOVED_BUNDLES.contains(&bundle) {
            reporter.report(
                &REMOVED_BUNDLES_RULE,
                key.start(),
                format!("The `{bundle}` bundle was removed in Odoo 17.0; its files are not loaded"),
            );
        }
    }
}
