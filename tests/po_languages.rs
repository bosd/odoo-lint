//! PO105: the configured languages of a repository.

mod common;

use common::write;
use odoo_lint::config::OdooLintConfig;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::path::Path;

fn settings(languages: &str) -> Settings {
    let mut settings = Settings::default();
    settings.select = vec!["PO105".into()];
    settings.config =
        OdooLintConfig::from_odoo_lint_toml(&format!("[rules.po-translation-missing]\nlanguages = {languages}\n"))
            .unwrap();
    settings
}

/// (file name, line, message) of the violations under `root`.
fn found(root: &Path, settings: &Settings) -> Vec<(String, usize, String)> {
    let mut found: Vec<(String, usize, String)> = lint_paths(&[root.to_path_buf()], settings)
        .into_iter()
        .map(|v| {
            let name = Path::new(&v.file_path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (name, v.line, v.message)
        })
        .collect();
    found.sort();
    found
}

fn module(root: &Path, name: &str, models: &str) {
    write(
        root,
        &format!("{name}/__manifest__.py"),
        "{'name': 'Mod', 'version': '19.0.1.0.0', 'license': 'AGPL-3', 'data': ['views/views.xml']}\n",
    );
    write(root, &format!("{name}/__init__.py"), "from . import models\n");
    write(root, &format!("{name}/models/__init__.py"), "from . import task\n");
    write(root, &format!("{name}/models/task.py"), models);
    write(root, &format!("{name}/views/views.xml"), "<odoo/>\n");
}

const POT: &str = r#"msgid ""
msgstr ""

#. module: acme_po
#: model:ir.model.fields,field_description:acme_po.field_project_task__do_not_plan
msgid "Do not plan"
msgstr ""

#. module: acme_po
#: model:ir.model.fields,help:acme_po.field_project_task__do_not_plan
msgid "Keep the task out of planning"
msgstr ""
"#;

#[test]
fn no_i18n_at_all() {
    let dir = tempfile::tempdir().unwrap();
    module(
        dir.path(),
        "acme_po",
        "from odoo import fields, models\n\n\nclass Task(models.Model):\n    _inherit = \"project.task\"\n    do_not_plan = fields.Boolean()\n",
    );
    // Only technical code: nothing to translate.
    module(
        dir.path(),
        "acme_tech",
        "def post_init_hook(env):\n    env.cr.execute(\"SELECT 1\")\n",
    );
    assert_eq!(
        found(dir.path(), &settings(r#"["nl", "es"]"#)),
        vec![
            (
                "__manifest__.py".into(),
                1,
                "The module has translatable terms but no `i18n/es.po`; the configuration requires `es`".into()
            ),
            (
                "__manifest__.py".into(),
                1,
                "The module has translatable terms but no `i18n/nl.po`; the configuration requires `nl`".into()
            ),
        ]
    );
    // Without languages the rule does nothing.
    let mut plain = Settings::default();
    plain.select = vec!["PO105".into()];
    assert!(found(dir.path(), &plain).is_empty());
}

#[test]
fn untranslated_terms() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "acme_po", "def f():\n    pass\n");
    write(dir.path(), "acme_po/i18n/acme_po.pot", POT);
    // Dutch translates one term; Belgian Dutch adds the other.
    write(
        dir.path(),
        "acme_po/i18n/nl.po",
        POT.replacen(
            "msgstr \"\"\n\n#. module: acme_po\n#: model:ir.model.fields,help",
            "msgstr \"Niet plannen\"\n\n#. module: acme_po\n#: model:ir.model.fields,help",
            1,
        ),
    );
    write(
        dir.path(),
        "acme_po/i18n/nl_BE.po",
        POT.replace(
            "msgid \"Keep the task out of planning\"\nmsgstr \"\"",
            "msgid \"Keep the task out of planning\"\nmsgstr \"Niet in de planning\"",
        ),
    );
    write(dir.path(), "acme_po/i18n/es.po", POT);
    assert_eq!(
        found(dir.path(), &settings(r#"["nl", "es", "nl_BE", "de"]"#)),
        vec![
            (
                "__manifest__.py".into(),
                1,
                "The module has translatable terms but no `i18n/de.po`; the configuration requires `de`".into()
            ),
            (
                "es.po".into(),
                6,
                "2 of the 2 terms of `acme_po.pot` have no `es` translation, e.g. \"Do not plan\"".into()
            ),
        ]
    );
}
