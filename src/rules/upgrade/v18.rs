//! Changes of Odoo 18.0 (`odoo/upgrade_code/17.5-*` and the 17.0 -> 18.0
//! differences of views, schemas and models).

use super::{arch_elements, record_field, rename_tag, replace_in, text_range};
use crate::checker::{PythonContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::rules::python::{classes, field_definitions, methods};
use crate::rules::xml::{at, delete_element, is, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use crate::visit::{walk, Node};
use regex::Regex;
use ruff_python_ast::{Expr, StringLiteralValue};
use ruff_text_size::Ranged;
use std::sync::LazyLock;

const ODOO_18: Option<OdooVersion> = Some(OdooVersion::new(18, 0));

/// `tree` as a whole word, e.g. in `view_mode` lists and xpath steps.
static TREE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\btree\b").unwrap());
/// `tree` as an element name in an xpath expression.
static TREE_STEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|[/\[(|:])tree\b").unwrap());
static TREE_VIEW_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\btree_view_ref\b").unwrap());

pub const XML_TREE_VIEW: Rule = Rule {
    code: "U1801",
    name: "upgrade-tree-view",
    summary: "A view uses `<tree>`, renamed `<list>` in Odoo 18.0.",
    doc: r#"
## What it does

Reports `<tree>` elements in view archs: list views and the lists of
x2many fields.

## Why is this bad?

Odoo 18.0 renamed the list view type: installing a view with `<tree>` fails
with "Invalid view type: 'tree'".

## Example

```xml
<tree string="Orders"><field name="name"/></tree>
```

Use instead:

```xml
<list string="Orders"><field name="name"/></list>
```

## Fix safety

Safe: the start and end tag are renamed.
"#,
    check: Check::Xml(check_tree_view),
    min_odoo: ODOO_18,
    max_odoo: None,
};

fn check_tree_view(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in arch_elements(file).into_iter().filter(|n| is(*n, "tree")) {
            let fix = rename_tag(file, node, "list").map(|edits| Fix::safe("Rename to `<list>`", edits));
            reporter
                .report(
                    &XML_TREE_VIEW,
                    file,
                    at(file, node),
                    "`<tree>` is `<list>` since Odoo 18.0",
                )
                .fix = fix;
        }
    }
}

pub const XML_VIEW_MODE_TREE: Rule = Rule {
    code: "U1802",
    name: "upgrade-view-mode-tree",
    summary: "An action's `view_mode` says `tree`, renamed `list` in Odoo 18.0.",
    doc: r#"
## What it does

Reports `tree` in the `view_mode` of window actions and their views, and in
`binding_view_types`.

## Why is this bad?

Odoo 18.0 renamed the view type. An `ir.actions.act_window.view` with `tree`
fails to install; a window action with `tree` in its `view_mode` fails when
it is opened.

## Fix safety

Safe: `tree` becomes `list`.
"#,
    check: Check::Xml(check_view_mode_tree),
    min_odoo: ODOO_18,
    max_odoo: None,
};

const ACTION_MODELS: &[&str] = &["ir.actions.act_window", "ir.actions.act_window.view"];

fn check_view_mode_tree(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for field in file.elements() {
            let relevant = record_field(field, ACTION_MODELS, "view_mode")
                || (is(field, "field") && field.attribute("name") == Some("binding_view_types"));
            if !relevant || !field.text().is_some_and(|t| TREE.is_match(t)) {
                continue;
            }
            let fix = text_range(field)
                .and_then(|range| replace_in(file, range, &TREE, "list"))
                .map(|edit| Fix::safe("Use `list`", vec![edit]));
            reporter
                .report(
                    &XML_VIEW_MODE_TREE,
                    file,
                    at(file, field),
                    "View type `tree` is `list` since Odoo 18.0",
                )
                .fix = fix;
        }
    }
}

pub const XML_TREE_REFERENCE: Rule = Rule {
    code: "U1803",
    name: "upgrade-tree-reference",
    summary: "An xpath, `mode` or `tree_view_ref` refers to `tree`, renamed `list` in Odoo 18.0.",
    doc: r#"
## What it does

Reports references to the old list view name: `tree` as an element in
`<xpath expr>`, `mode="tree"` on x2many fields, and `tree_view_ref` in
contexts.

## Why is this bad?

Since Odoo 18.0 an xpath to `//tree` finds nothing and the view fails to
install, and `tree_view_ref` is silently ignored: the default list view is
used instead of the one you named.

## Fix safety

Safe: `tree` becomes `list` and `tree_view_ref` becomes `list_view_ref`.
"#,
    check: Check::Xml(check_tree_reference),
    min_odoo: ODOO_18,
    max_odoo: None,
};

fn check_tree_reference(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in file.elements() {
            let mut edits = Vec::new();
            for attribute in node.attributes() {
                let range = attribute.range_value();
                let edit = match attribute.name() {
                    "expr" if is(node, "xpath") => replace_in(file, range, &TREE_STEP, "${1}list"),
                    "mode" if is(node, "field") => replace_in(file, range, &TREE, "list"),
                    _ => replace_in(file, range, &TREE_VIEW_REF, "list_view_ref"),
                };
                edits.extend(edit);
            }
            if is(node, "field") && node.attribute("name") == Some("context") {
                edits.extend(text_range(node).and_then(|r| replace_in(file, r, &TREE_VIEW_REF, "list_view_ref")));
            }
            if edits.is_empty() {
                continue;
            }
            reporter
                .report(
                    &XML_TREE_REFERENCE,
                    file,
                    at(file, node),
                    "Reference to the `tree` view, which is `list` since Odoo 18.0",
                )
                .fix = Some(Fix::safe("Refer to `list`", edits));
        }
    }
}

