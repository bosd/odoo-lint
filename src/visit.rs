//! An AST walk that knows the enclosing scopes of every node, for rules that
//! depend on "inside a method of a model" and similar context.

use ruff_python_ast::visitor::{self, Visitor};
use ruff_python_ast::{ExceptHandler, Expr, ExprLambda, Stmt, StmtClassDef, StmtFunctionDef};

/// A scope as Python (and astroid) sees it.
#[derive(Debug, Clone, Copy)]
pub enum Scope<'a> {
    Class(&'a StmtClassDef),
    Function(&'a StmtFunctionDef),
    Lambda(&'a ExprLambda),
    /// List, set and dict comprehensions and generator expressions.
    Comprehension,
}

/// A node handed to the callback of [`walk`].
#[derive(Debug, Clone, Copy)]
pub enum Node<'a> {
    Stmt(&'a Stmt),
    Expr(&'a Expr),
    ExceptHandler(&'a ExceptHandler),
}

/// Calls `f` for every statement, expression and except handler, with the
/// scopes enclosing it (outermost first). A class or function itself is
/// reported with the scopes around it, not including itself.
pub fn walk<'a>(suite: &'a [Stmt], f: impl FnMut(Node<'a>, &[Scope<'a>])) {
    let mut walker = Walker { scopes: Vec::new(), f };
    walker.visit_body(suite);
}

struct Walker<'a, F> {
    scopes: Vec<Scope<'a>>,
    f: F,
}

impl<'a, F: FnMut(Node<'a>, &[Scope<'a>])> Visitor<'a> for Walker<'a, F> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        (self.f)(Node::Stmt(stmt), &self.scopes);
        let scope = match stmt {
            Stmt::ClassDef(class) => Some(Scope::Class(class)),
            Stmt::FunctionDef(function) => Some(Scope::Function(function)),
            _ => None,
        };
        match scope {
            Some(scope) => {
                self.scopes.push(scope);
                visitor::walk_stmt(self, stmt);
                self.scopes.pop();
            }
            None => visitor::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        (self.f)(Node::Expr(expr), &self.scopes);
        let scope = match expr {
            Expr::Lambda(lambda) => Some(Scope::Lambda(lambda)),
            Expr::ListComp(_) | Expr::SetComp(_) | Expr::DictComp(_) | Expr::Generator(_) => Some(Scope::Comprehension),
            _ => None,
        };
        match scope {
            Some(scope) => {
                self.scopes.push(scope);
                visitor::walk_expr(self, expr);
                self.scopes.pop();
            }
            None => visitor::walk_expr(self, expr),
        }
    }

    fn visit_except_handler(&mut self, handler: &'a ExceptHandler) {
        (self.f)(Node::ExceptHandler(handler), &self.scopes);
        visitor::walk_except_handler(self, handler);
    }
}

/// Innermost scope that is a frame in astroid's sense: function, lambda or
/// class (comprehensions are skipped). `None` at module level.
pub fn frame<'a>(scopes: &[Scope<'a>]) -> Option<Scope<'a>> {
    scopes
        .iter()
        .rev()
        .find(|s| !matches!(s, Scope::Comprehension))
        .copied()
}

/// The function `scopes` ends in, if that function is a method: defined
/// directly in a class body.
pub fn current_method<'a>(scopes: &[Scope<'a>]) -> Option<(&'a StmtClassDef, &'a StmtFunctionDef)> {
    match scopes {
        [.., Scope::Class(class), Scope::Function(function)] => Some((class, function)),
        _ => None,
    }
}

/// Innermost enclosing class, if any.
pub fn enclosing_class<'a>(scopes: &[Scope<'a>]) -> Option<&'a StmtClassDef> {
    scopes.iter().rev().find_map(|s| match s {
        Scope::Class(class) => Some(*class),
        _ => None,
    })
}

/// Innermost enclosing function (not lambda), if any.
pub fn enclosing_function<'a>(scopes: &[Scope<'a>]) -> Option<&'a StmtFunctionDef> {
    scopes.iter().rev().find_map(|s| match s {
        Scope::Function(function) => Some(*function),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruff_python_parser::parse_module;

    #[test]
    fn reports_scopes() {
        let src = "class A:\n    def m(self):\n        x = [y for y in z]\n        f = lambda: 1\n";
        let parsed = parse_module(src).unwrap();
        let mut seen = Vec::new();
        walk(parsed.suite(), |node, scopes| {
            if let Node::Expr(Expr::Name(name)) = node {
                let kinds: Vec<&str> = scopes
                    .iter()
                    .map(|s| match s {
                        Scope::Class(_) => "class",
                        Scope::Function(_) => "function",
                        Scope::Lambda(_) => "lambda",
                        Scope::Comprehension => "comprehension",
                    })
                    .collect();
                seen.push((name.id.to_string(), kinds.join(">")));
            }
        });
        assert!(seen.contains(&("x".into(), "class>function".into())));
        assert!(seen.contains(&("y".into(), "class>function>comprehension".into())));
    }

    #[test]
    fn method_detection() {
        let src = "class A:\n    def m(self):\n        def inner():\n            pass\n";
        let parsed = parse_module(src).unwrap();
        let mut methods = Vec::new();
        walk(parsed.suite(), |node, scopes| {
            if let Node::Stmt(Stmt::Pass(_)) = node {
                methods.push(current_method(scopes).is_some());
            }
        });
        assert_eq!(methods, vec![false]);
    }
}
