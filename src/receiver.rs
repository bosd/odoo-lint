//! The model of a recordset expression in Python code, as far as the code
//! shows it: `self` in a model's method, `self.env["sale.order"]`, a
//! variable assigned or looped from one, a lambda in `filtered`. Go to
//! definition, find references and completion use it for attribute access
//! (`order.partner_id`).

use crate::index::class_models;
use crate::visit::{enclosing_class, enclosing_function, walk, Node, Scope};
use ruff_python_ast::{Expr, Stmt};
use ruff_text_size::{Ranged, TextRange};

/// What an expression holds: records of one of `models`, reached through the
/// fields `path` (`order.partner_id` is `sale.order` and `["partner_id"]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Records {
    pub models: Vec<String>,
    pub path: Vec<String>,
}

/// Methods of a recordset that return records of the same model.
const SAME_MODEL: &[&str] = &[
    "browse",
    "copy",
    "create",
    "exists",
    "filtered",
    "filtered_domain",
    "new",
    "search",
    "sorted",
    "sudo",
    "with_company",
    "with_context",
    "with_env",
    "with_prefetch",
    "with_user",
];

/// The functions and lambdas in a module, to find what a name holds.
pub(crate) struct Receivers<'a> {
    /// Lambdas passed to `filtered`, `sorted` or `mapped`, with the records
    /// they receive.
    lambdas: Vec<(TextRange, &'a Expr)>,
    /// Comprehensions: their range, the name each `for` binds and what it
    /// iterates.
    comprehensions: Vec<(TextRange, &'a str, &'a Expr)>,
}

impl<'a> Receivers<'a> {
    pub(crate) fn new(suite: &'a [Stmt]) -> Self {
        let mut lambdas = Vec::new();
        let mut comprehensions = Vec::new();
        walk(suite, |node, _| {
            let Node::Expr(expr) = node else { return };
            match expr {
                Expr::Call(call) => {
                    let Expr::Attribute(func) = &*call.func else { return };
                    if !matches!(func.attr.as_str(), "filtered" | "sorted" | "mapped") {
                        return;
                    }
                    for arg in call
                        .arguments
                        .args
                        .iter()
                        .chain(call.arguments.keywords.iter().map(|k| &k.value))
                    {
                        if let Expr::Lambda(lambda) = arg {
                            lambdas.push((lambda.range(), &*func.value));
                        }
                    }
                }
                Expr::ListComp(comp) => {
                    for generator in &comp.generators {
                        if let Expr::Name(name) = &generator.target {
                            comprehensions.push((comp.range(), name.id.as_str(), &generator.iter));
                        }
                    }
                }
                Expr::SetComp(comp) => {
                    for generator in &comp.generators {
                        if let Expr::Name(name) = &generator.target {
                            comprehensions.push((comp.range(), name.id.as_str(), &generator.iter));
                        }
                    }
                }
                Expr::Generator(comp) => {
                    for generator in &comp.generators {
                        if let Expr::Name(name) = &generator.target {
                            comprehensions.push((comp.range(), name.id.as_str(), &generator.iter));
                        }
                    }
                }
                Expr::DictComp(comp) => {
                    for generator in &comp.generators {
                        if let Expr::Name(name) = &generator.target {
                            comprehensions.push((comp.range(), name.id.as_str(), &generator.iter));
                        }
                    }
                }
                _ => {}
            }
        });
        Receivers {
            lambdas,
            comprehensions,
        }
    }

    /// The records `expr` holds, with `scopes` around it.
    pub(crate) fn records(&self, expr: &Expr, scopes: &[Scope]) -> Option<Records> {
        self.records_at(expr, scopes, 0)
    }

