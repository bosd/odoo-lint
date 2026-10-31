//! PO106: a module's `.pot` against what `odl pot` writes.

mod common;

use common::write;
use odoo_lint::fix::{Applicability, FixMode};
use odoo_lint::fixer::fix_paths;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::Path;

fn settings(addons_path: Vec<std::path::PathBuf>) -> Settings {
    let mut settings = Settings::default();
    settings.select = vec!["PO106".into()];
    settings.addons_path = addons_path;
    settings
}

fn module(root: &Path, pot: &str) {
    write(
        root,
        "acme/__manifest__.py",
        "{'name': 'Acme', 'version': '19.0.1.0.0', 'license': 'AGPL-3'}\n",
    );
    write(root, "acme/__init__.py", "from . import wizard\n");
    write(
        root,
        "acme/wizard.py",
        "from odoo import _\n\n\ndef check():\n    raise UserError(_(\"Nothing to write\"))\n",
    );
    write(root, "acme/i18n/acme.pot", pot);
}

const OLD: &str =
    "msgid \"\"\nmsgstr \"\"\n\n#. module: acme\n#: code:addons/acme/wizard.py:0\nmsgid \"Gone\"\nmsgstr \"\"\n";

/// An addons path with `base`, the only dependency.
fn base(root: &Path) -> Vec<std::path::PathBuf> {
    write(
        root,
        "odoo/base/__manifest__.py",
        "{'name': 'Base', 'version': '19.0.1.0.0'}\n",
    );
    vec![root.join("odoo")]
}

#[test]
fn reports_and_writes_an_outdated_template() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), OLD);
    let paths = [dir.path().join("acme")];
    let settings = settings(base(dir.path()));
    let found = lint_paths(&paths, &settings);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].file_path.ends_with("acme.pot"));
    assert_eq!(
        found[0].message,
        "The template lacks 1 term of the source and has 1 term the source no longer exports; run `odl pot`"
    );
    assert_eq!(found[0].fix.as_ref().unwrap().applicability, Applicability::Safe);

    let result = fix_paths(&paths, &settings, FixMode::Safe);
    assert!(result.remaining.is_empty(), "{:?}", result.remaining);
    result.write().unwrap();
    let pot = fs::read_to_string(dir.path().join("acme/i18n/acme.pot")).unwrap();
    assert!(pot.contains("msgid \"Nothing to write\""), "{pot}");
    assert!(!pot.contains("Gone"), "{pot}");
    assert!(lint_paths(&paths, &settings).is_empty());
}

#[test]
fn without_dependencies_only_missing_terms_of_code_and_data() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), OLD);
    let paths = [dir.path().join("acme")];
    // `base` is not on the addons path: the labels of models may be wrong,
    // so terms the template has too many of are not reported, and nothing
    // is rewritten.
    let settings = settings(Vec::new());
    let found = lint_paths(&paths, &settings);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        found[0].message,
        "The template lacks 1 term of the source's code or data files; \
         run `odl pot` with the module's dependencies on the addons path"
    );
    assert!(found[0].fix.is_none());

    // With the code's term in it, the extra term is not reported.
    let current =
        format!("{OLD}\n#. module: acme\n#: code:addons/acme/wizard.py:0\nmsgid \"Nothing to write\"\nmsgstr \"\"\n");
    fs::write(dir.path().join("acme/i18n/acme.pot"), current).unwrap();
    assert!(lint_paths(&paths, &settings).is_empty());
}

#[test]
fn leaves_a_module_without_template_alone() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), OLD);
    fs::remove_file(dir.path().join("acme/i18n/acme.pot")).unwrap();
    assert!(lint_paths(&[dir.path().join("acme")], &settings(Vec::new())).is_empty());
}
