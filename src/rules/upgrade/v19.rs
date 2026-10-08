//! Changes of Odoo 19.0 (`odoo/upgrade_code/18.*` and the 18.0 -> 19.0
//! differences of views, schemas and models).

use super::{arch_elements, record_field, replace_in, view_archs};
use crate::checker::{ManifestContext, PythonContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::rules::python::calls::searched_model;
use crate::rules::python::{classes, field_definitions, methods, source_of};
use crate::rules::xml::{at, child_field, is, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use crate::semantic::func_name;
use crate::visit::{walk, Node};
use regex::Regex;
use ruff_python_ast::{Expr, Stmt};
use ruff_text_size::Ranged;
use std::sync::LazyLock;

const ODOO_19: Option<OdooVersion> = Some(OdooVersion::new(19, 0));

/// Deletes the whole line(s) of `range` when nothing else is on them.
fn delete_lines(source: &str, start: usize, end: usize) -> Edit {
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let rest = &source[end..];
    let after = rest.trim_start_matches([' ', '\t']);
    if source[line_start..start].trim().is_empty() && after.starts_with('\n') {
        Edit::delete(line_start, end + (rest.len() - after.len()) + 1)
    } else {
        Edit::delete(start, end)
    }
}

/// Renames an identifier (keyword, attribute or parameter name).
fn rename(range: ruff_text_size::TextRange, new: &str) -> Edit {
    Edit::replace(range.start().to_usize(), range.end().to_usize(), new)
}

// --- Python ---------------------------------------------------------------

pub const ENV_SHORTCUTS: Rule = Rule {
    code: "U1901",
    name: "upgrade-env-shortcuts",
    summary: "`._uid`, `._context` or `request.cr/uid/context`, deprecated in Odoo 19.0.",
    doc: r#"
## What it does

Reports `self._uid` and `self._context`, and `request.cr`, `request.uid` and
`request.context` in controllers. (`self._cr` in models is W8165.)

## Why is this bad?

Odoo 19.0 deprecated these shortcuts for the environment's `uid`, `context`
and `cr` (Odoo's script `18.5-00-deprecated-properties`).

## Fix safety

Safe: `x._uid` becomes `x.env.uid`, `request.cr` becomes `request.env.cr`.
The `env` attributes exist in every supported version.
"#,
    check: Check::Python(check_env_shortcuts),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_env_shortcuts(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Attribute(attribute)) = node else {
            return;
        };
        let name = attribute.attr.as_str();
        let new = match name {
            "_uid" => "env.uid",
            "_context" => "env.context",
            "cr" | "uid" | "context" if matches!(&*attribute.value, Expr::Name(n) if n.id.as_str() == "request") => {
                match name {
                    "cr" => "env.cr",
                    "uid" => "env.uid",
                    _ => "env.context",
                }
            }
            _ => return,
        };
        reporter
            .report(
                &ENV_SHORTCUTS,
                attribute.attr.start(),
                format!("`.{name}` is deprecated since Odoo 19.0; use `.{new}`"),
            )
            .fix = Some(Fix::safe(
            format!("Use `.{new}`"),
            vec![rename(attribute.attr.range(), new)],
        ));
    });
}

pub const SQL_CONSTRAINTS: Rule = Rule {
    code: "U1902",
    name: "upgrade-sql-constraints",
    summary: "`_sql_constraints`, silently ignored since Odoo 19.0.",
    doc: r#"
## What it does

Reports `_sql_constraints` in model classes.

## Why is this bad?

Odoo 19.0 no longer reads `_sql_constraints`: it logs a warning and the
constraint is not created, so duplicates and invalid rows get in. Declare
each constraint as a `models.Constraint` attribute instead.

## Example

```python
_sql_constraints = [
    ("code_uniq", "unique(code)", "The code must be unique."),
]
```

Use instead:

```python
_code_uniq = models.Constraint("unique(code)", "The code must be unique.")
```

## Fix safety

Safe, as Odoo's script `18.1-00-sql-constraint` does it, when the list holds
only `(name, definition, message)` tuples with a literal name and the file
uses `models.`. Otherwise there is no fix.
"#,
    check: Check::Python(check_sql_constraints),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_sql_constraints(ctx: &PythonContext, reporter: &mut Reporter) {
    let identifier = Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").expect("valid");
    for class in classes(ctx.parsed.suite()) {
        for stmt in &class.body {
            let Stmt::Assign(assign) = stmt else { continue };
            let is_target = assign
                .targets
                .iter()
                .any(|t| matches!(t, Expr::Name(n) if n.id.as_str() == "_sql_constraints"));
            if !is_target {
                continue;
            }
            let items: &[Expr] = match &*assign.value {
                Expr::List(list) => &list.elts,
                Expr::Tuple(tuple) => &tuple.elts,
                _ => &[],
            };
            let mut lines = Vec::new();
            for item in items {
                let Expr::Tuple(tuple) = item else { break };
                let [name, definition, message] = tuple.elts.as_slice() else {
                    break;
                };
                let Some(name) = name.as_string_literal_expr().map(|s| s.value.to_str()) else {
                    break;
                };
                if !identifier.is_match(name) {
                    break;
                }
                lines.push(format!(
                    "_{name} = models.Constraint({}, {})",
                    source_of(ctx.source, definition),
                    source_of(ctx.source, message)
                ));
            }
            let convertible = !lines.is_empty() && lines.len() == items.len() && ctx.source.contains("models.");
            let fix = convertible.then(|| {
                let start = stmt.start().to_usize();
                let line_start = ctx.source[..start].rfind('\n').map_or(0, |i| i + 1);
                let indent = &ctx.source[line_start..start];
                Fix::safe(
                    "Use `models.Constraint` attributes",
                    vec![Edit::replace(
                        start,
                        stmt.end().to_usize(),
                        lines.join(&format!("\n{indent}")),
                    )],
                )
            });
            reporter
                .report(
                    &SQL_CONSTRAINTS,
                    stmt.start(),
                    "`_sql_constraints` is ignored since Odoo 19.0; use `models.Constraint` attributes",
                )
                .fix = fix;
        }
    }
}

pub const API_MODEL_CREATE: Rule = Rule {
    code: "U1903",
    name: "upgrade-api-model-create",
    summary: "`create` is decorated with `@api.model`, which makes it a batch create in Odoo 19.0.",
    doc: r#"
## What it does

Reports `create` methods decorated with `@api.model`.

## Why is this bad?

Since Odoo 19.0, `@api.model` on `create` wraps it as `@api.model_create_multi`:
the argument is a list of dicts. Code that treats it as one dict
(`vals.get(...)`, `vals["x"] = ...`) breaks at runtime.

Use `@api.model_create_multi` and loop over `vals_list`.
"#,
    check: Check::Python(check_api_model_create),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_api_model_create(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        for method in methods(class).filter(|m| m.name.as_str() == "create") {
            let api_model = method.decorator_list.iter().any(|d| match &d.expression {
                Expr::Attribute(a) => a.attr.as_str() == "model",
                Expr::Name(n) => n.id.as_str() == "model",
                _ => false,
            });
            if api_model {
                reporter.report(
                    &API_MODEL_CREATE,
                    method.name.start(),
                    "`@api.model` on `create` receives a list of dicts since Odoo 19.0; use `@api.model_create_multi`",
                );
            }
        }
    }
}

pub const API_RETURNS: Rule = Rule {
    code: "U1904",
    name: "upgrade-api-returns",
    summary: "`@api.returns`, removed in Odoo 19.0.",
    doc: r#"
## What it does

Reports the `@api.returns(...)` decorator.

## Why is this bad?

Odoo 19.0 removed `api.returns` without a deprecation period: the module
fails to load with `AttributeError`.

## Fix safety

Safe: the decorator line is removed.
"#,
    check: Check::Python(check_api_returns),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_api_returns(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Stmt(Stmt::FunctionDef(function)) = node else {
            return;
        };
        for decorator in &function.decorator_list {
            let Expr::Call(call) = &decorator.expression else {
                continue;
            };
            if !matches!(&*call.func, Expr::Attribute(a) if a.attr.as_str() == "returns") {
                continue;
            }
            let range = decorator.range();
            reporter
                .report(
                    &API_RETURNS,
                    decorator.start(),
                    "`@api.returns` was removed in Odoo 19.0",
                )
                .fix = Some(Fix::safe(
                "Remove the decorator",
                vec![delete_lines(
                    ctx.source,
                    range.start().to_usize(),
                    range.end().to_usize(),
                )],
            ));
        }
    });
}

pub const READ_GROUP_OVERRIDE: Rule = Rule {
    code: "U1905",
    name: "upgrade-read-group-override",
    summary: "A model overrides `read_group`, which the web client no longer calls in Odoo 19.0.",
    doc: r#"
## What it does

Reports `read_group` methods in model classes.

## Why is this bad?

Odoo 19.0 deprecated `read_group`: the web client calls
`formatted_read_group`, so an override silently stops affecting grouped
list and pivot views. Override `_read_group` (backend) or
`formatted_read_group` (what the UI shows) instead. Odoo 20.0 changes the
signature of `read_group` again.
"#,
    check: Check::Python(check_read_group_override),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_read_group_override(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        if ctx.semantic.odoo_model_kind(class).is_none() {
            continue;
        }
        for method in methods(class).filter(|m| m.name.as_str() == "read_group") {
            reporter.report(
                &READ_GROUP_OVERRIDE,
                method.name.start(),
                "`read_group` overrides are not called by the web client since Odoo 19.0",
            );
        }
    }
}

pub const ROUTE_JSON: Rule = Rule {
    code: "U1906",
    name: "upgrade-route-json",
    summary: "A route uses `type='json'`, renamed `'jsonrpc'` in Odoo 19.0.",
    doc: r#"
## What it does

Reports `type="json"` in `@http.route(...)`.

## Why is this bad?

Odoo 19.0 renamed the route type `jsonrpc`; `json` is a deprecated alias
(Odoo's script `18.1-02-route-jsonrpc`).

## Fix safety

Safe: the value becomes `jsonrpc`.
"#,
    check: Check::Python(check_route_json),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_route_json(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        if func_name(&call.func) != "route" {
            return;
        }
        for keyword in &call.arguments.keywords {
            if keyword.arg.as_ref().is_none_or(|a| a.as_str() != "type") {
                continue;
            }
            let Some(value) = keyword.value.as_string_literal_expr() else {
                continue;
            };
            if value.value.to_str() != "json" {
                continue;
            }
            let text = source_of(ctx.source, &keyword.value);
            let quote = text.chars().next().unwrap_or('"');
            reporter
                .report(
                    &ROUTE_JSON,
                    keyword.start(),
                    "Route type `json` is `jsonrpc` since Odoo 19.0",
                )
                .fix = Some(Fix::safe(
                "Use `jsonrpc`",
                vec![rename(keyword.value.range(), &format!("{quote}jsonrpc{quote}"))],
            ));
        }
    });
}

pub const PYTHON_GROUPS_ID: Rule = Rule {
    code: "U1907",
    name: "upgrade-python-groups-id",
    summary: "Python code uses `groups_id`, renamed `group_ids` in Odoo 19.0.",
    doc: r#"
## What it does

Reports `.groups_id` and the string `"groups_id"` (in values for `create` and
`write`, domains and `mapped`), and `.users` of groups.

## Why is this bad?

Odoo 19.0 renamed `groups_id` to `group_ids` on users, views, menus and
actions, without an alias: the old name raises an error.

## Fix safety

Unsafe: `groups_id` becomes `group_ids`, which is right for those models but
wrong for a custom model that has its own `groups_id` field.
"#,
    check: Check::Python(check_python_groups_id),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_python_groups_id(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(expr) = node else { return };
        let edit = match expr {
            Expr::Attribute(attribute) if attribute.attr.as_str() == "groups_id" => {
                rename(attribute.attr.range(), "group_ids")
            }
            Expr::StringLiteral(literal)
                if literal.value.to_str() == "groups_id" && !literal.value.is_implicit_concatenated() =>
            {
                let text = source_of(ctx.source, expr);
                rename(expr.range(), &text.replacen("groups_id", "group_ids", 1))
            }
            _ => return,
        };
        reporter
            .report(
                &PYTHON_GROUPS_ID,
                expr.start(),
                "`groups_id` is `group_ids` since Odoo 19.0",
            )
            .fix = Some(Fix::unsafe_("Use `group_ids`", vec![edit]));
    });
}

