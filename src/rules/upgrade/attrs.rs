//! Odoo 16's view `attrs` domains and `states` as Odoo 17's Python
//! expressions: `[('state', '=', 'draft')]` becomes `state == 'draft'`.

use ruff_python_ast::Expr;
use ruff_python_parser::parse_expression;
use ruff_text_size::Ranged;

/// The keys of `attrs` that are attributes of their own in 17.0.
pub(crate) const MODIFIERS: &[&str] = &["invisible", "readonly", "required", "column_invisible"];

/// A condition, with its precedence for parentheses: `or` < `and` < `not` <
/// comparisons and names.
enum Cond {
    Atom(String, u8),
    Not(Box<Cond>),
    And(Vec<Cond>),
    Or(Vec<Cond>),
}

const OR: u8 = 1;
const AND: u8 = 2;
const NOT: u8 = 3;
const ATOM: u8 = 4;

impl Cond {
    fn precedence(&self) -> u8 {
        match self {
            Cond::Atom(_, p) => *p,
            Cond::Not(_) => NOT,
            Cond::And(_) => AND,
            Cond::Or(_) => OR,
        }
    }

    fn render(&self) -> String {
        let join =
            |items: &[Cond], sep: &str, min: u8| items.iter().map(|c| c.wrapped(min)).collect::<Vec<_>>().join(sep);
        match self {
            Cond::Atom(text, _) => text.clone(),
            Cond::Not(inner) => format!("not {}", inner.wrapped(NOT)),
            Cond::And(items) => join(items, " and ", AND),
            Cond::Or(items) => join(items, " or ", OR),
        }
    }

    fn wrapped(&self, min: u8) -> String {
        if self.precedence() < min {
            format!("({})", self.render())
        } else {
            self.render()
        }
    }

    fn and(items: Vec<Cond>) -> Cond {
        Self::flat(items, true)
    }

    fn or(items: Vec<Cond>) -> Cond {
        Self::flat(items, false)
    }

    fn flat(items: Vec<Cond>, and: bool) -> Cond {
        let mut out = Vec::new();
        for item in items {
            match item {
                Cond::And(inner) if and => out.extend(inner),
                Cond::Or(inner) if !and => out.extend(inner),
                other => out.push(other),
            }
        }
        match (out.len(), and) {
            (1, _) => out.pop().expect("one item"),
            (_, true) => Cond::And(out),
            (_, false) => Cond::Or(out),
        }
    }
}

/// The text of `expr` in `src`.
fn text<'a>(src: &'a str, expr: &Expr) -> &'a str {
    &src[expr.range().start().to_usize()..expr.range().end().to_usize()]
}

fn term(src: &str, elts: &[Expr]) -> Option<Cond> {
    let [field, operator, value] = elts else { return None };
    let field = field.as_string_literal_expr()?.value.to_str();
    if field.is_empty()
        || !field
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_'))
    {
        return None;
    }
    let operator = operator.as_string_literal_expr()?.value.to_str();
    let value_text = text(src, value).trim();
    // An empty list is how 16.0 views compared x2many fields: in 17.0 the
    // field is a list object, so test it for emptiness.
    let empty =
        matches!(value, Expr::List(l) if l.elts.is_empty()) || matches!(value, Expr::Tuple(t) if t.elts.is_empty());
    let falsy = matches!(value, Expr::BooleanLiteral(b) if !b.value) || matches!(value, Expr::NoneLiteral(_)) || empty;
    let truthy = matches!(value, Expr::BooleanLiteral(b) if b.value);
    let atom = |s: String| Some(Cond::Atom(s, ATOM));
    match operator {
        "=" | "==" if falsy => Some(Cond::Atom(format!("not {field}"), NOT)),
        "=" | "==" if truthy => atom(field.to_string()),
        "!=" | "<>" if falsy => atom(field.to_string()),
        "!=" | "<>" if truthy => Some(Cond::Atom(format!("not {field}"), NOT)),
        "=" | "==" => atom(format!("{field} == {value_text}")),
        "!=" | "<>" => atom(format!("{field} != {value_text}")),
        "<" | ">" | "<=" | ">=" => atom(format!("{field} {operator} {value_text}")),
        "in" | "not in" => atom(format!("{field} {operator} {value_text}")),
        _ => None,
    }
}

