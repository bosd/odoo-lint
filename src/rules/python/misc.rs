//! Smaller rules: silent excepts, deprecated domain operators, vim modelines.

use crate::checker::{PythonContext, Reporter};
use crate::odoo_version::OdooVersion;
use crate::pyliteral::repr_str;
use crate::rules::{Check, Rule};
use crate::visit::{walk, Node};
use ruff_python_ast::token::TokenKind;
use ruff_python_ast::{ExceptHandler, Expr, Stmt};
use ruff_text_size::Ranged;

const DEPRECATED_OPERATORS: &[&str] = &["inselect", "not inselect"];

pub const EXCEPT_PASS: Rule = Rule {
    code: "W8138",
    name: "except-pass",
    summary: "An `except` block only contains `pass`.",
    doc: r#"
## What it does

Reports `except` blocks whose only statement is `pass` and that do not bind
the exception (`except ... as e`).

## Why is this bad?

The error disappears without a trace, which makes failures in production
impossible to diagnose. Log it, or catch only the exception you expect.

## Example

```python
try:
    value = int(text)
except Exception:
    pass
```

Use instead:

```python
try:
    value = int(text)
except ValueError:
    _logger.debug("Not a number: %s", text)
```
"#,
    check: Check::Python(check_except_pass),
    min_odoo: None,
    max_odoo: None,
};

pub const DEPRECATED_INSELECT_OPERATOR: Rule = Rule {
    code: "E8149",
    name: "deprecated-inselect-operator",
    summary: "A domain uses the deprecated `inselect` operator.",
    doc: r#"
## What it does

Reports the strings `inselect` and `not inselect`, the domain operators that
take raw SQL.

## Why is this bad?

Odoo 18.0 removed them. Use `in` or `not in` with an `SQL` object instead. See
[odoo/odoo#171371](https://github.com/odoo/odoo/pull/171371).
"#,
    check: Check::Python(check_inselect),
    min_odoo: Some(OdooVersion::new(18, 0)),
    max_odoo: None,
};

pub const USE_VIM_COMMENT: Rule = Rule {
    code: "W8202",
    name: "use-vim-comment",
    summary: "A file contains a vim modeline comment.",
    doc: r#"
## What it does

Reports comments starting with `vim:`.

## Why is this bad?

Editor settings belong in the developer's own configuration, or in a shared
`.editorconfig`, not in every source file.
"#,
    check: Check::Python(check_vim_comment),
    min_odoo: None,
    max_odoo: None,
};

fn check_except_pass(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::ExceptHandler(ExceptHandler::ExceptHandler(handler)) = node else {
            return;
        };
        if handler.name.is_none() && matches!(handler.body.as_slice(), [Stmt::Pass(_)]) {
            reporter.report(
                &EXCEPT_PASS,
                handler.start(),
                "pass into block except. If you really need to use the pass consider logging that exception",
            );
        }
    });
}

fn check_inselect(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::StringLiteral(string)) = node else {
            return;
        };
        let value = string.value.to_str();
        if DEPRECATED_OPERATORS.contains(&value.to_lowercase().as_str()) {
            reporter.report(
                &DEPRECATED_INSELECT_OPERATOR,
                string.start(),
                format!(
                    "The domain operator {} is deprecated in Odoo 18.0+. Use 'in' SQL or 'not in' SQL instead. More info at https://github.com/odoo/odoo/pull/171371",
                    repr_str(value)
                ),
            );
        }
    });
}

fn check_vim_comment(ctx: &PythonContext, reporter: &mut Reporter) {
    for token in ctx.parsed.tokens().iter().filter(|t| t.kind() == TokenKind::Comment) {
        let comment = &ctx.source[token.range()];
        if comment.trim_matches(['#', ' ']).to_lowercase().starts_with("vim:") {
            reporter.report(&USE_VIM_COMMENT, token.start(), "Use of vim comment");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::run_python_rule;

    #[test]
    fn except_pass() {
        let src = "try:\n    x()\nexcept Exception:\n    pass\nexcept ValueError as e:\n    pass\nexcept KeyError:\n    log()\n    pass\n";
        let v = run_python_rule(&EXCEPT_PASS, src);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 3);
    }

    #[test]
    fn inselect() {
        let src = "domain = [('id', 'inselect', q), ('id', 'NOT INSELECT', q), ('id', 'in', q)]\n";
        let v = run_python_rule(&DEPRECATED_INSELECT_OPERATOR, src);
        assert_eq!(v.len(), 2);
        assert!(v[1]
            .message
            .starts_with("The domain operator 'NOT INSELECT' is deprecated"));
    }

    #[test]
    fn vim_comment() {
        let src = "# vim:fileencoding=utf-8\n# -*- coding: utf-8 -*-\nx = 1  # Vim: set ts=4\n";
        assert_eq!(run_python_rule(&USE_VIM_COMMENT, src).len(), 2);
    }
}
