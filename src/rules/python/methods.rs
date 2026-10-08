//! Rules on methods of classes: deprecated names, `super()` usage and
//! `unlink()`.

use super::{calls_in, class_assigns, classes, def_offset, methods};
use crate::checker::{PythonContext, Reporter};
use crate::config::list_or;
use crate::odoo_version::OdooVersion;
use crate::rules::{Check, Rule};
use crate::semantic::func_name;
use crate::visit::{current_method, walk, Node, Scope};
use ruff_python_ast::visitor::{self, Visitor};
use ruff_python_ast::{Expr, Stmt, StmtFunctionDef};
use ruff_text_size::Ranged;

const DEFAULT_METHODS_REQUIRED_SUPER: &[&str] = &[
    "copy",
    "create",
    "default_get",
    "read",
    "setUp",
    "setUpClass",
    "tearDown",
    "tearDownClass",
    "unlink",
    "write",
];
const DEFAULT_NO_MISSING_RETURN: &[&str] = &[
    "__init__",
    "_register_hook",
    "setUp",
    "setUpClass",
    "tearDown",
    "tearDownClass",
];
const DEFAULT_DEPRECATED_MODEL_METHODS: &[(&str, &[&str])] = &[("16.0", &["fields_view_get"])];

pub const DEPRECATED_NAME_GET: Rule = Rule {
    code: "E8146",
    name: "deprecated-name-get",
    summary: "A model defines `name_get`, replaced by `_compute_display_name`.",
    doc: r#"
## What it does

Reports methods named `name_get`.

## Why is this bad?

Odoo 17.0 replaced `name_get` by the computed field `display_name`. An
overridden `name_get` is no longer called, so the custom display name silently
disappears. See [odoo/odoo#122085](https://github.com/odoo/odoo/pull/122085).

## Example

```python
def name_get(self):
    return [(rec.id, f"[{rec.code}] {rec.name}") for rec in self]
```

Use instead:

```python
def _compute_display_name(self):
    for rec in self:
        rec.display_name = f"[{rec.code}] {rec.name}"
```
"#,
    check: Check::Python(check_name_get),
    min_odoo: Some(OdooVersion::new(17, 0)),
    max_odoo: None,
};

pub const DEPRECATED_ODOO_MODEL_METHOD: Rule = Rule {
    code: "W8160",
    name: "deprecated-odoo-model-method",
    summary: "A model overrides a method Odoo deprecated.",
    doc: r#"
## What it does

Reports model methods that Odoo deprecated in the target version or earlier.
By default that is `fields_view_get`, deprecated in 16.0 in favour of
`get_views` and `_get_view`.

## Why is this bad?

The deprecated method is no longer called, or will be removed, so the
override silently stops working.

## Configuration

```toml
[tool.odoo-lint.rules.deprecated-odoo-model-method]
methods = { "16.0" = ["fields_view_get"] }
```
"#,
    check: Check::Python(check_deprecated_model_methods),
    min_odoo: None,
    max_odoo: None,
};

pub const METHOD_REQUIRED_SUPER: Rule = Rule {
    code: "W8106",
    name: "method-required-super",
    summary: "An override of a core method does not call `super()`.",
    doc: r#"
## What it does

Reports overrides of methods such as `create`, `write`, `unlink`, `copy` and
`setUp` that never call `super()`.

## Why is this bad?

The override replaces the method of every module below it in the inheritance
chain, including the ORM's own implementation.

## Configuration

```toml
[tool.odoo-lint.rules.method-required-super]
methods = ["copy", "create", "default_get", "read", "unlink", "write"]
```
"#,
    check: Check::Python(check_required_super),
    min_odoo: None,
    max_odoo: None,
};

pub const PROHIBITED_METHOD_OVERRIDE: Rule = Rule {
    code: "W8107",
    name: "prohibited-method-override",
    summary: "A method that must not be overridden is overridden.",
    doc: r#"
## What it does

Reports overrides (methods calling `super()` on themselves) of the methods in
the configured list. The list is empty by default.

## Configuration

```toml
[tool.odoo-lint.rules.prohibited-method-override]
methods = ["_compute_amount"]
```
"#,
    check: Check::Python(check_prohibited_override),
    min_odoo: None,
    max_odoo: None,
};

pub const MISSING_RETURN: Rule = Rule {
    code: "W8110",
    name: "missing-return",
    summary: "A method calls `super()` but does not return anything.",
    doc: r#"
## What it does

Reports methods that call `super()` but have no `return`.

## Why is this bad?

The value the parent method returns, such as the created records of
`create()`, is lost for every caller.

## Example

```python
def write(self, vals):
    super().write(vals)
```

Use instead:

```python
def write(self, vals):
    return super().write(vals)
```

## Configuration

```toml
[tool.odoo-lint.rules.missing-return]
ignore-methods = ["__init__", "_register_hook", "setUp", "setUpClass", "tearDown", "tearDownClass"]
```
"#,
    check: Check::Python(check_missing_return),
    min_odoo: None,
    max_odoo: None,
};

pub const NO_RAISE_UNLINK: Rule = Rule {
    code: "E8140",
    name: "no-raise-unlink",
    summary: "`unlink()` raises an exception.",
    doc: r#"
## What it does

Reports `raise` statements inside the `unlink()` method of a model.

## Why is this bad?

Since Odoo 15.0, checks that prevent deleting records belong in a method
decorated with `@api.ondelete`. That method is skipped when the module is
uninstalled, so uninstalling does not fail on your check.

## Example

```python
def unlink(self):
    if self.filtered("posted"):
        raise UserError(_("Cannot delete posted entries."))
    return super().unlink()
```

Use instead:

```python
@api.ondelete(at_uninstall=False)
def _unlink_except_posted(self):
    if self.filtered("posted"):
        raise UserError(_("Cannot delete posted entries."))
```
"#,
    check: Check::Python(check_raise_unlink),
    min_odoo: Some(OdooVersion::new(15, 0)),
    max_odoo: None,
};

pub const SUPER_METHOD_MISMATCH: Rule = Rule {
    code: "W8164",
    name: "super-method-mismatch",
    summary: "A method calls `super()` on a different method.",
    doc: r#"
## What it does

Reports `super().other()` inside a method that is not `other`. Methods with
`queue` or `cache` in their name are skipped.

## Why is this bad?

It is usually a copy-paste error: the override skips its own parent method
and calls another one instead.

## Example

```python
def write(self, vals):
    return super().create(vals)
```
"#,
    check: Check::Python(check_super_mismatch),
    min_odoo: None,
    max_odoo: None,
};

fn check_name_get(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        for method in methods(class).filter(|m| m.name.as_str() == "name_get") {
            reporter.report(
                &DEPRECATED_NAME_GET,
                def_offset(ctx.source, method),
                "'name_get' is deprecated. Use '_compute_display_name' instead. More info at https://github.com/odoo/odoo/pull/122085.",
            );
        }
    }
}