pub const XML_CRON_NUMBERCALL: Rule = Rule {
    code: "U1804",
    name: "upgrade-cron-numbercall",
    summary: "A scheduled action sets `numbercall` or `doall`, removed in Odoo 18.0.",
    doc: r#"
## What it does

Reports `<field name="numbercall">` and `<field name="doall">` in `ir.cron`
records.

## Why is this bad?

Odoo 18.0 removed both fields: installing the record fails with "Invalid
field". Scheduled actions now always run until they are deactivated.

## Fix safety

Safe for `doall` and for `numbercall` -1 (run forever): the field is
removed. Unsafe for another `numbercall`: an action meant to run a limited
number of times now runs until deactivated.
"#,
    check: Check::Xml(check_cron_numbercall),
    min_odoo: ODOO_18,
    max_odoo: None,
};

fn check_cron_numbercall(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for field in file.elements() {
            let numbercall = record_field(field, &["ir.cron"], "numbercall");
            if !numbercall && !record_field(field, &["ir.cron"], "doall") {
                continue;
            }
            let value = field.attribute("eval").or(field.text()).unwrap_or_default().trim();
            let edits = vec![delete_element(file, field)];
            let fix = if !numbercall || value == "-1" {
                Fix::safe("Remove the field", edits)
            } else {
                Fix::unsafe_("Remove the field (the action then runs until deactivated)", edits)
            };
            let name = field.attribute("name").unwrap_or_default();
            reporter
                .report(
                    &XML_CRON_NUMBERCALL,
                    file,
                    at(file, field),
                    format!("`ir.cron` field `{name}` was removed in Odoo 18.0"),
                )
                .fix = Some(fix);
        }
    }
}

pub const XML_DEFAULT_PERIOD: Rule = Rule {
    code: "U1805",
    name: "upgrade-default-period",
    summary: "A date filter's `default_period` uses a name Odoo 18.0 replaced.",
    doc: r#"
## What it does

Reports `default_period` values like `this_month`, `last_year` or
`antepenultimate_month` on search filters.

## Why is this bad?

Odoo 18.0 renamed the periods: `this_month` is `month`, `last_month` is
`month-1`, `antepenultimate_month` is `month-2`, and the same for years.
Installing the old names fails with "Invalid default period".

## Fix safety

Safe: the values are renamed.
"#,
    check: Check::Xml(check_default_period),
    min_odoo: ODOO_18,
    max_odoo: None,
};

static OLD_PERIOD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(this|last|antepenultimate)_(month|year)\b").unwrap());

fn check_default_period(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in file.elements().filter(|n| is(*n, "filter")) {
            let Some(attribute) = node.attributes().find(|a| a.name() == "default_period") else {
                continue;
            };
            if !OLD_PERIOD.is_match(attribute.value()) {
                continue;
            }
            let range = attribute.range_value();
            let text = &file.source[range.clone()];
            let new = OLD_PERIOD
                .replace_all(text, |caps: &regex::Captures| {
                    let unit = &caps[2];
                    match &caps[1] {
                        "this" => unit.to_string(),
                        "last" => format!("{unit}-1"),
                        _ => format!("{unit}-2"),
                    }
                })
                .into_owned();
            reporter
                .report(
                    &XML_DEFAULT_PERIOD,
                    file,
                    at(file, node),
                    format!("`default_period=\"{text}\"` uses names Odoo 18.0 replaced; use `{new}`"),
                )
                .fix = Some(Fix::safe(
                format!("Use `{new}`"),
                vec![Edit::replace(range.start, range.end, new.clone())],
            ));
        }
    }
}