pub const OSV_EXPRESSION: Rule = Rule {
    code: "U1908",
    name: "upgrade-osv-expression",
    summary: "`odoo.osv.expression` is imported, deprecated in Odoo 19.0 and removed in 20.0.",
    doc: r#"
## What it does

Reports imports from `odoo.osv` and `odoo.osv.expression`.

## Why is this bad?

Odoo 19.0 replaced the domain helpers by `odoo.fields.Domain`
(`Domain.AND`, `Domain.OR`, `Domain.TRUE`, `Domain(...)`); Odoo 20.0 removed
`odoo.osv`. The result is a `Domain` object rather than a list, so code that
indexes or extends the list needs a look.
"#,
    check: Check::Python(check_osv_expression),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_osv_expression(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Stmt(Stmt::ImportFrom(import)) = node else {
            return;
        };
        let module = import.module.as_ref().map(|m| m.as_str()).unwrap_or_default();
        if import.level == 0 && (module == "odoo.osv" || module.starts_with("odoo.osv.")) {
            reporter.report(
                &OSV_EXPRESSION,
                import.start(),
                "`odoo.osv` is deprecated since Odoo 19.0 (removed in 20.0); use `odoo.fields.Domain`",
            );
        }
    });
}

pub const AUTO_JOIN: Rule = Rule {
    code: "U1909",
    name: "upgrade-auto-join",
    summary: "A field uses `auto_join=`, renamed `bypass_search_access=` in Odoo 19.0.",
    doc: r#"
## What it does

Reports the `auto_join` parameter of relational fields.

## Why is this bad?

Odoo 19.0 renamed it `bypass_search_access`, without an alias: the old name
is ignored with a warning, so searches through the field silently change.

## Fix safety

Safe: the parameter is renamed. There is no fix when the field already has
`bypass_search_access`.
"#,
    check: Check::Python(check_auto_join),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_auto_join(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        for (_, call) in field_definitions(class) {
            let keywords = &call.arguments.keywords;
            let Some(keyword) = keywords
                .iter()
                .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "auto_join"))
            else {
                continue;
            };
            let taken = keywords
                .iter()
                .any(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "bypass_search_access"));
            let arg = keyword.arg.as_ref().expect("found by name");
            reporter
                .report(
                    &AUTO_JOIN,
                    keyword.start(),
                    "`auto_join` is `bypass_search_access` since Odoo 19.0",
                )
                .fix = (!taken).then(|| {
                Fix::safe(
                    "Rename to `bypass_search_access`",
                    vec![rename(arg.range(), "bypass_search_access")],
                )
            });
        }
    }
}

