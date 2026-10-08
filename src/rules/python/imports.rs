//! Rules on import statements.

use crate::checker::{PythonContext, Reporter};
use crate::rules::{Check, Rule};
use ruff_python_ast::Stmt;
use ruff_text_size::Ranged;
use std::path::Path;

pub const ODOO_EXCEPTION_WARNING: Rule = Rule {
    code: "R8101",
    name: "odoo-exception-warning",
    summary: "`odoo.exceptions.Warning` is imported.",
    doc: r#"
## What it does

Reports `from odoo.exceptions import Warning`.

## Why is this bad?

`Warning` is a deprecated alias of `UserError`, removed in later Odoo
versions, and it shadows Python's built-in `Warning`.

## Example

```python
from odoo.exceptions import Warning
```

Use instead:

```python
from odoo.exceptions import UserError
```
"#,
    check: Check::Python(check_exception_warning),
    min_odoo: None,
    max_odoo: None,
};

pub const ODOO_ADDONS_RELATIVE_IMPORT: Rule = Rule {
    code: "W8150",
    name: "odoo-addons-relative-import",
    summary: "A module imports itself through `odoo.addons.<module>`.",
    doc: r#"
## What it does

Reports imports of the module's own code through `odoo.addons.<module>`.
Files directly in `tests/` and migration scripts are skipped.

## Why is this bad?

The absolute import breaks when the module folder is renamed or installed
under another name, and hides that the import is local.

## Example

In module `acme_sale`:

```python
from odoo.addons.acme_sale.models.sale import helper
```

Use instead:

```python
from ..models.sale import helper
```
"#,
    check: Check::Python(check_relative_import),
    min_odoo: None,
    max_odoo: None,
};

pub const TEST_FOLDER_IMPORTED: Rule = Rule {
    code: "E8130",
    name: "test-folder-imported",
    summary: "`tests` is imported from a package `__init__.py`.",
    doc: r#"
## What it does

Reports imports of the `tests` package in an `__init__.py`.

## Why is this bad?

Odoo discovers and loads tests by itself, only when tests run. Importing them
in `__init__.py` loads test code, and its test-only dependencies, on every
production server.
"#,
    check: Check::Python(check_test_folder_imported),
    min_odoo: None,
    max_odoo: None,
};

fn check_exception_warning(ctx: &PythonContext, reporter: &mut Reporter) {
    crate::visit::walk(ctx.parsed.suite(), |node, _| {
        let crate::visit::Node::Stmt(Stmt::ImportFrom(import)) = node else {
            return;
        };
        let from_exceptions =
            import.level == 0 && import.module.as_ref().is_some_and(|m| m.as_str() == "odoo.exceptions");
        if from_exceptions && import.names.iter().any(|a| a.name.as_str() == "Warning") {
            reporter.report(
                &ODOO_EXCEPTION_WARNING,
                import.start(),
                "`odoo.exceptions.Warning` is a deprecated alias to `odoo.exceptions.UserError` use `from odoo.exceptions import UserError`",
            );
        }
    });
}