pub const XML_KANBAN_BOX: Rule = Rule {
    code: "U1806",
    name: "upgrade-kanban-box",
    summary: "A kanban view uses the `kanban-box` template, replaced by `card` in Odoo 18.0.",
    doc: r#"
## What it does

Reports `<t t-name="kanban-box">` and `kanban-menu` templates in kanban
views.

## Why is this bad?

Odoo 18.0 replaced them by `card` and `menu`, and warns. Odoo 19.0 removed
the old names: the view fails with "Missing 'card' template".

The card is not a renamed box: its root element is the card itself (no
`oe_kanban_global_click` wrapper), and helpers such as `kanban_image()` and
`user_context` are gone. Rewrite it by hand, following one of Odoo's own
18.0 kanban views.
"#,
    check: Check::Xml(check_kanban_box),
    min_odoo: ODOO_18,
    max_odoo: None,
};

fn check_kanban_box(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in arch_elements(file) {
            let Some(name) = node
                .attribute("t-name")
                .filter(|n| matches!(*n, "kanban-box" | "kanban-menu"))
            else {
                continue;
            };
            let new = if name == "kanban-box" { "card" } else { "menu" };
            reporter.report(
                &XML_KANBAN_BOX,
                file,
                at(file, node),
                format!("Kanban template `{name}` is `{new}` since Odoo 18.0 (removed in 19.0)"),
            );
        }
    }
}

pub const GROUP_OPERATOR: Rule = Rule {
    code: "U1807",
    name: "upgrade-group-operator",
    summary: "A field uses `group_operator=`, renamed `aggregator=` in Odoo 18.0.",
    doc: r#"
## What it does

Reports the `group_operator` field parameter.

## Why is this bad?

Odoo 18.0 renamed it `aggregator`. It is still mapped, with a deprecation
warning, until Odoo 20.0 removed it.

## Fix safety

Safe: the parameter is renamed. There is no fix when the field already has
`aggregator`.
"#,
    check: Check::Python(check_group_operator),
    min_odoo: ODOO_18,
    max_odoo: None,
};

fn check_group_operator(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        for (_, call) in field_definitions(class) {
            let keywords = &call.arguments.keywords;
            let Some(keyword) = keywords
                .iter()
                .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "group_operator"))
            else {
                continue;
            };
            let has_aggregator = keywords
                .iter()
                .any(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "aggregator"));
            let name = keyword.arg.as_ref().expect("found by name");
            reporter
                .report(
                    &GROUP_OPERATOR,
                    keyword.start(),
                    "`group_operator` is `aggregator` since Odoo 18.0",
                )
                .fix = (!has_aggregator).then(|| {
                Fix::safe(
                    "Rename to `aggregator`",
                    vec![Edit::replace(
                        name.start().to_usize(),
                        name.end().to_usize(),
                        "aggregator",
                    )],
                )
            });
        }
    }
}

pub const USER_HAS_GROUPS: Rule = Rule {
    code: "U1808",
    name: "upgrade-user-has-groups",
    summary: "`user_has_groups()`, removed in Odoo 18.0, is called.",
    doc: r#"
## What it does

Reports calls to `self.user_has_groups(...)`.

## Why is this bad?

Odoo 18.0 removed the method from models, without a deprecation period in
17.0: the call raises `AttributeError`. `self.env.user.has_groups(...)`
takes the same group expressions.

## Fix safety

Safe: `x.user_has_groups(...)` becomes `x.env.user.has_groups(...)`.
"#,
    check: Check::Python(check_user_has_groups),
    min_odoo: ODOO_18,
    max_odoo: None,
};

fn check_user_has_groups(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(attribute) = &*call.func else {
            return;
        };
        if attribute.attr.as_str() != "user_has_groups" {
            return;
        }
        let name = &attribute.attr;
        reporter
            .report(
                &USER_HAS_GROUPS,
                call.start(),
                "`user_has_groups()` was removed in Odoo 18.0; use `env.user.has_groups()`",
            )
            .fix = Some(Fix::safe(
            "Use `env.user.has_groups()`",
            vec![Edit::replace(
                name.start().to_usize(),
                name.end().to_usize(),
                "env.user.has_groups",
            )],
        ));
    });
}