pub const NAME_SEARCH_ARGS: Rule = Rule {
    code: "U1910",
    name: "upgrade-name-search-args",
    summary: "`name_search(args=...)`, renamed `domain=` in Odoo 19.0.",
    doc: r#"
## What it does

Reports the `args` keyword in calls to `name_search`, and `name_search`
overrides with an `args` parameter.

## Why is this bad?

Odoo 19.0 renamed the parameter `domain`: a keyword call fails, and an
override no longer matches the calls it receives.

## Fix safety

Safe for calls: the keyword is renamed. Overrides are reported only: their
body uses the name too.
"#,
    check: Check::Python(check_name_search_args),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_name_search_args(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| match node {
        Node::Expr(Expr::Call(call)) if func_name(&call.func) == "name_search" => {
            for keyword in &call.arguments.keywords {
                let Some(arg) = keyword.arg.as_ref().filter(|a| a.as_str() == "args") else {
                    continue;
                };
                reporter
                    .report(
                        &NAME_SEARCH_ARGS,
                        keyword.start(),
                        "`name_search(args=)` is `domain=` since Odoo 19.0",
                    )
                    .fix = Some(Fix::safe("Rename to `domain`", vec![rename(arg.range(), "domain")]));
            }
        }
        Node::Stmt(Stmt::FunctionDef(function))
            if function.name.as_str() == "name_search" && function.parameters.includes("args") =>
        {
            reporter.report(
                &NAME_SEARCH_ARGS,
                function.name.start(),
                "The `args` parameter of `name_search` is `domain` since Odoo 19.0",
            );
        }
        _ => {}
    });
}

