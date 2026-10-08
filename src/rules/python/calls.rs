//! Rules on function and method calls.

use super::{calls_in, source_of};
use crate::checker::{PythonContext, Reporter};
use crate::config::list_or;
use crate::rules::{Check, Rule};
use crate::semantic::{dotted_name, func_lib, func_name};
use crate::visit::{current_method, walk, Node};
use ruff_python_ast::visitor::{self, Visitor};
use ruff_python_ast::{Expr, ExprAttribute, ExprCall, Stmt, StmtFunctionDef};
use ruff_text_size::{Ranged, TextSize};

const DEFAULT_CURSOR_EXPR: &[&str] = &["cr", "self._cr", "self.cr", "self.env.cr"];
const DEFAULT_TIMEOUT_METHODS: &[&str] = &[
    "ftplib.FTP",
    "http.client.HTTPConnection",
    "http.client.HTTPSConnection",
    "odoo.addons.iap.models.iap.jsonrpc",
    "requests.delete",
    "requests.get",
    "requests.head",
    "requests.options",
    "requests.patch",
    "requests.post",
    "requests.put",
    "requests.request",
    "serial.Serial",
    "smtplib.SMTP",
    "suds.client.Client",
    "urllib.request.urlopen",
];

pub const PRINT_USED: Rule = Rule {
    code: "W8116",
    name: "print-used",
    summary: "`print()` is used instead of a logger.",
    doc: r#"
## What it does

Reports calls to the built-in `print()`.

## Why is this bad?

Odoo servers run as services: `print` output is lost or ends up outside the
log, without a timestamp, level or database name.

## Example

```python
print("Synchronising", len(records))
```

Use instead:

```python
_logger = logging.getLogger(__name__)
_logger.info("Synchronising %s", len(records))
```
"#,
    check: Check::Python(check_print),
    min_odoo: None,
    max_odoo: None,
};

