//! Python rules ported from pylint-odoo, with its codes, names and messages.

use crate::semantic::func_lib;
use crate::visit::{walk, Node};
use ruff_python_ast::{Expr, ExprCall, Stmt, StmtAssign, StmtClassDef, StmtFunctionDef};
use ruff_text_size::{Ranged, TextSize};

pub mod calls;
pub mod fields;
pub mod format_strings;
pub mod imports;
pub mod inherit;
pub mod methods;
pub mod misc;
pub mod models;
pub mod sql;
pub mod translations;

/// Every class in the file, nested ones included.
pub(crate) fn classes(suite: &[Stmt]) -> Vec<&StmtClassDef> {
    let mut found = Vec::new();
    walk(suite, |node, _| {
        if let Node::Stmt(Stmt::ClassDef(class)) = node {
            found.push(class);
        }
    });
    found
}

/// `name = fields.X(...)` assignments directly in a class body.
pub(crate) fn field_definitions(class: &StmtClassDef) -> impl Iterator<Item = (&StmtAssign, &ExprCall)> {
    class.body.iter().filter_map(|stmt| {
        let assign = stmt.as_assign_stmt()?;
        let call = assign.value.as_call_expr()?;
        (func_lib(&call.func) == "fields").then_some((assign, call))
    })
}

/// Methods defined directly in a class body.
pub(crate) fn methods(class: &StmtClassDef) -> impl Iterator<Item = &StmtFunctionDef> {
    class.body.iter().filter_map(Stmt::as_function_def_stmt)
}

/// Whether the class body assigns `name` (an entry in astroid's `locals`).
pub(crate) fn class_assigns(class: &StmtClassDef, name: &str) -> bool {
    class.body.iter().any(|stmt| match stmt {
        Stmt::Assign(assign) => assign
            .targets
            .iter()
            .any(|t| t.as_name_expr().is_some_and(|n| n.id.as_str() == name)),
        Stmt::AnnAssign(assign) => assign.target.as_name_expr().is_some_and(|n| n.id.as_str() == name),
        _ => false,
    })
}

/// Position of the `def` (or `async`) keyword, where pylint reports
/// function messages; ruff's range starts at the first decorator.
pub(crate) fn def_offset(source: &str, function: &StmtFunctionDef) -> TextSize {
    let name_start = function.name.start().to_usize();
    let before = &source[..name_start];
    let def = before.rfind("def").unwrap_or(name_start);
    let start = match before[..def].trim_end().strip_suffix("async") {
        Some(rest) if function.is_async => rest.len(),
        _ => def,
    };
    TextSize::try_from(start).unwrap_or(function.name.start())
}

/// pylint-odoo's `_get_str_value`: the value of a string literal, or of an
/// f-string with `{}` for every interpolation.
pub(crate) fn str_value(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        Expr::FString(f) => {
            let mut out = String::new();
            for part in &f.value {
                match part {
                    ruff_python_ast::FStringPartRef::Literal(literal) => out.push_str(&literal.value),
                    ruff_python_ast::FStringPartRef::FString(fstring) => {
                        for element in &fstring.elements {
                            match element {
                                ruff_python_ast::InterpolatedStringElement::Literal(literal) => {
                                    out.push_str(&literal.value)
                                }
                                ruff_python_ast::InterpolatedStringElement::Interpolation(_) => out.push_str("{}"),
                            }
                        }
                    }
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// Every call expression inside `body`, nested functions and classes included
/// (astroid's `nodes_of_class(Call)`).
pub(crate) fn calls_in(body: &[Stmt]) -> Vec<&ExprCall> {
    let mut calls = Vec::new();
    walk(body, |node, _| {
        if let Node::Expr(Expr::Call(call)) = node {
            calls.push(call);
        }
    });
    calls
}

/// The source text of a node, for messages that quote code.
pub(crate) fn source_of<'s>(source: &'s str, node: &impl Ranged) -> &'s str {
    &source[node.range()]
}