pub const CLEAR_CACHES: Rule = Rule {
    code: "U1911",
    name: "upgrade-clear-caches",
    summary: "`clear_caches()`, removed in Odoo 19.0.",
    doc: r#"
## What it does

Reports calls to `.clear_caches()`.

## Why is this bad?

Odoo 19.0 removed the method: the call raises `AttributeError`.

## Fix safety

Safe: `x.clear_caches()` becomes `x.env.registry.clear_cache()`.
"#,
    check: Check::Python(check_clear_caches),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_clear_caches(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(attribute) = &*call.func else {
            return;
        };
        if attribute.attr.as_str() != "clear_caches" || !call.arguments.is_empty() {
            return;
        }
        reporter
            .report(&CLEAR_CACHES, call.start(), "`clear_caches()` was removed in Odoo 19.0")
            .fix = Some(Fix::safe(
            "Use `env.registry.clear_cache()`",
            vec![rename(attribute.attr.range(), "env.registry.clear_cache")],
        ));
    });
}

pub const REMOVED_HELPERS: Rule = Rule {
    code: "U1912",
    name: "upgrade-removed-helpers",
    summary: "`get_module_resource`, `get_resource_path` or `odoo.registry()`, removed in Odoo 19.0.",
    doc: r#"
## What it does

Reports uses of `get_module_resource`, `get_resource_path` and
`odoo.registry()`.

## Why is this bad?

Odoo 19.0 removed them. Use `odoo.tools.misc.file_path("module/path")` for
files and `odoo.modules.registry.Registry(dbname)` for registries.
"#,
    check: Check::Python(check_removed_helpers),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_removed_helpers(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let name = func_name(&call.func);
        let registry = name == "registry"
            && matches!(&*call.func, Expr::Attribute(a) if matches!(&*a.value, Expr::Name(n) if n.id.as_str() == "odoo"));
        if registry || matches!(name, "get_module_resource" | "get_resource_path") {
            reporter.report(
                &REMOVED_HELPERS,
                call.start(),
                format!("`{name}()` was removed in Odoo 19.0"),
            );
        }
    });
}

