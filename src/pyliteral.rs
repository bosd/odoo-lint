//! Python semantics for literal expressions: `str()`, `repr()` and
//! truthiness, so messages match what Python (and pylint-odoo) print.

use ruff_python_ast::{Expr, Number, UnaryOp};

/// `repr()` of a Python string.
pub fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn repr_float(f: f64) -> String {
    if f.is_infinite() {
        return if f > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if f.is_nan() {
        return "nan".into();
    }
    // Debug prints the shortest round-trip form with a `.0` for integers,
    // like Python's repr for everyday values.
    format!("{f:?}")
}

/// `repr()` of a literal expression; `None` for non-literals.
pub fn repr(expr: &Expr) -> Option<String> {
    Some(match expr {
        Expr::StringLiteral(s) => repr_str(s.value.to_str()),
        Expr::BytesLiteral(b) => {
            let bytes: Vec<u8> = b
                .value
                .as_slice()
                .iter()
                .flat_map(|part| part.value.iter().copied())
                .collect();
            format!("b{}", repr_str(&String::from_utf8_lossy(&bytes)))
        }
        Expr::NumberLiteral(n) => match &n.value {
            Number::Int(i) => i.to_string(),
            Number::Float(f) => repr_float(*f),
            Number::Complex { real, imag } if *real == 0.0 => format!("{}j", repr_float(*imag)),
            Number::Complex { real, imag } => format!("({}+{}j)", repr_float(*real), repr_float(*imag)),
        },
        Expr::BooleanLiteral(b) => if b.value { "True" } else { "False" }.into(),
        Expr::NoneLiteral(_) => "None".into(),
        Expr::EllipsisLiteral(_) => "Ellipsis".into(),
        Expr::UnaryOp(op) if matches!(op.op, UnaryOp::USub) => format!("-{}", repr(&op.operand)?),
        Expr::UnaryOp(op) if matches!(op.op, UnaryOp::UAdd) => repr(&op.operand)?,
        Expr::List(list) => format!("[{}]", repr_all(&list.elts)?),
        Expr::Tuple(tuple) if tuple.elts.len() == 1 => format!("({},)", repr(&tuple.elts[0])?),
        Expr::Tuple(tuple) => format!("({})", repr_all(&tuple.elts)?),
        Expr::Set(set) => format!("{{{}}}", repr_all(&set.elts)?),
        Expr::Call(_) => "set()".into(),
        Expr::Dict(dict) => {
            let items: Option<Vec<String>> = dict
                .items
                .iter()
                .map(|item| Some(format!("{}: {}", repr(item.key.as_ref()?)?, repr(&item.value)?)))
                .collect();
            format!("{{{}}}", items?.join(", "))
        }
        _ => return None,
    })
}

fn repr_all(elts: &[Expr]) -> Option<String> {
    let parts: Option<Vec<String>> = elts.iter().map(repr).collect();
    Some(parts?.join(", "))
}

/// `str()` of a literal expression: like `repr()`, except strings are bare.
pub fn str_of(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
        _ => repr(expr),
    }
}

/// Python truthiness of a literal expression; `None` for non-literals.
pub fn is_truthy(expr: &Expr) -> Option<bool> {
    Some(match expr {
        Expr::StringLiteral(s) => !s.value.to_str().is_empty(),
        Expr::BytesLiteral(b) => b.value.as_slice().iter().any(|part| !part.value.is_empty()),
        Expr::NumberLiteral(n) => match &n.value {
            Number::Int(i) => i.as_i64() != Some(0),
            Number::Float(f) => *f != 0.0,
            Number::Complex { real, imag } => *real != 0.0 || *imag != 0.0,
        },
        Expr::BooleanLiteral(b) => b.value,
        Expr::NoneLiteral(_) => false,
        Expr::EllipsisLiteral(_) => true,
        Expr::UnaryOp(op) if matches!(op.op, UnaryOp::USub | UnaryOp::UAdd) => is_truthy(&op.operand)?,
        Expr::List(list) => !list.elts.is_empty(),
        Expr::Tuple(tuple) => !tuple.elts.is_empty(),
        Expr::Set(set) => !set.elts.is_empty(),
        Expr::Dict(dict) => !dict.items.is_empty(),
        // `set()`, the only call a manifest may contain.
        Expr::Call(_) => false,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruff_python_parser::parse_expression;

    fn expr(src: &str) -> Expr {
        parse_expression(src).unwrap().into_expr()
    }

    #[test]
    fn reprs() {
        let cases = [
            ("'a'", "'a'"),
            ("\"it's\"", "\"it's\""),
            ("'a\\nb'", "'a\\nb'"),
            ("1", "1"),
            ("1.0", "1.0"),
            ("-2", "-2"),
            ("True", "True"),
            ("None", "None"),
            ("[]", "[]"),
            ("['a', 1]", "['a', 1]"),
            ("('a',)", "('a',)"),
            ("{'a': [1]}", "{'a': [1]}"),
            ("set()", "set()"),
            ("b''", "b''"),
        ];
        for (src, want) in cases {
            assert_eq!(repr(&expr(src)).as_deref(), Some(want), "{src}");
        }
        assert_eq!(str_of(&expr("'a'")).as_deref(), Some("a"));
        assert_eq!(str_of(&expr("''")).as_deref(), Some(""));
        assert_eq!(repr(&expr("x")), None);
    }

    #[test]
    fn truthiness() {
        for src in ["''", "0", "0.0", "False", "None", "[]", "()", "{}", "set()", "b''"] {
            assert_eq!(is_truthy(&expr(src)), Some(false), "{src}");
        }
        for src in ["'x'", "1", "True", "[0]", "{'a': 1}", "-1"] {
            assert_eq!(is_truthy(&expr(src)), Some(true), "{src}");
        }
    }
}