pub const INVALID_COMMIT: Rule = Rule {
    code: "E8102",
    name: "invalid-commit",
    summary: "The database transaction is committed by hand.",
    doc: r#"
## What it does

Reports `cr.commit()` on the database cursor (`cr`, `self._cr`, `self.cr` and
`self.env.cr`).

## Why is this bad?

Odoo commits or rolls back the whole request. A manual commit makes a failing
request leave half its changes behind and breaks the test framework's
rollback. See the
[OCA guidelines](https://github.com/OCA/odoo-community.org/blob/master/website/Contribution/CONTRIBUTING.rst#never-commit-the-transaction).

## Configuration

```toml
[tool.odoo-lint.rules.invalid-commit]
cursor-expr = ["cr", "self._cr", "self.cr", "self.env.cr"]
```
"#,
    check: Check::Python(check_commit),
    min_odoo: None,
    max_odoo: None,
};

pub const CONTEXT_OVERRIDDEN: Rule = Rule {
    code: "W8121",
    name: "context-overridden",
    summary: "`with_context()` replaces the whole context.",
    doc: r#"
## What it does

Reports `with_context(some_dict)`: a positional argument replaces the context
instead of extending it. `with_context(clean_context(...))` is allowed.

## Why is this bad?

Keys other code relies on, such as the language, company or `tz`, are
dropped.

## Example

```python
records.with_context({"skip_check": True})
```

Use instead:

```python
records.with_context(skip_check=True)
```
"#,
    check: Check::Python(check_context_overridden),
    min_odoo: None,
    max_odoo: None,
};

pub const EXTERNAL_REQUEST_TIMEOUT: Rule = Rule {
    code: "E8106",
    name: "external-request-timeout",
    summary: "An external request has no `timeout`.",
    doc: r#"
## What it does

Reports calls to network functions without a `timeout` keyword: `requests`,
`urllib.request.urlopen`, `http.client`, `ftplib`, `smtplib`, `serial`,
`suds` and Odoo's IAP `jsonrpc`. Import aliases are followed.

## Why is this bad?

Without a timeout a request to an unresponsive server blocks an Odoo worker
until it is killed, and with it every user waiting on that worker.

## Example

```python
response = requests.get(url)
```

Use instead:

```python
response = requests.get(url, timeout=10)
```

## Configuration

```toml
[tool.odoo-lint.rules.external-request-timeout]
methods = ["requests.get", "requests.post"]
```
"#,
    check: Check::Python(check_request_timeout),
    min_odoo: None,
    max_odoo: None,
};

pub const BAD_BUILTIN_GROUPBY: Rule = Rule {
    code: "W8155",
    name: "bad-builtin-groupby",
    summary: "`itertools.groupby` is used instead of `odoo.tools.groupby`.",
    doc: r#"
## What it does

Reports calls to `itertools.groupby`, also through `from itertools import
groupby` and import aliases.

## Why is this bad?

`itertools.groupby` only groups consecutive items, so unsorted records end up
in several groups with the same key. `odoo.tools.groupby` groups all of them.
See [odoo/odoo#105376](https://github.com/odoo/odoo/issues/105376).
"#,
    check: Check::Python(check_groupby),
    min_odoo: None,
    max_odoo: None,
};

pub const NO_SEARCH_ALL: Rule = Rule {
    code: "W8163",
    name: "no-search-all",
    summary: "`search([])` without a `limit` loads every record.",
    doc: r#"
## What it does

Reports `search()` and `search_read()` with an empty domain and no `limit` or
`count` inside model methods, including a domain variable assigned `[]` in the
same method and never extended before the call.

## Why is this bad?

The query loads every record of the model, which gets slower as the database
grows and can exhaust the worker's memory.

## Example

```python
partners = self.env["res.partner"].search([])
```

Use instead: add a domain, or a `limit` when any record will do.

## Configuration

Models whose number of records is small by design can be allowed:

```toml
[tool.odoo-lint.rules.no-search-all]
# res.company: one per company (13 in production). machine.machine is not
# listed: 1,554 records and growing.
bounded-models = ["res.company"]
```

A search on `self.env["res.company"]` (also after `.sudo()`,
`.with_context(...)` and the like) is then not reported.

Count the records in a production database before listing a model, and
note the reason next to it: the name does not tell. Two models of the same
module can differ by orders of magnitude, and a model that is small today
may not stay small. There is no default list for that reason.
"#,
    check: Check::Python(check_search_all),
    min_odoo: None,
    max_odoo: None,
};

fn check_print(ctx: &PythonContext, reporter: &mut Reporter) {
    // `print` is only the builtin when the file does not rebind it.
    let shadowed = ctx.semantic.imports.iter().any(|b| b.bound == "print") || {
        let mut found = false;
        walk(ctx.parsed.suite(), |node, _| match node {
            Node::Stmt(Stmt::FunctionDef(f)) if f.name.as_str() == "print" => found = true,
            Node::Stmt(Stmt::ClassDef(c)) if c.name.as_str() == "print" => found = true,
            Node::Stmt(Stmt::Assign(a)) => {
                found |= a
                    .targets
                    .iter()
                    .any(|t| matches!(t, Expr::Name(n) if n.id.as_str() == "print"));
            }
            _ => {}
        });
        found
    };
    if shadowed {
        return;
    }
    walk(ctx.parsed.suite(), |node, _| {
        if let Node::Expr(Expr::Call(call)) = node {
            if matches!(&*call.func, Expr::Name(n) if n.id.as_str() == "print") {
                reporter.report(&PRINT_USED, call.start(), "Print used. Use `logger` instead.");
            }
        }
    });
}

/// pylint-odoo's `get_cursor_name`: `self.env.cr` for `self.env.cr.commit`.
pub(crate) fn cursor_name(func: &ExprAttribute) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut expr = &*func.value;
    loop {
        match expr {
            Expr::Attribute(attr) => {
                parts.push(attr.attr.as_str());
                expr = &attr.value;
            }
            Expr::Name(name) => {
                parts.push(name.id.as_str());
                break;
            }
            _ => break,
        }
    }
    parts.reverse();
    parts.join(".")
}

pub(crate) fn cursor_exprs(ctx: &PythonContext) -> Vec<String> {
    list_or(
        ctx.settings.config.rules().invalid_commit.as_ref(),
        |c| c.cursor_expr.as_ref(),
        DEFAULT_CURSOR_EXPR,
    )
}

fn check_commit(ctx: &PythonContext, reporter: &mut Reporter) {
    let cursors = cursor_exprs(ctx);
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(attr) = &*call.func else { return };
        if attr.attr.as_str() == "commit" && cursors.contains(&cursor_name(attr)) {
            reporter.report(
                &INVALID_COMMIT,
                call.start(),
                "Use of cr.commit() directly - More info https://github.com/OCA/odoo-community.org/blob/master/website/Contribution/CONTRIBUTING.rst#never-commit-the-transaction",
            );
        }
    });
}