/// Parses one condition in Polish notation from `items[*index..]`.
fn condition(src: &str, items: &[Expr], index: &mut usize) -> Option<Cond> {
    let item = items.get(*index)?;
    *index += 1;
    if let Some(operator) = item.as_string_literal_expr() {
        return match operator.value.to_str() {
            "&" => Some(Cond::and(vec![
                condition(src, items, index)?,
                condition(src, items, index)?,
            ])),
            "|" => Some(Cond::or(vec![
                condition(src, items, index)?,
                condition(src, items, index)?,
            ])),
            "!" => Some(Cond::Not(Box::new(condition(src, items, index)?))),
            _ => None,
        };
    }
    match item {
        Expr::Tuple(t) => term(src, &t.elts),
        Expr::List(l) => term(src, &l.elts),
        _ => None,
    }
}

/// A domain (or a constant) as a Python expression.
fn domain(src: &str, expr: &Expr) -> Option<String> {
    let items = match expr {
        Expr::List(l) => &l.elts,
        Expr::Tuple(t) => &t.elts,
        Expr::BooleanLiteral(b) => return Some(if b.value { "True" } else { "False" }.into()),
        Expr::NumberLiteral(_) => return Some(if text(src, expr).trim() == "0" { "False" } else { "True" }.into()),
        _ => return None,
    };
    if items.is_empty() {
        return Some("True".into());
    }
    let mut index = 0;
    let mut parts = Vec::new();
    while index < items.len() {
        parts.push(condition(src, items, &mut index)?);
    }
    Some(Cond::and(parts).render())
}

/// The modifiers of an `attrs` value combined with `states`, as Odoo 16
/// did: the `states` term is appended to the `invisible` domain (an implicit
/// "and", or the operand of a dangling operator), and replaces a constant.
/// `None` when it isn't a dict of convertible domains.
pub(crate) fn attrs_to_expressions(value: Option<&str>, states: Option<&str>) -> Option<Vec<(String, String)>> {
    let state_term = match states {
        Some(states) => Some(states_term(states)?),
        None => None,
    };
    let mut out = Vec::new();
    if let Some(value) = value {
        let parsed = parse_expression(value).ok()?;
        let Expr::Dict(dict) = parsed.expr() else { return None };
        for item in &dict.items {
            let key = item.key.as_ref()?.as_string_literal_expr()?.value.to_str();
            if !MODIFIERS.contains(&key) {
                return None;
            }
            let expression = match (&state_term, key, &item.value) {
                (Some(term), "invisible", Expr::List(_) | Expr::Tuple(_)) => {
                    // Append the term to the list as written, and convert that.
                    let list = text(value, &item.value).trim_end();
                    let body = list[..list.len() - 1].trim_end().trim_end_matches(',');
                    let close = &list[list.len() - 1..];
                    let combined = if body.len() == 1 {
                        format!("{body}{term}{close}")
                    } else {
                        format!("{body}, {term}{close}")
                    };
                    let parsed = parse_expression(&combined).ok()?;
                    domain(&combined, parsed.expr())?
                }
                (Some(_), "invisible", _) => states_to_invisible(states?)?,
                _ => domain(value, &item.value)?,
            };
            out.push((key.to_string(), expression));
        }
    }
    if let Some(states) = states {
        if !out.iter().any(|(key, _)| key == "invisible") {
            out.push(("invisible".into(), states_to_invisible(states)?));
        }
    }
    Some(out)
}

fn state_names(value: &str) -> Option<Vec<&str>> {
    let states: Vec<&str> = value.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    let valid = !states.is_empty()
        && states
            .iter()
            .all(|s| s.chars().all(|c| c.is_alphanumeric() || c == '_'));
    valid.then_some(states)
}

/// The domain term of `states="a,b"`.
fn states_term(value: &str) -> Option<String> {
    let states = state_names(value)?;
    Some(match states.as_slice() {
        [one] => format!("('state', '!=', '{one}')"),
        many => format!(
            "('state', 'not in', ({}))",
            many.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", ")
        ),
    })
}

/// `states="a,b"` as the `invisible` expression it means.
pub(crate) fn states_to_invisible(value: &str) -> Option<String> {
    let states = state_names(value)?;
    Some(match states.as_slice() {
        [one] => format!("state != '{one}'"),
        many => format!(
            "state not in ({})",
            many.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", ")
        ),
    })
}