pub const SEQUENCE_GET: Rule = Rule {
    code: "U1913",
    name: "upgrade-sequence-get",
    summary: "`ir.sequence` `get()`/`get_id()`, removed in Odoo 19.0.",
    doc: r#"
## What it does

Reports `self.env["ir.sequence"].get(...)` and `.get_id(...)`.

## Why is this bad?

Odoo 19.0 removed these long-deprecated aliases.

## Fix safety

Safe: `get` becomes `next_by_code` and `get_id` becomes `next_by_id`.
"#,
    check: Check::Python(check_sequence_get),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_sequence_get(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(attribute) = &*call.func else {
            return;
        };
        let new = match attribute.attr.as_str() {
            "get" => "next_by_code",
            "get_id" => "next_by_id",
            _ => return,
        };
        if searched_model(&call.func) != Some("ir.sequence") {
            return;
        }
        reporter
            .report(
                &SEQUENCE_GET,
                call.start(),
                format!(
                    "`ir.sequence.{}()` was removed in Odoo 19.0; use `{new}()`",
                    attribute.attr
                ),
            )
            .fix = Some(Fix::safe(
            format!("Use `{new}()`"),
            vec![rename(attribute.attr.range(), new)],
        ));
    });
}

pub const DOMAIN_OPERATORS: Rule = Rule {
    code: "U1914",
    name: "upgrade-domain-operators",
    summary: "A domain uses `<>`, `==` or an upper-case operator, deprecated in Odoo 19.0.",
    doc: r#"
## What it does

Reports domain terms with the operators `<>`, `==` or an upper-case
operator such as `ILIKE`.

## Why is this bad?

Odoo 19.0 deprecated them in favour of `!=`, `=` and lower case.

## Fix safety

Safe: the operator is replaced.
"#,
    check: Check::Python(check_domain_operators),
    min_odoo: ODOO_19,
    max_odoo: None,
};

