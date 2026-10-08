//! Rules that need to know a class is an Odoo model.

use super::{calls_in, classes, field_definitions, source_of, str_value};
use crate::checker::{PythonContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::rules::{Check, Rule};
use crate::semantic::{func_lib, func_name};
use crate::visit::{enclosing_class, walk, Node};
use ruff_python_ast::visitor::{self, Visitor};
use ruff_python_ast::{Expr, Stmt, StmtFunctionDef};
use ruff_text_size::{Ranged, TextSize};
use std::path::Path;

/// Roots of an expression that make `.write()` a write on the computed records.
const BROWSABLE_ROOTS: &[&str] = &[
    "self.browse",
    "self.copy",
    "self.env",
    "self.filtered",
    "self.filtered_domain",
    "self.mapped",
    "self.search",
    "self.sorted",
    "self",
];

pub const NO_WRITE_IN_COMPUTE: Rule = Rule {
    code: "E8135",
    name: "no-write-in-compute",
    summary: "A compute method calls `write()`.",
    doc: r#"
## What it does

Reports `write()` on the computed records, or records derived from them,
inside a compute method of a model.

## Why is this bad?

`write()` inside a compute triggers a new write cycle, with its access checks,
recomputations and possibly infinite recursion. Assign the fields or use
`update()` instead.

## Example

```python
def _compute_total(self):
    for rec in self:
        rec.write({"total": sum(rec.line_ids.mapped("amount"))})
```

Use instead:

```python
def _compute_total(self):
    for rec in self:
        rec.total = sum(rec.line_ids.mapped("amount"))
```
"#,
    check: Check::Python(check_write_in_compute),
    min_odoo: None,
    max_odoo: None,
};

pub const NO_WIZARD_IN_MODELS: Rule = Rule {
    code: "C8113",
    name: "no-wizard-in-models",
    summary: "A wizard (`TransientModel`) is defined in the `models` folder.",
    doc: r#"
## What it does

Reports `TransientModel` classes in files of a folder whose name starts with
`model`. Settings (`res.config.settings`) are allowed there.

## Why is this bad?

The OCA module structure keeps wizards in `wizards/`, so they are easy to
find. See the
[complete structure](https://github.com/OCA/odoo-community.org/blob/master/website/Contribution/CONTRIBUTING.rst#complete-structure).
"#,
    check: Check::Python(check_wizard_in_models),
    min_odoo: None,
    max_odoo: None,
};

pub const DEPRECATED_SELF_CR: Rule = Rule {
    code: "W8165",
    name: "deprecated-self-cr",
    summary: "`self._cr` is used instead of `self.env.cr`.",
    doc: r#"
## What it does

Reports `self._cr` in model classes.

## Why is this bad?

Odoo 19.0 deprecated the `_cr` shortcut; `self.env.cr` is the supported way
to reach the cursor.

## Fix safety

Safe: `self._cr` becomes `self.env.cr`, the same cursor.
"#,
    check: Check::Python(check_self_cr),
    min_odoo: Some(OdooVersion::new(19, 0)),
    max_odoo: None,
};

/// How a name is bound inside a function, for tracing where records come from.
enum Binding<'a> {
    Value(&'a Expr),
    LoopOver(&'a Expr),
    Other,
}

struct BindingFinder<'a> {
    name: &'a str,
    before: TextSize,
    found: Option<Binding<'a>>,
}

impl<'a> Visitor<'a> for BindingFinder<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if stmt.start() >= self.before {
            return;
        }
        let is_target = |e: &Expr| matches!(e, Expr::Name(n) if n.id.as_str() == self.name);
        match stmt {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,
            Stmt::Assign(a) if a.targets.len() == 1 && is_target(&a.targets[0]) => {
                self.found = Some(Binding::Value(&a.value))
            }
            Stmt::Assign(a) if a.targets.iter().any(is_target) => self.found = Some(Binding::Other),
            Stmt::AugAssign(a) if is_target(&a.target) => self.found = Some(Binding::Other),
            Stmt::AnnAssign(a) if is_target(&a.target) => self.found = Some(Binding::Other),
            Stmt::For(f) if is_target(&f.target) => self.found = Some(Binding::LoopOver(&f.iter)),
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

/// pylint-odoo's `_get_root_method_assignation`: follows calls, subscripts,
/// attributes and assignments back to `self` and returns the last attribute
/// chain seen on the way, such as `self.search` or `self`.
fn root_of<'a>(
    ctx: &PythonContext<'a>,
    function: &'a StmtFunctionDef,
    expr: &'a Expr,
    lib: Option<String>,
    depth: usize,
) -> Option<String> {
    if depth > 32 {
        return lib;
    }
    match expr {
        Expr::Call(call) => root_of(ctx, function, &call.func, lib, depth + 1),
        Expr::Subscript(sub) => root_of(ctx, function, &sub.value, lib, depth + 1),
        Expr::Attribute(attr) => root_of(
            ctx,
            function,
            &attr.value,
            Some(source_of(ctx.source, attr).to_string()),
            depth + 1,
        ),
        Expr::Name(name) if name.id.as_str() == "self" => lib,
        Expr::Name(name) => {
            let mut finder = BindingFinder {
                name: name.id.as_str(),
                before: name.start(),
                found: None,
            };
            finder.visit_body(&function.body);
            match finder.found {
                Some(Binding::Value(value)) => root_of(ctx, function, value, lib, depth + 1),
                Some(Binding::LoopOver(iter)) => root_of(
                    ctx,
                    function,
                    iter,
                    Some(source_of(ctx.source, iter).to_string()),
                    depth + 1,
                ),
                _ => lib,
            }
        }
        _ => lib,
    }
}

/// Every function in `body`, nested ones included.
fn functions_in(body: &[Stmt]) -> Vec<&StmtFunctionDef> {
    let mut found = Vec::new();
    walk(body, |node, _| {
        if let Node::Stmt(Stmt::FunctionDef(f)) = node {
            found.push(f);
        }
    });
    found
}

fn check_write_in_compute(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        if ctx.semantic.odoo_model_kind(class).is_none() {
            continue;
        }
        let computes: Vec<String> = field_definitions(class)
            .flat_map(|(_, call)| call.arguments.keywords.iter())
            .filter(|kw| kw.arg.as_ref().is_some_and(|a| a.as_str() == "compute"))
            .filter_map(|kw| match &kw.value {
                Expr::Name(name) => Some(name.id.to_string()),
                value => str_value(value),
            })
            .collect();
        if computes.is_empty() {
            continue;
        }
        for function in functions_in(&class.body) {
            if !computes.iter().any(|c| c == function.name.as_str()) {
                continue;
            }
            for call in calls_in(&function.body) {
                if func_name(&call.func) != "write" {
                    continue;
                }
                let on_records = func_lib(&call.func) == "self" || {
                    let root = root_of(ctx, function, &call.func, None, 0).unwrap_or_default();
                    BROWSABLE_ROOTS.contains(&root.as_str()) || root.starts_with("self.with_")
                };
                if on_records {
                    reporter.report(
                        &NO_WRITE_IN_COMPUTE,
                        call.start(),
                        "Compute method calling `write`. Use `update` instead.",
                    );
                }
            }
        }
    }
}

fn check_wizard_in_models(ctx: &PythonContext, reporter: &mut Reporter) {
    let in_models_folder = Path::new(ctx.file_path)
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n.to_string_lossy().starts_with("model"));
    if !in_models_folder {
        return;
    }
    for class in classes(ctx.parsed.suite()) {
        let Some(("TransientModel", base)) = ctx.semantic.odoo_model_kind(class) else {
            continue;
        };
        let inherit = class
            .body
            .iter()
            .filter_map(Stmt::as_assign_stmt)
            .filter(|a| matches!(a.targets.first(), Some(Expr::Name(n)) if n.id.as_str() == "_inherit"))
            .filter_map(|a| a.value.as_string_literal_expr().map(|s| s.value.to_str()))
            .last()
            .unwrap_or("");
        if !inherit.starts_with("res.config") {
            reporter.report(
                &NO_WIZARD_IN_MODELS,
                base.start(),
                "No wizard class for model directory. See the complete structure https://github.com/OCA/odoo-community.org/blob/master/website/Contribution/CONTRIBUTING.rst#complete-structure",
            );
        }
    }
}