fn check_context_overridden(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(attr) = &*call.func else { return };
        let Some(first) = call.arguments.args.first() else {
            return;
        };
        if attr.attr.as_str() != "with_context" || !call.arguments.keywords.is_empty() {
            return;
        }
        let clean_context = match first {
            Expr::Call(inner) => {
                ctx.semantic.qualified_call_name(&inner.func, inner.start()).as_deref()
                    == Some("odoo.tools.clean_context")
            }
            _ => false,
        };
        if !clean_context {
            reporter.report(
                &CONTEXT_OVERRIDDEN,
                call.start(),
                format!(
                    "Context overridden using dict. Better using kwargs `with_context(**{})` or `with_context(key=value)`",
                    source_of(ctx.source, first)
                ),
            );
        }
    });
}

fn check_request_timeout(ctx: &PythonContext, reporter: &mut Reporter) {
    let methods = list_or(
        ctx.settings.config.rules().external_request_timeout.as_ref(),
        |c| c.methods.as_ref(),
        DEFAULT_TIMEOUT_METHODS,
    );
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Some(name) = ctx.semantic.qualified_call_name(&call.func, call.start()) else {
            return;
        };
        if !methods.contains(&name) {
            return;
        }
        let has_timeout = call
            .arguments
            .keywords
            .iter()
            .any(|kw| kw.arg.as_ref().is_some_and(|a| a.as_str() == "timeout"));
        if !has_timeout {
            reporter.report(
                &EXTERNAL_REQUEST_TIMEOUT,
                call.start(),
                format!("Use of external request method `{name}` without timeout. It could wait for a long time"),
            );
        }
    });
}

fn check_groupby(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let func_text = dotted_name(&call.func).unwrap_or_else(|| source_of(ctx.source, &*call.func).to_string());
        if !func_text.ends_with("groupby") {
            return;
        }
        let is_itertools = func_text == "itertools.groupby"
            || (!func_text.ends_with("tools.groupby")
                && ctx.semantic.qualified_call_name(&call.func, call.start()).as_deref() == Some("itertools.groupby"));
        if is_itertools {
            reporter.report(
                &BAD_BUILTIN_GROUPBY,
                call.start(),
                "Used builtin function `itertools.groupby`. Prefer `odoo.tools.groupby` instead. More info about https://github.com/odoo/odoo/issues/105376",
            );
        }
    });
}

/// Assignments to a name in a function body, outside nested scopes.
struct AssignmentFinder<'a> {
    name: &'a str,
    /// `(offset, value)` of plain `name = value` assignments.
    assignments: Vec<(TextSize, &'a Expr)>,
    /// Offsets of any other binding: augmented, annotated, loops, `with`.
    other_bindings: Vec<TextSize>,
}

