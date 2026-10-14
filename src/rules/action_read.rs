//! ODOO014: an action read with `read()`, which only administrators may do.

use crate::checker::{PythonContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::rules::{Check, Rule};
use crate::visit::{walk, Node, Scope};
use ruff_python_ast::{Expr, ExprCall, ExprContext, Number, Stmt};
use ruff_text_size::{Ranged, TextRange};
use std::collections::HashMap;

pub const ACTION_READ: Rule = Rule {
    code: "ODOO014",
    name: "action-read",
    summary: "An action is read with `env.ref(...).read()`, which only administrators may do.",
    doc: r#"
## What it does

Reports `read()` on an action taken with `env.ref()`:
`self.env.ref("sale.action_orders").read()[0]`, also through a variable.

## Why is this bad?

Only `base.group_system` may read `ir.actions.act_window`,
`ir.actions.server` and the other action models: for every other user the
button that returns the action raises an access error. Since Odoo 14.0
`env["ir.actions.actions"]._for_xml_id("sale.action_orders")` returns the
action as the web client needs it, for every user.

## Example

```python
def action_view_orders(self):
    action = self.env.ref("sale.action_orders").read()[0]
    action["domain"] = [("partner_id", "=", self.id)]
    return action
```

Use instead:

```python
def action_view_orders(self):
    action = self.env["ir.actions.actions"]._for_xml_id("sale.action_orders")
    action["domain"] = [("partner_id", "=", self.id)]
    return action
```

## Fix safety

Unsafe: `_for_xml_id` returns the fields the web client reads, not all of
them, so code that uses another field of the action needs a look. There is
a fix for `.read()[0]` only; the `env.ref()` line goes when nothing else
uses its variable.

## Limitations

An XML id counts as an action when its name or the variable's contains
`action`.
"#,
    check: Check::Python(check),
    min_odoo: Some(OdooVersion::new(14, 0)),
    max_odoo: None,
};

/// `<env>.ref("<id>")`: the source of the environment and of the id literal.
fn env_ref<'a>(source: &'a str, expr: &Expr) -> Option<(&'a str, &'a str, String)> {
    let Expr::Call(call) = expr else { return None };
    let Expr::Attribute(attr) = &*call.func else {
        return None;
    };
    let other_keyword = call
        .arguments
        .keywords
        .iter()
        .any(|k| k.arg.as_ref().is_none_or(|a| a.as_str() != "raise_if_not_found"));
    if attr.attr.as_str() != "ref" || other_keyword || call.arguments.args.len() != 1 {
        return None;
    }
    let env = &source[attr.value.start().to_usize()..attr.value.end().to_usize()];
    if !(env == "env" || env.ends_with(".env")) {
        return None;
    }
    let literal = call.arguments.args[0].as_string_literal_expr()?;
    let id = &source[literal.start().to_usize()..literal.end().to_usize()];
    Some((env, id, literal.value.to_str().to_string()))
}

/// An assignment `name = <env>.ref("<id>")`.
struct RefAssignment<'a> {
    function: TextRange,
    statement: TextRange,
    env: &'a str,
    id: &'a str,
    xml_id: String,
}

fn innermost_function(scopes: &[Scope]) -> TextRange {
    scopes
        .iter()
        .rev()
        .find_map(|s| match s {
            Scope::Function(f) => Some(f.range()),
            _ => None,
        })
        .unwrap_or_default()
}