fn check_deprecated_model_methods(ctx: &PythonContext, reporter: &mut Reporter) {
    let configured = ctx
        .settings
        .config
        .rules()
        .deprecated_odoo_model_method
        .as_ref()
        .and_then(|c| c.methods.clone());
    let by_version: Vec<(String, Vec<String>)> = match configured {
        Some(map) => map.into_iter().collect(),
        None => DEFAULT_DEPRECATED_MODEL_METHODS
            .iter()
            .map(|(v, m)| (v.to_string(), m.iter().map(|s| s.to_string()).collect()))
            .collect(),
    };
    let deprecated: Vec<String> = by_version
        .into_iter()
        .filter(|(version, _)| {
            version
                .parse::<OdooVersion>()
                .is_ok_and(|v| v <= ctx.settings.target_version)
        })
        .flat_map(|(_, methods)| methods)
        .collect();
    for class in classes(ctx.parsed.suite()) {
        if ctx.semantic.odoo_model_kind(class).is_none() {
            continue;
        }
        for method in methods(class).filter(|m| deprecated.iter().any(|d| d == m.name.as_str())) {
            reporter.report(
                &DEPRECATED_ODOO_MODEL_METHOD,
                def_offset(ctx.source, method),
                format!(
                    "{} has been deprecated by Odoo. Please look for alternatives.",
                    method.name
                ),
            );
        }
    }
}

