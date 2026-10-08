//! `__manifest__.py` parsing: the manifest is a Python dict literal.

use ruff_python_ast::{Expr, ExprDict, ModModule, Number, Operator, Stmt, UnaryOp};
use ruff_python_parser::{parse_module, Parsed};

/// File names Odoo accepts for a module manifest (`__openerp__.py` up to 9.0).
pub const MANIFEST_FILE_NAMES: &[&str] = &["__manifest__.py", "__openerp__.py"];

pub fn is_manifest_file_name(name: &str) -> bool {
    MANIFEST_FILE_NAMES.contains(&name)
}

#[derive(Debug)]
pub struct Manifest {
    parsed: Parsed<ModModule>,
}

impl Manifest {
    /// Parses a manifest the way Odoo loads it, with `ast.literal_eval`: the
    /// file must be a single dict literal. `None` otherwise, like pylint-odoo,
    /// which then skips the manifest checks.
    pub fn parse(source: &str) -> Option<Self> {
        let parsed = parse_module(source).ok()?;
        let manifest = Self { parsed };
        let dict = manifest.find_dict()?;
        dict.items
            .iter()
            .all(|item| item.key.as_ref().is_some_and(is_literal) && is_literal(&item.value))
            .then_some(manifest)
    }

    fn find_dict(&self) -> Option<&ExprDict> {
        match self.parsed.suite().as_slice() {
            [Stmt::Expr(expr)] => expr.value.as_dict_expr(),
            _ => None,
        }
    }

    pub fn dict(&self) -> &ExprDict {
        self.find_dict().expect("checked in Manifest::parse")
    }

    /// `(key, key expression, value)` for every entry with a string key.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &Expr, &Expr)> {
        self.dict().items.iter().filter_map(|item| {
            let key = item.key.as_ref()?;
            let name = key.as_string_literal_expr()?.value.to_str();
            Some((name, key, &item.value))
        })
    }

    /// The key expression and value of `key`; the last one wins, like in Python.
    pub fn entry(&self, key: &str) -> Option<(&Expr, &Expr)> {
        self.entries()
            .filter(|(k, _, _)| *k == key)
            .map(|(_, k, v)| (k, v))
            .last()
    }

    pub fn get(&self, key: &str) -> Option<&Expr> {
        self.entry(key).map(|(_, value)| value)
    }

    /// The value of `key` when it is a plain string literal.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_string_literal_expr().map(|s| s.value.to_str())
    }
}

/// Whether Python's `ast.literal_eval` accepts `expr`.
fn is_literal(expr: &Expr) -> bool {
    match expr {
        Expr::StringLiteral(_)
        | Expr::BytesLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_)
        | Expr::EllipsisLiteral(_) => true,
        Expr::List(list) => list.elts.iter().all(is_literal),
        Expr::Tuple(tuple) => tuple.elts.iter().all(is_literal),
        Expr::Set(set) => set.elts.iter().all(is_literal),
        Expr::Dict(dict) => dict
            .items
            .iter()
            .all(|item| item.key.as_ref().is_some_and(is_literal) && is_literal(&item.value)),
        Expr::UnaryOp(op) => matches!(op.op, UnaryOp::UAdd | UnaryOp::USub) && op.operand.is_number_literal_expr(),
        // `1 + 2j` style complex numbers.
        Expr::BinOp(bin) => {
            matches!(bin.op, Operator::Add | Operator::Sub)
                && is_real_number(&bin.left)
                && matches!(&*bin.right, Expr::NumberLiteral(n) if matches!(n.value, Number::Complex { .. }))
        }
        // `set()` is the only call literal_eval allows.
        Expr::Call(call) => {
            matches!(&*call.func, Expr::Name(name) if name.id.as_str() == "set")
                && call.arguments.args.is_empty()
                && call.arguments.keywords.is_empty()
        }
        _ => false,
    }
}

fn is_real_number(expr: &Expr) -> bool {
    match expr {
        Expr::NumberLiteral(n) => !matches!(n.value, Number::Complex { .. }),
        Expr::UnaryOp(op) => matches!(op.op, UnaryOp::UAdd | UnaryOp::USub) && is_real_number(&op.operand),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"# Copyright header
{
    "name": "Acme Sale",
    'author': "Acme Corp, Odoo Community Association (OCA)",
    "depends": ["sale"],
    "installable": True,
}
"#;

    #[test]
    fn reads_keys() {
        let m = Manifest::parse(SRC).unwrap();
        assert_eq!(m.get_str("name"), Some("Acme Sale"));
        assert_eq!(m.get_str("author"), Some("Acme Corp, Odoo Community Association (OCA)"));
        assert!(m.get("depends").unwrap().is_list_expr());
        assert!(m.get_str("depends").is_none());
        assert!(m.get("license").is_none());
        assert_eq!(m.entries().count(), 4);
    }

    #[test]
    fn rejects_non_manifests() {
        assert!(Manifest::parse("x = 1\n").is_none());
        assert!(Manifest::parse("{'name': \n").is_none());
        // Valid Python, but literal_eval (and so Odoo) rejects these.
        assert!(Manifest::parse("{'key': '' or ''}\n").is_none());
        assert!(Manifest::parse("{'version': VERSION}\n").is_none());
        assert!(Manifest::parse("\"\"\"doc\"\"\"\n{'name': 'x'}\n").is_none());
    }

    #[test]
    fn accepts_all_literal_forms() {
        let src = "{'a': [1, -2, (3.0, None)], 'b': {True: b'x'}, 'c': set(), 'd': 1 + 2j, 'e': ...}\n";
        assert!(Manifest::parse(src).is_some());
    }

    #[test]
    fn manifest_names() {
        assert!(is_manifest_file_name("__manifest__.py"));
        assert!(is_manifest_file_name("__openerp__.py"));
        assert!(!is_manifest_file_name("__init__.py"));
    }
}