impl<'a> Visitor<'a> for AssignmentFinder<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        let is_target = |e: &Expr| matches!(e, Expr::Name(n) if n.id.as_str() == self.name);
        match stmt {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,
            Stmt::Assign(assign) if assign.targets.len() == 1 && is_target(&assign.targets[0]) => {
                self.assignments.push((assign.start(), &assign.value));
            }
            Stmt::Assign(assign) if assign.targets.iter().any(is_target) => self.other_bindings.push(assign.start()),
            Stmt::AugAssign(assign) if is_target(&assign.target) => self.other_bindings.push(assign.start()),
            Stmt::AnnAssign(assign) if is_target(&assign.target) => self.other_bindings.push(assign.start()),
            Stmt::For(for_stmt) if is_target(&for_stmt.target) => self.other_bindings.push(for_stmt.start()),
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if !matches!(expr, Expr::Lambda(_)) {
            visitor::walk_expr(self, expr);
        }
    }
}

/// Whether the domain `name` is `[]` at `call` in `method`: its only
/// assignment before the call is `[]`, and it is not extended in between.
fn is_empty_domain_variable(method: &StmtFunctionDef, name: &str, call: &ExprCall) -> bool {
    let mut finder = AssignmentFinder {
        name,
        assignments: Vec::new(),
        other_bindings: Vec::new(),
    };
    finder.visit_body(&method.body);
    let before: Vec<_> = finder.assignments.iter().filter(|(o, _)| *o < call.start()).collect();
    let [(assigned_at, Expr::List(list))] = before.as_slice() else {
        return false;
    };
    if !list.elts.is_empty() || finder.other_bindings.iter().any(|o| *o < call.start()) {
        return false;
    }
    !calls_in(&method.body).iter().any(|c| {
        c.start() >= *assigned_at
            && c.start() <= call.start()
            && func_lib(&c.func) == name
            && ["append", "extend", "insert"].contains(&func_name(&c.func))
    })
}

/// The model of `self.env["model"]....search`, through chained calls such as
/// `.sudo()` and `.with_context(...)`.
pub(crate) fn searched_model(func: &Expr) -> Option<&str> {
    let Expr::Attribute(attribute) = func else { return None };
    let mut receiver = &*attribute.value;
    loop {
        match receiver {
            Expr::Call(call) => match &*call.func {
                Expr::Attribute(method) => receiver = &method.value,
                _ => return None,
            },
            Expr::Subscript(subscript) => {
                let is_env = match &*subscript.value {
                    Expr::Attribute(env) => env.attr.as_str() == "env",
                    Expr::Name(name) => name.id.as_str() == "env",
                    _ => false,
                };
                return is_env
                    .then(|| subscript.slice.as_string_literal_expr().map(|s| s.value.to_str()))
                    .flatten();
            }
            _ => return None,
        }
    }
}