fn calls_super(method: &StmtFunctionDef) -> bool {
    calls_in(&method.body)
        .iter()
        .any(|call| matches!(&*call.func, Expr::Name(name) if name.id.as_str() == "super"))
}

fn check_required_super(ctx: &PythonContext, reporter: &mut Reporter) {
    let required = list_or(
        ctx.settings.config.rules().method_required_super.as_ref(),
        |c| c.methods.as_ref(),
        DEFAULT_METHODS_REQUIRED_SUPER,
    );
    for class in classes(ctx.parsed.suite()) {
        for method in methods(class) {
            if required.iter().any(|r| r == method.name.as_str()) && !calls_super(method) {
                reporter.report(
                    &METHOD_REQUIRED_SUPER,
                    def_offset(ctx.source, method),
                    format!("Missing `super` call in \"{}\" method.", method.name),
                );
            }
        }
    }
}

fn check_prohibited_override(ctx: &PythonContext, reporter: &mut Reporter) {
    let prohibited = list_or(
        ctx.settings.config.rules().prohibited_method_override.as_ref(),
        |c| c.methods.as_ref(),
        &[],
    );
    if prohibited.is_empty() {
        return;
    }
    for class in classes(ctx.parsed.suite()) {
        for method in methods(class) {
            let name = method.name.as_str();
            if !prohibited.iter().any(|p| p == name) {
                continue;
            }
            let mut overrides = false;
            walk(&method.body, |node, _| {
                if let Node::Expr(Expr::Attribute(attr)) = node {
                    if attr.attr.as_str() == name
                        && matches!(&*attr.value, Expr::Call(call) if func_name(&call.func) == "super")
                    {
                        overrides = true;
                    }
                }
            });
            if overrides {
                reporter.report(
                    &PROHIBITED_METHOD_OVERRIDE,
                    def_offset(ctx.source, method),
                    format!("Prohibited override of \"{name}\" method."),
                );
            }
        }
    }
}

/// Looks for `return` and `yield` in a function body, without descending into
/// nested functions, classes or lambdas.
#[derive(Default)]
struct ReturnFinder {
    has_return: bool,
    is_generator: bool,
}

impl<'a> Visitor<'a> for ReturnFinder {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            Stmt::Return(_) => {
                self.has_return = true;
                visitor::walk_stmt(self, stmt);
            }
            _ => visitor::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_) => {}
            Expr::Yield(_) | Expr::YieldFrom(_) => {
                self.is_generator = true;
                visitor::walk_expr(self, expr);
            }
            _ => visitor::walk_expr(self, expr),
        }
    }
}

fn check_missing_return(ctx: &PythonContext, reporter: &mut Reporter) {
    let ignored = list_or(
        ctx.settings.config.rules().missing_return.as_ref(),
        |c| c.ignore_methods.as_ref(),
        DEFAULT_NO_MISSING_RETURN,
    );
    for class in classes(ctx.parsed.suite()) {
        for method in methods(class) {
            if ignored.iter().any(|i| i == method.name.as_str()) || !calls_super(method) {
                continue;
            }
            let mut finder = ReturnFinder::default();
            finder.visit_body(&method.body);
            if !finder.has_return && !finder.is_generator {
                reporter.report(
                    &MISSING_RETURN,
                    def_offset(ctx.source, method),
                    format!("Missing `return` (`super` is used) in method {}.", method.name),
                );
            }
        }
    }
}