fn check(ctx: &PythonContext, reporter: &mut Reporter) {
    let source = ctx.source;
    let suite = ctx.parsed.suite();
    // Per variable: its `env.ref()` assignments, and every store and load
    // with its function.
    let mut refs: HashMap<String, Vec<RefAssignment>> = HashMap::new();
    // Per store: its function, its range and the value it is given.
    let mut stores: HashMap<String, Vec<(TextRange, TextRange, Option<TextRange>)>> = HashMap::new();
    let mut values: HashMap<TextRange, TextRange> = HashMap::new();
    let mut loads: HashMap<String, Vec<TextRange>> = HashMap::new();
    // `.read()[0]`: the read call's range and the subscript's.
    let mut first_rows: HashMap<TextRange, TextRange> = HashMap::new();
    walk(suite, |node, scopes| match node {
        Node::Stmt(Stmt::Assign(assign)) => {
            for target in &assign.targets {
                values.insert(target.range(), assign.value.range());
            }
            if let ([Expr::Name(target)], Some((env, id, xml_id))) =
                (assign.targets.as_slice(), env_ref(source, &assign.value))
            {
                refs.entry(target.id.to_string()).or_default().push(RefAssignment {
                    function: innermost_function(scopes),
                    statement: assign.range(),
                    env,
                    id,
                    xml_id,
                });
            }
        }
        Node::Expr(Expr::Name(name)) => match name.ctx {
            ExprContext::Store => stores.entry(name.id.to_string()).or_default().push((
                innermost_function(scopes),
                name.range(),
                values.get(&name.range()).copied(),
            )),
            ExprContext::Load => loads.entry(name.id.to_string()).or_default().push(name.range()),
            _ => {}
        },
        Node::Expr(Expr::Subscript(subscript)) => {
            let zero = matches!(&*subscript.slice, Expr::NumberLiteral(n) if n.value == Number::Int(ruff_python_ast::Int::ZERO));
            if let (true, Expr::Call(call)) = (zero, &*subscript.value) {
                first_rows.insert(call.range(), subscript.range());
            }
        }
        _ => {}
    });
    walk(suite, |node, scopes| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(attr) = &*call.func else { return };
        if attr.attr.as_str() != "read" {
            return;
        }
        let function = innermost_function(scopes);
        let (env, id, xml_id, variable) = if let Some((env, id, xml_id)) = env_ref(source, &attr.value) {
            (env, id, xml_id, None)
        } else if let Expr::Name(name) = &*attr.value {
            let var = name.id.as_str();
            // The last store of the variable before the read, in the
            // function, must be the `env.ref()`. The read's own target
            // (`action = action.read()[0]`) comes after it.
            let var_stores = stores.get(var).map(Vec::as_slice).unwrap_or_default();
            let last_store = var_stores
                .iter()
                .filter(|(f, r, value)| {
                    *f == function && r.end() <= call.start() && !value.is_some_and(|v| v.contains_range(call.range()))
                })
                .map(|(_, r, _)| *r)
                .max_by_key(|r| r.start());
            let Some(assignment) = refs.get(var).and_then(|a| {
                a.iter()
                    .find(|a| a.function == function && last_store.is_some_and(|s| a.statement.contains_range(s)))
            }) else {
                return;
            };
            // Where the variable's value is no longer the action: the read's
            // own target, or the next store after the read.
            let end = var_stores
                .iter()
                .filter(|(f, r, value)| {
                    *f == function && (value.is_some_and(|v| v.contains_range(call.range())) || r.start() >= call.end())
                })
                .map(|(_, r, value)| {
                    if value.is_some_and(|v| v.contains_range(call.range())) {
                        call.end()
                    } else {
                        r.start()
                    }
                })
                .min()
                .unwrap_or(function.end());
            (
                assignment.env,
                assignment.id,
                assignment.xml_id.clone(),
                Some((var, assignment, end)),
            )
        } else {
            return;
        };
        let name = xml_id.rsplit('.').next().unwrap_or_default();
        if !name.contains("action") && !variable.is_some_and(|(var, _, _)| var.contains("action")) {
            return;
        }
        let violation = reporter.report(
            &ACTION_READ,
            call.start(),
            format!("Only administrators may read an action; use `env[\"ir.actions.actions\"]._for_xml_id({id})`"),
        );
        let Some(subscript) = first_rows.get(&call.range()).filter(|_| call.arguments.is_empty()) else {
            return;
        };
        violation.fix = Some(fix(source, env, id, *subscript, variable, &loads, call));
    });
}

fn fix(
    source: &str,
    env: &str,
    id: &str,
    subscript: TextRange,
    variable: Option<(&str, &RefAssignment, ruff_text_size::TextSize)>,
    loads: &HashMap<String, Vec<TextRange>>,
    call: &ExprCall,
) -> Fix {
    let quote = id
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .chars()
        .next()
        .unwrap_or('"');
    let mut edits = vec![Edit::replace(
        subscript.start().to_usize(),
        subscript.end().to_usize(),
        format!("{env}[{quote}ir.actions.actions{quote}]._for_xml_id({id})"),
    )];
    if let Some((var, assignment, end)) = variable {
        // The `env.ref()` line goes when the read was its only use.
        let used_elsewhere = loads.get(var).is_some_and(|l| {
            l.iter()
                .any(|r| r.start() >= assignment.statement.end() && r.end() <= end && !call.range().contains_range(*r))
        });
        if !used_elsewhere {
            let (start, end) = (
                assignment.statement.start().to_usize(),
                assignment.statement.end().to_usize(),
            );
            let line_start = source[..start].rfind('\n').map_or(0, |n| n + 1);
            let rest = &source[end..];
            let after = rest.trim_start_matches([' ', '\t']);
            if source[line_start..start].trim().is_empty() && after.starts_with('\n') {
                edits.push(Edit::delete(line_start, end + (rest.len() - after.len()) + 1));
            }
        }
    }
    Fix::unsafe_("Use `_for_xml_id`", edits)
}