/// Module names imported through `odoo.addons` by an import statement.
fn odoo_addons_modules(stmt: &Stmt) -> Vec<String> {
    match stmt {
        Stmt::ImportFrom(import) => {
            let module = import.module.as_ref().map(|m| m.as_str()).unwrap_or("");
            if !module.contains("odoo.addons") {
                return Vec::new();
            }
            let parts: Vec<&str> = module.split('.').collect();
            match parts.get(2) {
                Some(name) => vec![name.to_string()],
                None => import.names.first().map(|a| a.name.to_string()).into_iter().collect(),
            }
        }
        Stmt::Import(import) => import
            .names
            .iter()
            .filter(|a| a.name.as_str().contains("odoo.addons"))
            .filter_map(|a| a.name.as_str().split('.').nth(2).map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn check_relative_import(ctx: &PythonContext, reporter: &mut Reporter) {
    let Some(module) = ctx.module else { return };
    let file = Path::new(ctx.file_path);
    let file_dir = file.parent().unwrap_or(Path::new(""));
    // Migration scripts (migrations/<version>/*.py) import modules by name.
    if file_dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == "migrations")
    {
        return;
    }
    // Files directly in tests/ run only with the module installed.
    let relative_dir = file_dir.strip_prefix(&module.path).ok();
    let in_tests = relative_dir.is_some_and(|d| d == Path::new("tests"));
    let top_level: Vec<*const Stmt> = ctx.parsed.suite().iter().map(|s| s as *const Stmt).collect();
    crate::visit::walk(ctx.parsed.suite(), |node, _| {
        let crate::visit::Node::Stmt(stmt) = node else { return };
        if !matches!(stmt, Stmt::Import(_) | Stmt::ImportFrom(_)) {
            return;
        }
        if in_tests && top_level.contains(&(stmt as *const Stmt)) {
            return;
        }
        if odoo_addons_modules(stmt).contains(&module.name) {
            reporter.report(
                &ODOO_ADDONS_RELATIVE_IMPORT,
                stmt.start(),
                format!(
                    "Same Odoo module absolute import. You should use relative import with \".\" instead of \"odoo.addons.{}\"",
                    module.name
                ),
            );
        }
    });
}

/// Dotted name of the package an `__init__.py` belongs to, as astroid names
/// it: the module name followed by the sub-package folders.
fn package_name(ctx: &PythonContext, init_file: &Path) -> String {
    let dir = init_file.parent().unwrap_or(Path::new(""));
    match ctx.module {
        Some(module) => {
            let mut name = module.name.clone();
            if let Ok(relative) = dir.strip_prefix(&module.path) {
                for part in relative.components() {
                    name.push('.');
                    name.push_str(&part.as_os_str().to_string_lossy());
                }
            }
            name
        }
        None => dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

fn check_test_folder_imported(ctx: &PythonContext, reporter: &mut Reporter) {
    let file = Path::new(ctx.file_path);
    if file.file_name().is_none_or(|n| n != "__init__.py") {
        return;
    }
    for stmt in ctx.parsed.suite() {
        let packages: Vec<&str> = match stmt {
            Stmt::ImportFrom(import) => match &import.module {
                Some(module) => module.as_str().split('.').take(1).collect(),
                None => import.names.iter().map(|a| a.name.as_str()).collect(),
            },
            Stmt::Import(import) => import
                .names
                .iter()
                .filter_map(|a| a.name.as_str().split('.').next())
                .collect(),
            _ => continue,
        };
        if packages.contains(&"tests") {
            reporter.report(
                &TEST_FOLDER_IMPORTED,
                stmt.start(),
                format!("Test folder imported in module {}", package_name(ctx, file)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::{run_python_rule, run_python_rule_with, ModuleInfo};
    use crate::settings::Settings;
    use std::path::PathBuf;

    fn module() -> ModuleInfo {
        ModuleInfo {
            name: "acme_sale".into(),
            path: PathBuf::from("addons/acme_sale"),
            manifest_path: PathBuf::from("addons/acme_sale/__manifest__.py"),
            manifest: None,
        }
    }

    fn run(rule: &Rule, src: &str, file: &str) -> usize {
        run_python_rule_with(rule, src, file, Some(&module()), &Settings::default()).len()
    }

    #[test]
    fn exception_warning() {
        let src = "from odoo.exceptions import Warning\nfrom odoo.exceptions import UserError\n";
        assert_eq!(run_python_rule(&ODOO_EXCEPTION_WARNING, src).len(), 1);
    }

    #[test]
    fn relative_import() {
        let src = "from odoo.addons.acme_sale.models import sale\nfrom odoo.addons import acme_sale\nimport odoo.addons.acme_sale.models\nfrom odoo.addons.sale.models import x\nfrom . import y\n";
        assert_eq!(
            run(&ODOO_ADDONS_RELATIVE_IMPORT, src, "addons/acme_sale/models/a.py"),
            3
        );
        assert_eq!(
            run(&ODOO_ADDONS_RELATIVE_IMPORT, src, "addons/acme_sale/tests/test_a.py"),
            0
        );
        assert_eq!(
            run(
                &ODOO_ADDONS_RELATIVE_IMPORT,
                src,
                "addons/acme_sale/migrations/17.0.1.0.0/post.py"
            ),
            0
        );
    }

    #[test]
    fn tests_imported() {
        let src = "from . import models\nfrom . import tests\nfrom .tests import common\nimport tests\n";
        let v = run_python_rule_with(
            &TEST_FOLDER_IMPORTED,
            src,
            "addons/acme_sale/__init__.py",
            Some(&module()),
            &Settings::default(),
        );
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].message, "Test folder imported in module acme_sale");
        assert_eq!(run(&TEST_FOLDER_IMPORTED, src, "addons/acme_sale/models/a.py"), 0);
    }
}