    fn records_at(&self, expr: &Expr, scopes: &[Scope], depth: usize) -> Option<Records> {
        if depth > 16 {
            return None;
        }
        let model = |name: &str| {
            Some(Records {
                models: vec![name.to_string()],
                path: Vec::new(),
            })
        };
        match expr {
            Expr::Name(name) => self.name(name.id.as_str(), name.range(), scopes, depth),
            // `self.env["sale.order"]`, `env["sale.order"]`
            Expr::Subscript(subscript) if is_env(&subscript.value) => {
                model(subscript.slice.as_string_literal_expr()?.value.to_str())
            }
            // `records[0]`, `records[1:]`
            Expr::Subscript(subscript)
                if matches!(
                    &*subscript.slice,
                    Expr::NumberLiteral(_) | Expr::Slice(_) | Expr::UnaryOp(_)
                ) =>
            {
                self.records_at(&subscript.value, scopes, depth + 1)
            }
            Expr::Attribute(attribute) if is_env(&attribute.value) => match attribute.attr.as_str() {
                "user" => model("res.users"),
                "company" | "companies" => model("res.company"),
                _ => None,
            },
            Expr::Attribute(attribute) => {
                let mut records = self.records_at(&attribute.value, scopes, depth + 1)?;
                records.path.push(attribute.attr.to_string());
                Some(records)
            }
            Expr::Call(call) => {
                let Expr::Attribute(func) = &*call.func else {
                    return None;
                };
                let method = func.attr.as_str();
                if SAME_MODEL.contains(&method) {
                    return self.records_at(&func.value, scopes, depth + 1);
                }
                // `records.mapped("partner_id.country_id")`
                if method == "mapped" {
                    let path = call.arguments.args.first()?.as_string_literal_expr()?.value.to_str();
                    let mut records = self.records_at(&func.value, scopes, depth + 1)?;
                    records.path.extend(path.split('.').map(str::to_string));
                    return Some(records);
                }
                None
            }
            _ => None,
        }
    }

    /// What the name `name`, used at `at`, holds.
    fn name(&self, name: &str, at: TextRange, scopes: &[Scope], depth: usize) -> Option<Records> {
        // A lambda's parameter: the records `filtered` and the like give it.
        for (i, scope) in scopes.iter().enumerate().rev() {
            match scope {
                Scope::Lambda(lambda) => {
                    let binds = lambda
                        .parameters
                        .as_ref()
                        .and_then(|p| p.args.first())
                        .is_some_and(|p| p.parameter.name.as_str() == name);
                    if binds {
                        let (_, receiver) = self.lambdas.iter().find(|(range, _)| *range == lambda.range())?;
                        return self.records_at(receiver, &scopes[..i], depth + 1);
                    }
                }
                Scope::Comprehension => {
                    let comprehension = self
                        .comprehensions
                        .iter()
                        .filter(|(range, bound, _)| *bound == name && range.contains_range(at))
                        .min_by_key(|(range, ..)| range.len());
                    if let Some((_, _, iter)) = comprehension {
                        return self.records_at(iter, &scopes[..i], depth + 1);
                    }
                }
                Scope::Function(_) | Scope::Class(_) => break,
            }
        }
        let function = enclosing_function(scopes);
        if name == "self" {
            let models = enclosing_class(scopes).map(class_models).unwrap_or_default();
            if function.is_some() && !models.is_empty() {
                return Some(Records {
                    models,
                    path: Vec::new(),
                });
            }
            return None;
        }
        // The last assignment or `for` before the use, in the function.
        let function = function?;
        let mut last: Option<(&Expr, bool)> = None;
        let mut last_start = None;
        walk(&function.body, |node, inner| {
            // Not in a nested function or class.
            if inner.iter().any(|s| matches!(s, Scope::Function(_) | Scope::Class(_))) {
                return;
            }
            let Node::Stmt(stmt) = node else { return };
            let (value, iterates) = match stmt {
                Stmt::Assign(assign) if assign.targets.iter().any(|t| is_name(t, name)) => (&*assign.value, false),
                Stmt::AnnAssign(assign) if is_name(&assign.target, name) => match assign.value.as_deref() {
                    Some(value) => (value, false),
                    None => return,
                },
                Stmt::For(for_stmt) if is_name(&for_stmt.target, name) => (&*for_stmt.iter, true),
                _ => return,
            };
            if stmt.start() < at.start() && last_start.is_none_or(|s| s < stmt.start()) {
                last = Some((value, iterates));
                last_start = Some(stmt.start());
            }
        });
        let (value, _) = last?;
        // Looping over records gives records of the same model.
        self.records_at(value, scopes, depth + 1)
    }
}

fn is_name(expr: &Expr, name: &str) -> bool {
    matches!(expr, Expr::Name(n) if n.id.as_str() == name)
}

/// `self.env`, `record.env` or a plain `env`.
fn is_env(expr: &Expr) -> bool {
    match expr {
        Expr::Attribute(attribute) => attribute.attr.as_str() == "env",
        Expr::Name(name) => name.id.as_str() == "env",
        _ => false,
    }
}
