//! Rules for some Odoo versions follow each module's manifest version when
//! no target version is given.

use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::Path;

const MODEL: &str = r#"from odoo import api, models


class Product(models.Model):
    _inherit = "product.template"

    @api.model
    def name_search(self, name="", args=None, operator="ilike", limit=100):
        return super().name_search(name, args, operator, limit)
"#;

fn module(root: &Path, name: &str, version: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("__manifest__.py"),
        format!("{{'name': '{name}', 'version': '{version}', 'depends': ['product']}}\n"),
    )
    .unwrap();
    fs::write(dir.join("__init__.py"), "from . import models\n").unwrap();
    fs::write(dir.join("models.py"), MODEL).unwrap();
}

#[test]
fn upgrade_rules_follow_the_module_version() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "on_19", "19.0.1.0.0");
    module(dir.path(), "on_17", "17.0.1.0.0");
    module(dir.path(), "unversioned", "1.0.0");
    let mut settings = Settings::default();
    settings.select = vec!["U1910".into()];
    // The default target version, 17.0, is older than the rule.
    let files: Vec<String> = lint_paths(&[dir.path().to_path_buf()], &settings)
        .into_iter()
        .map(|v| v.file_path)
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].contains("on_19"), "{files:?}");

    // Modules without an Odoo series follow the target version.
    settings.target_version = "19.0".parse().unwrap();
    let files: Vec<String> = lint_paths(&[dir.path().to_path_buf()], &settings)
        .into_iter()
        .map(|v| v.file_path)
        .collect();
    assert_eq!(files.len(), 2, "{files:?}");

    // A target version given in the config or with `-v` wins, as
    // pylint-odoo's `valid-odoo-versions` does.
    settings.target_version = "17.0".parse().unwrap();
    settings.target_explicit = true;
    assert!(lint_paths(&[dir.path().to_path_buf()], &settings).is_empty());
}
