//! Which `.po` files the PO rules lint: the module's, not its documentation's.

mod common;

use common::write;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::path::Path;

const PO: &str = "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\n#: ../../guides/code_overview.rst:84\nmsgid \"Hello\"\nmsgstr \"Bonjour\"\n";

#[test]
fn documentation_translations_are_not_odoos() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "acme/__manifest__.py",
        "{'name': 'Acme', 'version': '18.0.1.0.0', 'license': 'AGPL-3'}\n",
    );
    write(dir.path(), "acme/__init__.py", "");
    write(dir.path(), "acme/i18n/fr.po", PO);
    write(dir.path(), "acme/doc/locale/fr/LC_MESSAGES/guides.po", PO);
    let mut settings = Settings::default();
    settings.select = vec!["PO102".into()];
    let found: Vec<String> = lint_paths(&[dir.path().to_path_buf()], &settings)
        .into_iter()
        .map(|v| {
            Path::new(&v.file_path)
                .strip_prefix(dir.path())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert_eq!(found, vec!["acme/i18n/fr.po".to_string()]);
}

#[test]
fn a_noqa_translator_comment_suppresses_in_its_entry() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "acme/__manifest__.py",
        "{'name': 'Acme', 'version': '18.0.1.0.0', 'license': 'AGPL-3'}\n",
    );
    write(dir.path(), "acme/__init__.py", "");
    write(
        dir.path(),
        "acme/i18n/fr.po",
        &PO.replace("#: ../", "# noqa: PO102\n#: ../"),
    );
    let mut settings = Settings::default();
    settings.select = vec!["PO102".into()];
    assert!(lint_paths(&[dir.path().to_path_buf()], &settings).is_empty());
}