fn check_search_all(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, scopes| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let method_name = func_name(&call.func);
        let (args, keywords) = (&call.arguments.args, &call.arguments.keywords);
        if !["search", "search_read"].contains(&method_name) || (args.is_empty() && keywords.is_empty()) {
            return;
        }
        let Some((class, method)) = current_method(scopes) else {
            return;
        };
        if ctx.semantic.odoo_model_kind(class).is_none() {
            return;
        }
        let domain = args.first().or_else(|| {
            keywords
                .iter()
                .find(|kw| kw.arg.as_ref().is_some_and(|a| a.as_str() == "domain"))
                .map(|kw| &kw.value)
        });
        let empty = match domain {
            Some(Expr::List(list)) => list.elts.is_empty(),
            Some(Expr::Name(name)) => is_empty_domain_variable(method, name.id.as_str(), call),
            _ => false,
        };
        let limited = keywords.iter().any(|kw| {
            kw.arg
                .as_ref()
                .is_some_and(|a| ["limit", "count"].contains(&a.as_str()))
        }) || args.len() >= 3
            || (method_name == "search" && args.len() >= 5);
        let bounded = ctx
            .settings
            .config
            .rules()
            .no_search_all
            .as_ref()
            .and_then(|c| c.bounded_models.as_ref())
            .zip(searched_model(&call.func))
            .is_some_and(|(models, model)| models.iter().any(|m| m == model));
        if empty && !limited && !bounded {
            reporter.report(
                &NO_SEARCH_ALL,
                call.start(),
                format!(
                    "Using an empty domain `{method_name}([])` without a `limit` will load all records, may impact performance."
                ),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::run_python_rule;

    fn count(rule: &Rule, src: &str) -> usize {
        run_python_rule(rule, src).len()
    }

    #[test]
    fn print() {
        assert_eq!(count(&PRINT_USED, "print('x')\nfoo.print('y')\n"), 1);
        assert_eq!(count(&PRINT_USED, "from rich import print\nprint('x')\n"), 0);
    }

    #[test]
    fn commit() {
        let src = "cr.commit()\nself.env.cr.commit()\nself._cr.commit()\nrequest.env.cr.commit()\nself.env['x'].cr.commit()\nsession.commit()\n";
        // `self.env['x'].cr` is cut at the subscript, leaving `cr`, as in pylint-odoo.
        assert_eq!(count(&INVALID_COMMIT, src), 4);
    }

    #[test]
    fn context_overridden() {
        let src = "from odoo.tools import clean_context\nself.with_context({'a': 1})\nself.with_context(ctx)\nself.with_context(a=1)\nself.with_context(**ctx)\nself.with_context(clean_context(ctx))\n";
        let v = run_python_rule(&CONTEXT_OVERRIDDEN, src);
        assert_eq!(v.len(), 2);
        assert!(v[0].message.contains("with_context(**{'a': 1})"));
    }

    #[test]
    fn request_timeout() {
        let src = "import requests\nimport requests as r\nfrom urllib.request import urlopen\nfrom http import client\nrequests.get(u)\nr.post(u, timeout=5)\nurlopen(u)\nclient.HTTPConnection(h)\nrequests.get(u, **kw)\nsession.get(u)\n";
        let names: Vec<_> = run_python_rule(&EXTERNAL_REQUEST_TIMEOUT, src)
            .into_iter()
            .map(|v| v.message.split('`').nth(1).unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "requests.get",
                "urllib.request.urlopen",
                "http.client.HTTPConnection",
                "requests.get"
            ]
        );
    }

    #[test]
    fn groupby() {
        let src = "import itertools\nimport itertools as it\nfrom itertools import groupby\nfrom odoo.tools import groupby as ogb\nitertools.groupby(x)\nit.groupby(x)\ngroupby(x)\ntools.groupby(x)\nogb(x)\n";
        assert_eq!(count(&BAD_BUILTIN_GROUPBY, src), 3);
    }

    #[test]
    fn search_all() {
        let src = "from odoo import models\nclass A(models.Model):\n    def m(self):\n        self.search([])\n        self.search([], limit=1)\n        self.search([('a', '=', 1)])\n        domain = []\n        self.search_read(domain)\n        other = []\n        other.append(('a', '=', 1))\n        self.search(other)\n        self.search(domain=[])\n        self.search([], 0, 1)\nclass B:\n    def m(self):\n        self.search([])\n";
        assert_eq!(count(&NO_SEARCH_ALL, src), 3);
    }

    #[test]
    fn bounded_models_may_be_searched_without_a_domain() {
        let src = "from odoo import models\nclass A(models.Model):\n    _name = 'a'\n    def m(self):\n        self.env['res.company'].sudo().search([]).partner_id\n        self.env['res.partner'].search([])\n";
        assert_eq!(count(&NO_SEARCH_ALL, src), 2);
        let config = crate::config::OdooLintConfig::from_odoo_lint_toml(
            "[rules.no-search-all]\nbounded-models = [\"res.company\"]\n",
        )
        .unwrap();
        let (settings, _) = crate::settings::Settings::new(config, None, Default::default()).unwrap();
        let found = crate::checker::run_python_rule_with(&NO_SEARCH_ALL, src, "m/models/a.py", None, &settings);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 6);
    }
}