pub const PYTHON_TREE_VIEW: Rule = Rule {
    code: "U1809",
    name: "upgrade-python-tree-view",
    summary: "Python code refers to the `tree` view type, renamed `list` in Odoo 18.0.",
    doc: r#"
## What it does

Reports `tree` in action dictionaries built in Python: a `view_mode` value
such as `"tree,form"`, `(view_id, "tree")` in `views`, and `tree_view_ref`
keys in contexts.

## Why is this bad?

Since Odoo 18.0 the view type is `list`. An action with `tree` fails when it
is opened, and `tree_view_ref` is silently ignored.

## Fix safety

Safe: the strings are changed in place.
"#,
    check: Check::Python(check_python_tree_view),
    min_odoo: ODOO_18,
    max_odoo: None,
};

/// A string literal written as one piece, as it is in the source.
fn simple_string<'a>(ctx: &'a PythonContext, expr: &'a Expr) -> Option<(&'a StringLiteralValue, &'a str)> {
    let literal = expr.as_string_literal_expr()?;
    (!literal.value.is_implicit_concatenated()).then(|| (&literal.value, &ctx.source[literal.range()]))
}

fn check_python_tree_view(ctx: &PythonContext, reporter: &mut Reporter) {
    let report = |expr: &Expr, pattern: &Regex, replacement: &str, reporter: &mut Reporter| {
        let Some((_, text)) = simple_string(ctx, expr) else {
            return;
        };
        let new = pattern.replace_all(text, replacement);
        if new == text {
            return;
        }
        let range = expr.range();
        reporter
            .report(
                &PYTHON_TREE_VIEW,
                expr.start(),
                "The `tree` view type is `list` since Odoo 18.0",
            )
            .fix = Some(Fix::safe(
            "Refer to `list`",
            vec![Edit::replace(
                range.start().to_usize(),
                range.end().to_usize(),
                new.into_owned(),
            )],
        ));
    };
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(expr) = node else { return };
        match expr {
            Expr::Dict(dict) => {
                for item in &dict.items {
                    let Some(key) = item.key.as_ref().and_then(|k| k.as_string_literal_expr()) else {
                        continue;
                    };
                    match key.value.to_str() {
                        "view_mode" | "view_type" => report(&item.value, &TREE, "list", reporter),
                        "views" => {
                            let items: &[Expr] = match &item.value {
                                Expr::List(l) => &l.elts,
                                Expr::Tuple(t) => &t.elts,
                                _ => &[],
                            };
                            for view in items {
                                if let Expr::Tuple(pair) = view {
                                    if let [_, mode] = pair.elts.as_slice() {
                                        report(mode, &TREE, "list", reporter);
                                    }
                                }
                            }
                        }
                        "tree_view_ref" => {
                            report(item.key.as_ref().unwrap(), &TREE_VIEW_REF, "list_view_ref", reporter)
                        }
                        _ => {}
                    }
                }
            }
            Expr::Call(call) => {
                // `dict(..., view_mode="tree,form")` and `with_context(tree_view_ref=...)`.
                for keyword in &call.arguments.keywords {
                    let Some(arg) = &keyword.arg else { continue };
                    match arg.as_str() {
                        "view_mode" | "view_type" => report(&keyword.value, &TREE, "list", reporter),
                        "tree_view_ref" => {
                            reporter
                                .report(
                                    &PYTHON_TREE_VIEW,
                                    keyword.start(),
                                    "The `tree` view type is `list` since Odoo 18.0",
                                )
                                .fix = Some(Fix::safe(
                                "Refer to `list`",
                                vec![Edit::replace(
                                    arg.start().to_usize(),
                                    arg.end().to_usize(),
                                    "list_view_ref",
                                )],
                            ));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    });
}

pub const NAME_SEARCH_OVERRIDE: Rule = Rule {
    code: "U1810",
    name: "upgrade-name-search-override",
    summary: "A model overrides `_name_search`, which Odoo 18.0 no longer calls.",
    doc: r#"
## What it does

Reports `_name_search` methods in model classes.

## Why is this bad?

Odoo 18.0 removed `_name_search`: an override is never called, so the custom
search in many2one dropdowns silently stops working. Override
`_search_display_name` instead, or set `_rec_names_search` when searching on
more fields is all it did.
"#,
    check: Check::Python(check_name_search_override),
    min_odoo: ODOO_18,
    max_odoo: None,
};

fn check_name_search_override(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        if ctx.semantic.odoo_model_kind(class).is_none() {
            continue;
        }
        for method in methods(class).filter(|m| m.name.as_str() == "_name_search") {
            reporter.report(
                &NAME_SEARCH_OVERRIDE,
                method.name.start(),
                "`_name_search` is no longer called since Odoo 18.0; override `_search_display_name`",
            );
        }
    }
}
