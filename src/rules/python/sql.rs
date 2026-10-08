//! E8103 sql-injection: SQL built with string formatting before `execute()`.

use super::calls::cursor_name;
use super::source_of;
use crate::checker::{PythonContext, Reporter};
use crate::rules::{Check, Rule};
use crate::visit::{enclosing_function, walk, Node};
use ruff_python_ast::{Expr, ExprCall, InterpolatedStringElement, Operator, Stmt, StmtFunctionDef};
use ruff_text_size::Ranged;
use std::path::Path;

/// pylint-odoo always uses its default cursor expressions for this check.
const CURSOR_EXPR: &[&str] = &["cr", "self._cr", "self.cr", "self.env.cr"];

pub const SQL_INJECTION: Rule = Rule {
    code: "E8103",
    name: "sql-injection",
    summary: "SQL passed to `execute()` is built with string formatting.",
    doc: r#"
## What it does

Reports `cr.execute()` and `cr.executemany()` calls whose query is built with
`%`, `+`, `.format()` or an f-string from values that are not constants. The
query variable is followed back to its assignments in the same function.

Allowed in the formatting: constants, `self._attribute` and `self._method()`
(such as `self._table`), and `psycopg2.sql` objects. Calls with a second
argument and files named `test_*` are skipped.

## Why is this bad?

A value that reaches the query from a user or another record can change the
query itself: read other tables, bypass access rules or delete data. See the
[OCA guidelines](https://github.com/OCA/odoo-community.org/blob/master/website/Contribution/CONTRIBUTING.rst#no-sql-injection).

## Example

```python
self.env.cr.execute("SELECT id FROM res_partner WHERE name = '%s'" % name)
```

Use instead:

```python
self.env.cr.execute("SELECT id FROM res_partner WHERE name = %s", (name,))
```

Ruff's `S608` is a generic, heuristic variant of this check.
"#,
    check: Check::Python(check_sql_injection),
    min_odoo: None,
    max_odoo: None,
};

struct Checker<'a, 'c> {
    ctx: &'c PythonContext<'a>,
    function: Option<&'a StmtFunctionDef>,
}

impl<'a> Checker<'a, '_> {
    /// Values of `target = value` assignments in the enclosing function, for
    /// a target written exactly like `expr` (pylint-odoo's
    /// `_get_assignation_nodes`).
    fn assignments(&self, expr: &Expr) -> Vec<&'a Expr> {
        if !matches!(expr, Expr::Name(_) | Expr::Subscript(_)) {
            return Vec::new();
        }
        let Some(function) = self.function else {
            return Vec::new();
        };
        let wanted = source_of(self.ctx.source, expr);
        let mut values = Vec::new();
        walk(&function.body, |node, _| {
            if let Node::Stmt(Stmt::Assign(assign)) = node {
                if assign
                    .targets
                    .first()
                    .is_some_and(|t| source_of(self.ctx.source, t) == wanted)
                {
                    values.push(&*assign.value);
                }
            }
        });
        values
    }

    /// pylint-odoo's `_is_psycopg2_sql`: a call to something imported from
    /// `psycopg2`, directly or through a variable.
    fn is_psycopg2_sql(&self, expr: &Expr) -> bool {
        // Follow `name = value` once per level; `x = x` does not recurse.
        let follows = |v: &&Expr| !matches!(v, Expr::Name(_)) && self.is_psycopg2_sql(v);
        if matches!(expr, Expr::Name(_)) && self.assignments(expr).iter().any(follows) {
            return true;
        }
        let Expr::Call(call) = expr else { return false };
        if !matches!(&*call.func, Expr::Attribute(_) | Expr::Name(_)) {
            return false;
        }
        let root = source_of(self.ctx.source, &*call.func)
            .split('.')
            .next()
            .unwrap_or_default();
        // The first top-level binding of that name must be a psycopg2 import.
        self.ctx
            .semantic
            .imports
            .iter()
            .find(|b| b.top_level && b.bound == root)
            .is_some_and(|b| b.qualified.split('.').next() == Some("psycopg2"))
    }

    /// pylint-odoo's `_sqli_allowable`.
    fn allowable(&self, expr: &Expr) -> bool {
        if self.is_psycopg2_sql(expr) {
            return true;
        }
        let expr = match expr {
            Expr::Call(call) => &*call.func,
            other => other,
        };
        match expr {
            Expr::Attribute(attr) => matches!(&*attr.value, Expr::Name(_)) && attr.attr.as_str().starts_with('_'),
            Expr::StringLiteral(_)
            | Expr::BytesLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::BooleanLiteral(_)
            | Expr::NoneLiteral(_)
            | Expr::EllipsisLiteral(_) => true,
            _ => false,
        }
    }

    /// pylint-odoo's `_check_node_for_sqli_risk`.
    fn is_risky(&self, expr: &Expr) -> bool {
        match expr {
            Expr::BinOp(binop) if matches!(binop.op, Operator::Mod | Operator::Add) => {
                let right_risky = match &*binop.right {
                    Expr::Tuple(tuple) => !tuple.elts.iter().all(|e| self.allowable(e)),
                    Expr::Dict(dict) => !dict.items.iter().all(|item| self.allowable(&item.value)),
                    right => !self.allowable(right),
                };
                right_risky || (!self.allowable(&binop.left) && self.is_risky(&binop.left))
            }
            Expr::Call(call) => match &*call.func {
                Expr::Attribute(attr) if attr.attr.as_str() == "format" => {
                    !call.arguments.args.iter().all(|a| self.allowable(a))
                        || !call.arguments.keywords.iter().all(|kw| self.allowable(&kw.value))
                }
                _ => false,
            },
            Expr::FString(fstring) => fstring.value.f_strings().any(|f| {
                f.elements.iter().any(|element| match element {
                    InterpolatedStringElement::Interpolation(interpolation) => {
                        !self.allowable(&interpolation.expression)
                    }
                    InterpolatedStringElement::Literal(_) => false,
                })
            }),
            _ => false,
        }
    }
}