const OPERATORS: &[&str] = &[
    "like",
    "ilike",
    "not like",
    "not ilike",
    "=like",
    "=ilike",
    "in",
    "not in",
    "child_of",
    "parent_of",
    "any",
    "not any",
];

fn check_domain_operators(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::List(list)) = node else { return };
        for term in &list.elts {
            let Expr::Tuple(tuple) = term else { continue };
            let [field, operator, _] = tuple.elts.as_slice() else {
                continue;
            };
            if field.as_string_literal_expr().is_none() {
                continue;
            }
            let Some(literal) = operator.as_string_literal_expr() else {
                continue;
            };
            let old = literal.value.to_str();
            let new = match old {
                "<>" => "!=".to_string(),
                "==" => "=".to_string(),
                other if other != other.to_lowercase() && OPERATORS.contains(&other.to_lowercase().as_str()) => {
                    other.to_lowercase()
                }
                _ => continue,
            };
            let text = source_of(ctx.source, operator);
            let quote = text.chars().next().unwrap_or('"');
            reporter
                .report(
                    &DOMAIN_OPERATORS,
                    operator.start(),
                    format!("Domain operator `{old}` is deprecated since Odoo 19.0; use `{new}`"),
                )
                .fix = Some(Fix::safe(
                format!("Use `{new}`"),
                vec![rename(operator.range(), &format!("{quote}{new}{quote}"))],
            ));
        }
    });
}

pub const MODELS_NEWID: Rule = Rule {
    code: "U1915",
    name: "upgrade-models-newid",
    summary: "`from odoo.models import NewId`, which fails in Odoo 19.0.",
    doc: r#"
## What it does

Reports `NewId` imported from `odoo.models`.

## Why is this bad?

Odoo 19.0 narrowed what `odoo.models` exports: the import fails. `NewId`
lives in `odoo.api` (since 18.0).

## Fix safety

Safe when `NewId` is the only name imported: the module becomes `odoo.api`.
"#,
    check: Check::Python(check_models_newid),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_models_newid(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Stmt(Stmt::ImportFrom(import)) = node else {
            return;
        };
        let Some(module) = import.module.as_ref().filter(|m| m.as_str() == "odoo.models") else {
            return;
        };
        if import.level != 0 || !import.names.iter().any(|a| a.name.as_str() == "NewId") {
            return;
        }
        let fix = (import.names.len() == 1)
            .then(|| Fix::safe("Import from `odoo.api`", vec![rename(module.range(), "odoo.api")]));
        reporter
            .report(
                &MODELS_NEWID,
                import.start(),
                "Import `NewId` from `odoo.api` since Odoo 19.0",
            )
            .fix = fix;
    });
}

// --- XML ------------------------------------------------------------------

/// Models whose `groups_id` field is `group_ids` since 19.0.
const GROUPS_MODELS: &[&str] = &[
    "res.users",
    "ir.ui.view",
    "ir.ui.menu",
    "ir.actions.act_window",
    "ir.actions.server",
    "ir.actions.report",
];

static GROUPS_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgroups_id\b").unwrap());