fn check_raise_unlink(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, scopes| {
        let Node::Stmt(Stmt::Raise(raise)) = node else { return };
        let Some((class, function)) = current_method(scopes) else {
            return;
        };
        if function.name.as_str() == "unlink" && (class_assigns(class, "_name") || class_assigns(class, "_inherit")) {
            reporter.report(
                &NO_RAISE_UNLINK,
                raise.start(),
                "No exceptions should be raised inside unlink() functions",
            );
        }
    });
}

fn check_super_mismatch(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, scopes| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(attr) = &*call.func else { return };
        let Expr::Call(super_call) = &*attr.value else { return };
        if func_name(&super_call.func) != "super" {
            return;
        }
        // astroid's frame skips comprehensions; it must be a method.
        let frames: Vec<&Scope> = scopes.iter().filter(|s| !matches!(s, Scope::Comprehension)).collect();
        let [.., Scope::Class(_), Scope::Function(function)] = frames.as_slice() else {
            return;
        };
        let called = attr.attr.as_str();
        let defined = function.name.as_str();
        if called != defined && !defined.contains("queue") && !defined.contains("cache") {
            reporter.report(
                &SUPER_METHOD_MISMATCH,
                call.start(),
                format!("`super().{called}` mismatch but defined method is `{defined}`"),
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
    fn name_get() {
        let v = run_python_rule(
            &DEPRECATED_NAME_GET,
            "class A:\n    @api.multi\n    def name_get(self):\n        pass\n",
        );
        assert_eq!(v.len(), 1);
        assert_eq!((v[0].line, v[0].column), (3, 5));
        assert_eq!(count(&DEPRECATED_NAME_GET, "def name_get():\n    pass\n"), 0);
    }

    #[test]
    fn deprecated_model_method() {
        let src = "from odoo import models\nclass A(models.Model):\n    def fields_view_get(self):\n        pass\nclass B(object):\n    def fields_view_get(self):\n        pass\n";
        assert_eq!(count(&DEPRECATED_ODOO_MODEL_METHOD, src), 1);
    }

    #[test]
    fn required_super_and_missing_return() {
        let src = "class A:\n    def write(self, vals):\n        pass\n    def create(self, vals):\n        super().create(vals)\n    def copy(self):\n        return super().copy()\n    def unlink(self):\n        yield super().unlink()\n    def setUp(self):\n        super().setUp()\n";
        let required: Vec<_> = run_python_rule(&METHOD_REQUIRED_SUPER, src)
            .into_iter()
            .map(|v| v.message)
            .collect();
        assert_eq!(required, vec!["Missing `super` call in \"write\" method."]);
        let missing: Vec<_> = run_python_rule(&MISSING_RETURN, src)
            .into_iter()
            .map(|v| v.message)
            .collect();
        assert_eq!(missing, vec!["Missing `return` (`super` is used) in method create."]);
    }

    #[test]
    fn raise_in_unlink() {
        let src = "class A(models.Model):\n    _inherit = 'a'\n    def unlink(self):\n        if self:\n            raise UserError('x')\n        def inner():\n            raise ValueError\n        return super().unlink()\nclass B:\n    def unlink(self):\n        raise ValueError\n";
        let v = run_python_rule(&NO_RAISE_UNLINK, src);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 5);
    }

    #[test]
    fn super_mismatch() {
        let src = "class A:\n    def write(self, vals):\n        super().create(vals)\n        super(A, self).write(vals)\n        [super().read() for x in y]\n    def _queue_job(self):\n        super().other()\ndef f():\n    super().x()\n";
        let v: Vec<_> = run_python_rule(&SUPER_METHOD_MISMATCH, src)
            .into_iter()
            .map(|v| v.message)
            .collect();
        assert_eq!(
            v,
            vec![
                "`super().create` mismatch but defined method is `write`",
                "`super().read` mismatch but defined method is `write`",
            ]
        );
    }
}
