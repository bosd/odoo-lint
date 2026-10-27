//! `__manifest__.py` parsing: the manifest is a Python dict literal.

use ruff_python_ast::{Expr, ExprDict, ModModule, Number, Operator, Stmt, UnaryOp};
use ruff_python_parser::{parse_module, Parsed};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Manifest file names, in order of preference: `__openerp__.py` up to Odoo
/// 9.0, `__terp__.py` and `__odoo__.py` in early versions.
pub const MANIFEST_FILE_NAMES: &[&str] = &["__manifest__.py", "__openerp__.py", "__odoo__.py", "__terp__.py"];

pub fn is_manifest_file_name(name: &str) -> bool {
    MANIFEST_FILE_NAMES.contains(&name)
}

/// Manifest keys that list a module's files: data and demo files, tests,
/// and `qweb`, whose entries are globs. `assets` (globs too) is not a list.
pub const FILE_KEYS: &[&str] = &["data", "demo", "demo_xml", "init_xml", "qweb", "test", "update_xml"];

/// Manifest keys of the data files Odoo loads, in the order it loads them:
/// data, then demo.
pub const LOADED_KEYS: &[&str] = &["init_xml", "update_xml", "data", "demo_xml", "demo"];

/// The manifest of the module folder `dir`, if it is one.
pub fn manifest_path(dir: &Path) -> Option<PathBuf> {
    MANIFEST_FILE_NAMES.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

/// Whether `dir` is a module folder: one with a manifest.
pub fn is_module_dir(dir: &Path) -> bool {
    manifest_path(dir).is_some()
}

/// The folder of module `name` in the first of `dirs` that has it, as Odoo
/// looks a module up on its addons path.
pub fn find_module_dir(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    dirs.iter().map(|dir| dir.join(name)).find(|path| is_module_dir(path))
}

/// The module folders in `dirs`, in addons path order and sorted within a
/// folder, each once.
pub fn module_dirs(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if is_module_dir(&path) && seen.insert(path.canonicalize().unwrap_or_else(|_| path.clone())) {
                found.push(path);
            }
        }
    }
    found
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