pub const XML_GROUPS_ID: Rule = Rule {
    code: "U1916",
    name: "upgrade-groups-id",
    summary: "A record or view uses `groups_id`, renamed `group_ids` in Odoo 19.0.",
    doc: r#"
## What it does

Reports `<field name="groups_id">` in records of users, views, menus and
actions, and in views of those models (fields and xpaths). The `groups=`
attribute is not affected.

## Why is this bad?

Odoo 19.0 renamed the field without an alias: installing fails with
"Invalid field".

## Fix safety

Safe: `groups_id` becomes `group_ids`.
"#,
    check: Check::Xml(check_xml_groups_id),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_xml_groups_id(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        // Records of the models.
        for field in file.elements().filter(|f| record_field(*f, GROUPS_MODELS, "groups_id")) {
            let Some(name) = field.attributes().find(|a| a.name() == "name") else {
                continue;
            };
            reporter
                .report(
                    &XML_GROUPS_ID,
                    file,
                    at(file, field),
                    "`groups_id` is `group_ids` since Odoo 19.0",
                )
                .fix = Some(Fix::safe(
                "Use `group_ids`",
                vec![Edit::replace(
                    name.range_value().start,
                    name.range_value().end,
                    "group_ids",
                )],
            ));
        }
        // Views of the models.
        for (record, arch) in view_archs(file) {
            let model = child_field(record, "model").and_then(|m| m.text()).map(str::trim);
            if !model.is_some_and(|m| GROUPS_MODELS.contains(&m)) {
                continue;
            }
            for node in arch.descendants().filter(|n| n.is_element()) {
                let attribute = if is(node, "field") {
                    node.attributes()
                        .find(|a| a.name() == "name" && a.value() == "groups_id")
                } else if is(node, "xpath") {
                    node.attributes()
                        .find(|a| a.name() == "expr" && GROUPS_ID.is_match(a.value()))
                } else {
                    None
                };
                let Some(attribute) = attribute else { continue };
                let fix = replace_in(file, attribute.range_value(), &GROUPS_ID, "group_ids")
                    .map(|e| Fix::safe("Use `group_ids`", vec![e]));
                reporter
                    .report(
                        &XML_GROUPS_ID,
                        file,
                        at(file, node),
                        "`groups_id` is `group_ids` since Odoo 19.0",
                    )
                    .fix = fix;
            }
        }
    }
}

pub const XML_SEARCH_GROUP: Rule = Rule {
    code: "U1917",
    name: "upgrade-search-group-attributes",
    summary: "A search view's `<group>` has `expand` or `string`, rejected in Odoo 19.0.",
    doc: r#"
## What it does

Reports `expand` and `string` on `<group>` in search views, such as
`<group expand="0" string="Group By">`.

## Why is this bad?

Odoo 19.0 removed both attributes from the search view schema: the view
fails to install. Odoo dropped them from its own search views.

## Fix safety

Safe: the attributes are removed.
"#,
    check: Check::Xml(check_search_group),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_search_group(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in arch_elements(file) {
            if !is(node, "group") || !node.ancestors().any(|a| is(a, "search")) {
                continue;
            }
            let mut edits = Vec::new();
            for attribute in node.attributes().filter(|a| matches!(a.name(), "expand" | "string")) {
                let range = attribute.range();
                let before = file.source[..range.start]
                    .trim_end_matches([' ', '\t', '\n', '\r'])
                    .len();
                edits.push(Edit::delete(before, range.end));
            }
            if edits.is_empty() {
                continue;
            }
            reporter
                .report(
                    &XML_SEARCH_GROUP,
                    file,
                    at(file, node),
                    "`expand`/`string` on a search view `<group>` are rejected since Odoo 19.0",
                )
                .fix = Some(Fix::safe("Remove the attributes", edits));
        }
    }
}

pub const XML_GROUPS_USERS: Rule = Rule {
    code: "U1918",
    name: "upgrade-groups-users",
    summary: "A `res.groups` record sets `users`, renamed `user_ids` in Odoo 19.0.",
    doc: r#"
## What it does

Reports `<field name="users">` in `res.groups` records.

## Why is this bad?

Odoo 19.0 renamed the field `user_ids`: installing fails with "Invalid
field". (`category_id` is XML016.)

## Fix safety

Safe: `users` becomes `user_ids`.
"#,
    check: Check::Xml(check_groups_users),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_groups_users(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for field in file.elements().filter(|f| record_field(*f, &["res.groups"], "users")) {
            let Some(name) = field.attributes().find(|a| a.name() == "name") else {
                continue;
            };
            reporter
                .report(
                    &XML_GROUPS_USERS,
                    file,
                    at(file, field),
                    "`res.groups.users` is `user_ids` since Odoo 19.0",
                )
                .fix = Some(Fix::safe(
                "Use `user_ids`",
                vec![Edit::replace(
                    name.range_value().start,
                    name.range_value().end,
                    "user_ids",
                )],
            ));
        }
    }
}

