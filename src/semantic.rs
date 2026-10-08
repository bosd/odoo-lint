//! Lightweight name resolution for a Python file: what imported names refer
//! to, and which classes are Odoo models. No type inference: just enough to
//! answer what pylint-odoo asks astroid.

use ruff_python_ast::statement_visitor::{walk_stmt, StatementVisitor};
use ruff_python_ast::{Expr, Stmt, StmtClassDef};
use ruff_text_size::{Ranged, TextSize};

/// One name bound by an import statement.
#[derive(Debug, Clone)]
pub struct ImportBinding {
    /// pylint-odoo's lookup key: the alias, or the full dotted name for a
    /// plain `import a.b` (pylint-odoo's `_from_imports`).
    pub key: String,
    /// The name Python binds: the alias, or the first segment of `import a.b`.
    pub bound: String,
    /// Fully qualified name, e.g. `requests.get` for `from requests import get`.
    pub qualified: String,
    /// Start of the import statement.
    pub offset: TextSize,
    pub top_level: bool,
}

#[derive(Debug, Default)]
pub struct Semantic {
    pub imports: Vec<ImportBinding>,
}

/// Odoo model base classes.
pub const ODOO_MODEL_KINDS: &[&str] = &["Model", "AbstractModel", "TransientModel"];

struct ImportCollector {
    depth: usize,
    imports: Vec<ImportBinding>,
}

impl<'a> StatementVisitor<'a> for ImportCollector {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        let top_level = self.depth == 0;
        match stmt {
            Stmt::Import(import) => {
                for alias in &import.names {
                    let name = alias.name.as_str();
                    let asname = alias.asname.as_ref().map(|a| a.as_str());
                    self.imports.push(ImportBinding {
                        key: asname.unwrap_or(name).to_string(),
                        bound: asname
                            .unwrap_or_else(|| name.split('.').next().unwrap_or(name))
                            .to_string(),
                        qualified: name.to_string(),
                        offset: import.start(),
                        top_level,
                    });
                }
            }
            Stmt::ImportFrom(import) => {
                // astroid's modname has no leading dots for relative imports.
                let module = import.module.as_ref().map(|m| m.as_str()).unwrap_or("");
                for alias in &import.names {
                    let name = alias.name.as_str();
                    let bound = alias.asname.as_ref().map_or(name, |a| a.as_str());
                    self.imports.push(ImportBinding {
                        key: bound.to_string(),
                        bound: bound.to_string(),
                        qualified: format!("{module}.{name}"),
                        offset: import.start(),
                        top_level,
                    });
                }
            }
            _ => {}
        }
        let nested = matches!(stmt, Stmt::FunctionDef(_) | Stmt::ClassDef(_));
        if nested {
            self.depth += 1;
        }
        walk_stmt(self, stmt);
        if nested {
            self.depth -= 1;
        }
    }
}

/// `a.b.c` for a chain of names and attributes; `None` otherwise.
pub fn dotted_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name) => Some(name.id.to_string()),
        Expr::Attribute(attr) => Some(format!("{}.{}", dotted_name(&attr.value)?, attr.attr.as_str())),
        _ => None,
    }
}

/// pylint-odoo's `get_func_name`: the name of a called function or method.
pub fn func_name(func: &Expr) -> &str {
    match func {
        Expr::Name(name) => name.id.as_str(),
        Expr::Attribute(attr) => attr.attr.as_str(),
        _ => "",
    }
}

/// pylint-odoo's `get_func_lib`: `lib` in `lib.method(...)`, when `lib` is a name.
pub fn func_lib(func: &Expr) -> &str {
    match func {
        Expr::Attribute(attr) => attr.value.as_name_expr().map_or("", |n| n.id.as_str()),
        _ => "",
    }
}

impl Semantic {
    pub fn new(suite: &[Stmt]) -> Self {
        let mut collector = ImportCollector {
            depth: 0,
            imports: Vec::new(),
        };
        collector.visit_body(suite);
        Self {
            imports: collector.imports,
        }
    }

    /// What `key` refers to at `offset`, using imports seen earlier in the
    /// file in any scope (pylint-odoo's `_from_imports`).
    pub fn resolve_key(&self, key: &str, offset: TextSize) -> Option<&str> {
        self.imports
            .iter()
            .rfind(|b| b.key == key && b.offset < offset)
            .map(|b| b.qualified.as_str())
    }

    /// pylint-odoo's `_static_func_infer_name`: `requests.get` for
    /// `requests.get(...)`, `r.get(...)` with `import requests as r`, or
    /// `get(...)` with `from requests import get`.
    pub fn qualified_call_name(&self, func: &Expr, offset: TextSize) -> Option<String> {
        let lib = func_lib(func);
        let name = func_name(func);
        if !lib.is_empty() {
            let original = self.resolve_key(lib, offset).unwrap_or(lib);
            return Some(format!("{original}.{name}"));
        }
        self.resolve_key(name, offset).map(str::to_string)
    }

    /// The Odoo model kind (`Model`, `AbstractModel`, `TransientModel`) of a
    /// class with a base imported from `odoo`, and that base expression.
    pub fn odoo_model_kind<'a>(&self, class: &'a StmtClassDef) -> Option<(&'static str, &'a Expr)> {
        let bases = class.arguments.as_ref().map(|a| &a.args[..]).unwrap_or(&[]);
        for base in bases {
            let Some(dotted) = dotted_name(base) else { continue };
            let root = dotted.split('.').next().unwrap_or_default();
            let from_odoo = self
                .imports
                .iter()
                .rfind(|b| b.top_level && b.bound == root && b.offset < class.start())
                .is_some_and(|b| b.qualified.split('.').next() == Some("odoo"));
            let last = dotted.rsplit('.').next().unwrap_or_default();
            if from_odoo {
                if let Some(kind) = ODOO_MODEL_KINDS.iter().find(|k| **k == last) {
                    return Some((kind, base));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruff_python_parser::parse_module;

    #[test]
    fn resolves_imports_in_order() {
        let src = "import requests as r\nfrom http import client\nimport http.client\ndef f():\n    from urllib.request import urlopen\n";
        let parsed = parse_module(src).unwrap();
        let s = Semantic::new(parsed.suite());
        let end = TextSize::of(src);
        assert_eq!(s.resolve_key("r", end), Some("requests"));
        assert_eq!(s.resolve_key("client", end), Some("http.client"));
        assert_eq!(s.resolve_key("http.client", end), Some("http.client"));
        assert_eq!(s.resolve_key("urlopen", end), Some("urllib.request.urlopen"));
        assert_eq!(s.resolve_key("r", TextSize::new(0)), None);
    }

    #[test]
    fn odoo_models() {
        let src = "from odoo import models\nfrom odoo.models import TransientModel\nimport other\nclass A(models.Model): pass\nclass B(TransientModel): pass\nclass C(other.Model): pass\n";
        let parsed = parse_module(src).unwrap();
        let s = Semantic::new(parsed.suite());
        let kinds: Vec<_> = parsed
            .suite()
            .iter()
            .filter_map(|stmt| stmt.as_class_def_stmt())
            .map(|c| s.odoo_model_kind(c).map(|(k, _)| k))
            .collect();
        assert_eq!(kinds, vec![Some("Model"), Some("TransientModel"), None]);
    }
}
