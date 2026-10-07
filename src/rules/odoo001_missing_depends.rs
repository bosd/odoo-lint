//! ODOO001: compute method without `@api.depends`.
//!
//! Flags methods referenced as `compute=` by a field in the same class that are
//! not decorated with `@api.depends(...)` or `@api.depends_context(...)`.
//! Without dependencies Odoo never invalidates the cached value of a stored
//! computed field, and non-stored fields are recomputed on every access.

use crate::diagnostics::Violation;
use ruff_python_ast::statement_visitor::{walk_stmt, StatementVisitor};
use ruff_python_ast::{Decorator, Expr, Stmt, StmtClassDef};
use ruff_python_parser::parse_module;
use ruff_source_file::LineIndex;
use ruff_text_size::Ranged;
use std::collections::BTreeMap;

pub const CODE: &str = "ODOO001";

pub fn check_python_file(file_path: &str, content: &str) -> Vec<Violation> {
    let Ok(parsed) = parse_module(content) else {
        return vec![];
    };
    let mut collector = ClassCollector::default();
    collector.visit_body(parsed.suite());

    let line_index = LineIndex::from_source_text(content);
    let mut violations = Vec::new();
    for class in collector.classes {
        check_class(class, file_path, &line_index, &mut violations);
    }
    violations
}

/// Collects every class definition, including nested ones and those inside
/// `if`/`try` blocks.
#[derive(Default)]
struct ClassCollector<'a> {
    classes: Vec<&'a StmtClassDef>,
}

impl<'a> StatementVisitor<'a> for ClassCollector<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if let Stmt::ClassDef(class) = stmt {
            self.classes.push(class);
        }
        walk_stmt(self, stmt);
    }
}

fn check_class(class: &StmtClassDef, file_path: &str, line_index: &LineIndex, out: &mut Vec<Violation>) {
    // compute method name -> fields that use it
    let mut computes: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for stmt in &class.body {
        let Stmt::Assign(assign) = stmt else { continue };
        let Some(method) = compute_method(&assign.value) else {
            continue;
        };
        for target in &assign.targets {
            if let Expr::Name(name) = target {
                computes.entry(method).or_default().push(name.id.as_str());
            }
        }
    }
    if computes.is_empty() {
        return;
    }

    for stmt in &class.body {
        let Stmt::FunctionDef(func) = stmt else { continue };
        let Some(fields) = computes.get(func.name.as_str()) else {
            continue;
        };
        if func.decorator_list.iter().any(is_depends_decorator) {
            continue;
        }
        out.push(Violation {
            file_path: file_path.to_string(),
            line: line_index.line_index(func.name.start()).get(),
            rule_code: CODE,
            message: format!(
                "Compute method '{}' (field(s): {}) is missing @api.depends",
                func.name.as_str(),
                fields.join(", ")
            ),
        });
    }
}

/// Returns the compute method name if `value` is `fields.X(..., compute=...)`.
/// Accepts both `compute='_compute_x'` and `compute=_compute_x`.
fn compute_method(value: &Expr) -> Option<&str> {
    let Expr::Call(call) = value else { return None };
    let Expr::Attribute(attr) = call.func.as_ref() else {
        return None;
    };
    if !matches!(attr.value.as_ref(), Expr::Name(n) if n.id.as_str() == "fields") {
        return None;
    }
    let keyword = call.arguments.find_keyword("compute")?;
    match &keyword.value {
        Expr::StringLiteral(s) => Some(s.value.to_str()),
        Expr::Name(n) => Some(n.id.as_str()),
        _ => None,
    }
}

/// Matches `@api.depends(...)`, `@api.depends_context(...)` and the bare
/// `@depends(...)` / `@depends_context(...)` forms (`from odoo.api import depends`).
fn is_depends_decorator(decorator: &Decorator) -> bool {
    let expr = match &decorator.expression {
        Expr::Call(call) => call.func.as_ref(),
        other => other,
    };
    let name = match expr {
        Expr::Attribute(attr) if matches!(attr.value.as_ref(), Expr::Name(n) if n.id.as_str() == "api") => {
            attr.attr.as_str()
        }
        Expr::Name(n) => n.id.as_str(),
        _ => return false,
    };
    matches!(name, "depends" | "depends_context")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(src: &str) -> Vec<Violation> {
        check_python_file("test.py", src)
    }

    #[test]
    fn flags_compute_without_depends() {
        let src = r#"
from odoo import api, fields, models

class SaleOrder(models.Model):
    _inherit = "sale.order"

    total = fields.Float(compute="_compute_total", store=True)

    def _compute_total(self):
        for rec in self:
            rec.total = 0
"#;
        let v = check(src);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule_code, "ODOO001");
        assert_eq!(v[0].line, 9);
        assert!(v[0].message.contains("_compute_total"));
    }

    #[test]
    fn accepts_depends_and_depends_context() {
        let src = r#"
class A(models.Model):
    a = fields.Float(compute="_compute_a")
    b = fields.Char(compute="_compute_b")

    @api.depends("x")
    def _compute_a(self):
        pass

    @api.depends_context("uid")
    def _compute_b(self):
        pass
"#;
        assert!(check(src).is_empty());
    }

    #[test]
    fn shared_method_reported_once_with_all_fields() {
        let src = r#"
class A(models.Model):
    a = fields.Float(compute="_compute_ab")
    b = fields.Float(compute="_compute_ab")

    def _compute_ab(self):
        pass
"#;
        let v = check(src);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("a, b"));
    }

    #[test]
    fn ignores_methods_not_used_as_compute_and_invalid_python() {
        let src = r#"
class A(models.Model):
    name = fields.Char()

    def _compute_unused(self):
        pass
"#;
        assert!(check(src).is_empty());
        assert!(check("def broken(:\n").is_empty());
    }
}