pub const XML_T_CALL_ELEMENT: Rule = Rule {
    code: "U1920",
    name: "upgrade-t-call-element",
    summary: "`t-call` on an element other than `<t>`, rejected in Odoo 19.0.",
    doc: r#"
## What it does

Reports `t-call` on elements such as `<div>` or `<span>`.

## Why is this bad?

Odoo 19.0 requires `t-call` to be on a `<t>`: rendering fails with "t-call
must be on a <t> element". Wrap it: `<div><t t-call="..."/></div>`, or
move the attributes into the called template.
"#,
    check: Check::Xml(check_t_call_element),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_t_call_element(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in arch_elements(file) {
            if node.attribute("t-call").is_some() && !is(node, "t") {
                reporter.report(
                    &XML_T_CALL_ELEMENT,
                    file,
                    at(file, node),
                    format!(
                        "`t-call` on `<{}>` is rejected since Odoo 19.0; use a `<t>`",
                        node.tag_name().name()
                    ),
                );
            }
        }
    }
}

pub const XML_PARTNER_MOBILE: Rule = Rule {
    code: "U1921",
    name: "upgrade-partner-mobile-title",
    summary: "A partner or company view uses `mobile` or `title`, removed in Odoo 19.0.",
    doc: r#"
## What it does

Reports `mobile` and `title` fields in views of `res.partner` and
`res.company`.

## Why is this bad?

Odoo 19.0 removed `res.partner.mobile`, `res.company.mobile`, the partner
`title` field and the `res.partner.title` model: the view fails to
install. Merge mobile numbers into `phone`, or keep the field in your own
module.
"#,
    check: Check::Xml(check_partner_mobile),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_partner_mobile(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for (record, arch) in view_archs(file) {
            let model = child_field(record, "model")
                .and_then(|m| m.text())
                .map(str::trim)
                .unwrap_or_default();
            if !matches!(model, "res.partner" | "res.company") {
                continue;
            }
            for node in arch.descendants().filter(|n| is(*n, "field")) {
                let Some(name) = node.attribute("name").filter(|n| matches!(*n, "mobile" | "title")) else {
                    continue;
                };
                // Only fields of the view's model, not of a sub-view.
                if node
                    .ancestors()
                    .skip(1)
                    .take_while(|a| *a != arch)
                    .any(|a| is(a, "field"))
                {
                    continue;
                }
                reporter.report(
                    &XML_PARTNER_MOBILE,
                    file,
                    at(file, node),
                    format!("`{model}.{name}` was removed in Odoo 19.0"),
                );
            }
        }
    }
}

// --- Manifest -------------------------------------------------------------

pub const MANIFEST_OLD_DATA_KEYS: Rule = Rule {
    code: "U1919",
    name: "upgrade-manifest-old-data-keys",
    summary: "The manifest lists files under `update_xml` or `demo_xml`, ignored since Odoo 19.0.",
    doc: r#"
## What it does

Reports the manifest keys `update_xml` and `demo_xml`.

## Why is this bad?

Odoo 19.0 only loads `data` and `demo` (and `init_xml`, which 20.0 ignores
too): the files under the old keys are silently never loaded. Move them to
`data` and `demo`.
"#,
    check: Check::Manifest(check_manifest_old_data_keys),
    min_odoo: ODOO_19,
    max_odoo: None,
};

fn check_manifest_old_data_keys(ctx: &ManifestContext, reporter: &mut Reporter) {
    for key in ["update_xml", "demo_xml"] {
        if let Some((key_expr, _)) = ctx.manifest.entry(key) {
            let target = if key == "update_xml" { "data" } else { "demo" };
            reporter.report(
                &MANIFEST_OLD_DATA_KEYS,
                key_expr.start(),
                format!("`{key}` is ignored since Odoo 19.0; move its files to `{target}`"),
            );
        }
    }
}