/// Escapes an expression for a double-quoted XML attribute.
pub(crate) fn escape_attribute(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(attrs: &str) -> String {
        let mut converted = attrs_to_expressions(Some(attrs), None).unwrap();
        assert_eq!(converted.len(), 1);
        converted.pop().unwrap().1
    }

    #[test]
    fn terms() {
        assert_eq!(one("{'invisible': [('state', '=', 'draft')]}"), "state == 'draft'");
        assert_eq!(one("{'invisible': [('partner_id', '=', False)]}"), "not partner_id");
        assert_eq!(one("{'invisible': [('partner_id', '!=', False)]}"), "partner_id");
        assert_eq!(
            one("{'readonly': [('state', 'not in', ['draft', 'sent'])]}"),
            "state not in ['draft', 'sent']"
        );
        assert_eq!(
            one("{'required': [('parent.type', '=', 'sale')]}"),
            "parent.type == 'sale'"
        );
        assert_eq!(one("{'invisible': [('qty', '>', 0)]}"), "qty > 0");
        assert_eq!(one("{'invisible': [('line_ids', '=', [])]}"), "not line_ids");
        assert_eq!(one("{'invisible': [('line_ids', '!=', [])]}"), "line_ids");
        assert_eq!(one("{'invisible': True}"), "True");
        assert_eq!(one("{'invisible': 1}"), "True");
    }

    #[test]
    fn operators() {
        assert_eq!(
            one("{'invisible': [('a', '=', 1), ('b', '=', 2)]}"),
            "a == 1 and b == 2"
        );
        assert_eq!(
            one("{'invisible': ['|', ('a', '=', 1), ('b', '=', 2)]}"),
            "a == 1 or b == 2"
        );
        assert_eq!(
            one("{'invisible': ['|', ('a', '=', 1), ('b', '=', 2), ('c', '=', 3)]}"),
            "(a == 1 or b == 2) and c == 3"
        );
        assert_eq!(
            one("{'invisible': ['&', '|', ('a', '=', 1), ('b', '=', 2), '|', ('c', '=', 3), ('d', '=', 4)]}"),
            "(a == 1 or b == 2) and (c == 3 or d == 4)"
        );
        assert_eq!(one("{'invisible': ['!', ('a', '=', 1)]}"), "not a == 1");
        assert_eq!(
            one("{'invisible': ['!', '|', ('a', '=', 1), ('b', '=', 2)]}"),
            "not (a == 1 or b == 2)"
        );
        assert_eq!(
            one("{'invisible': ['|', '|', ('a', '=', 1), ('b', '=', 2), ('c', '=', 3)]}"),
            "a == 1 or b == 2 or c == 3"
        );
    }

    #[test]
    fn several_modifiers() {
        assert_eq!(
            attrs_to_expressions(
                Some("{'invisible': [('a', '=', False)], 'readonly': [('state', '!=', 'draft')]}"),
                None
            )
            .unwrap(),
            vec![
                ("invisible".into(), "not a".into()),
                ("readonly".into(), "state != 'draft'".into())
            ]
        );
    }

    #[test]
    fn unsupported() {
        assert!(attrs_to_expressions(Some("{'invisible': [('name', 'ilike', 'x')]}"), None).is_none());
        assert!(attrs_to_expressions(Some("{'invisible': [('id', 'child_of', 1)]}"), None).is_none());
        assert!(attrs_to_expressions(Some("{'nolabel': [('a', '=', 1)]}"), None).is_none());
        assert!(attrs_to_expressions(Some("{'invisible': ['|', ('a', '=', 1)]}"), None).is_none());
        assert!(attrs_to_expressions(Some("not a dict"), None).is_none());
    }

    fn with_states(attrs: Option<&str>, states: &str) -> String {
        let converted = attrs_to_expressions(attrs, Some(states)).unwrap();
        converted.into_iter().find(|(k, _)| k == "invisible").unwrap().1
    }

    #[test]
    fn states_are_appended_like_odoo_16() {
        assert_eq!(with_states(None, "draft,sent"), "state not in ('draft', 'sent')");
        // An implicit "and" with a complete domain.
        assert_eq!(
            with_states(Some("{'invisible': [('a', '=', False)]}"), "done"),
            "not a and state != 'done'"
        );
        // The operand of a dangling operator.
        assert_eq!(
            with_states(Some("{'invisible': ['|', ('a', '=', True)]}"), "done"),
            "a or state != 'done'"
        );
        assert_eq!(
            with_states(
                Some("{'invisible': ['|', '|', ('a', '=', True), ('b', '=', 'full')]}"),
                "done"
            ),
            "a or b == 'full' or state != 'done'"
        );
        // A constant is replaced.
        assert_eq!(with_states(Some("{'invisible': True}"), "done"), "state != 'done'");
        assert_eq!(with_states(Some("{'readonly': True}"), "done"), "state != 'done'");
    }

    #[test]
    fn states() {
        assert_eq!(states_to_invisible("draft").unwrap(), "state != 'draft'");
        assert_eq!(
            states_to_invisible("draft, sent").unwrap(),
            "state not in ('draft', 'sent')"
        );
        assert!(states_to_invisible("").is_none());
    }
}