fn check_self_cr(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, scopes| {
        let Node::Expr(Expr::Attribute(attr)) = node else {
            return;
        };
        let is_self_cr =
            attr.attr.as_str() == "_cr" && matches!(&*attr.value, Expr::Name(n) if n.id.as_str() == "self");
        let in_model = enclosing_class(scopes).is_some_and(|c| ctx.semantic.odoo_model_kind(c).is_some());
        if is_self_cr && in_model {
            reporter
                .report(
                    &DEPRECATED_SELF_CR,
                    attr.start(),
                    "Use \"self.env.cr\" instead of \"self._cr\" (deprecated since 19.0)",
                )
                .fix = Some(Fix::safe(
                "Use `self.env.cr`",
                vec![Edit::replace(
                    attr.attr.start().to_usize(),
                    attr.attr.end().to_usize(),
                    "env.cr",
                )],
            ));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::{run_python_rule, run_python_rule_with};
    use crate::settings::Settings;

    const HEADER: &str = "from odoo import fields, models\n\n\nclass A(models.Model):\n    _name = 'a'\n    total = fields.Float(compute='_compute_total')\n";

    #[test]
    fn write_in_compute() {
        let src = format!(
            "{HEADER}\n    def _compute_total(self):\n        self.write({{}})\n        for rec in self:\n            rec.write({{}})\n            rec.line_ids.write({{}})\n        recs = self.search([])\n        recs.write({{}})\n        self.with_context(a=1).write({{}})\n        other = self.env['b'].browse(1)\n        other.write({{}})\n        partner.write({{}})\n        self[0].write({{}})\n\n    def other(self):\n        self.write({{}})\n"
        );
        assert_eq!(run_python_rule(&NO_WRITE_IN_COMPUTE, &src).len(), 6);
    }

    #[test]
    fn wizard_in_models() {
        let src = "from odoo import models\nclass W(models.TransientModel):\n    _name = 'w'\nclass S(models.TransientModel):\n    _inherit = 'res.config.settings'\n";
        let settings = Settings::default();
        assert_eq!(
            run_python_rule_with(&NO_WIZARD_IN_MODELS, src, "m/models/w.py", None, &settings).len(),
            1
        );
        assert_eq!(
            run_python_rule_with(&NO_WIZARD_IN_MODELS, src, "m/wizards/w.py", None, &settings).len(),
            0
        );
    }

    #[test]
    fn self_cr() {
        let src = "from odoo import models\nclass A(models.Model):\n    def m(self):\n        self._cr.execute('x')\nclass B:\n    def m(self):\n        self._cr.execute('x')\n";
        assert_eq!(run_python_rule(&DEPRECATED_SELF_CR, src).len(), 1);
    }
}