fn is_execute(call: &ExprCall) -> bool {
    let Expr::Attribute(attr) = &*call.func else {
        return false;
    };
    matches!(attr.attr.as_str(), "execute" | "executemany")
        && call.arguments.args.len() == 1
        && CURSOR_EXPR.contains(&cursor_name(attr).as_str())
}

fn check_sql_injection(ctx: &PythonContext, reporter: &mut Reporter) {
    let is_test_file = Path::new(ctx.file_path)
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with("test_"));
    if is_test_file {
        return;
    }
    walk(ctx.parsed.suite(), |node, scopes| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        if !is_execute(call) {
            return;
        }
        let checker = Checker {
            ctx,
            function: enclosing_function(scopes),
        };
        let query = &call.arguments.args[0];
        let risky = checker.is_risky(query) || checker.assignments(query).iter().any(|v| checker.is_risky(v));
        if risky {
            reporter.report(
                &SQL_INJECTION,
                call.start(),
                "SQL injection risk. Use parameters if you can. - More info https://github.com/OCA/odoo-community.org/blob/master/website/Contribution/CONTRIBUTING.rst#no-sql-injection",
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::{run_python_rule, run_python_rule_with};
    use crate::settings::Settings;

    #[test]
    fn injections() {
        let src = r#"
from psycopg2 import sql


class A:
    def m(self, name, ids):
        self.env.cr.execute("SELECT * FROM t WHERE name = '%s'" % name)
        self.env.cr.execute("SELECT * FROM %s" % self._table)
        self.env.cr.execute("SELECT * FROM t WHERE id IN %s", (tuple(ids),))
        self.env.cr.execute("SELECT * FROM " + name)
        self.env.cr.execute("SELECT * FROM {}".format(name))
        self.env.cr.execute(f"SELECT * FROM {name}")
        self.env.cr.execute(f"SELECT * FROM {self._table}")
        query = "SELECT * FROM t WHERE x = %s" % name
        self.env.cr.execute(query)
        self.env.cr.execute(sql.SQL("SELECT * FROM {}").format(sql.Identifier(name)))
        self.env.cr.execute("SELECT %s" % (1, ))
        other.execute("SELECT %s" % name)
"#;
        assert_eq!(run_python_rule(&SQL_INJECTION, src).len(), 5);
    }

    #[test]
    fn test_files_are_skipped() {
        let src = "cr.execute('x %s' % y)\n";
        let settings = Settings::default();
        assert_eq!(
            run_python_rule_with(&SQL_INJECTION, src, "m/tests/test_x.py", None, &settings).len(),
            0
        );
        assert_eq!(
            run_python_rule_with(&SQL_INJECTION, src, "m/models/x.py", None, &settings).len(),
            1
        );
    }
}
