//! Module checks: OCA's non-XML module checks (MOD001-MOD009) and the files
//! in a module (ODOO002, ODOO003).

use odoo_lint::config::OdooLintConfig;
use odoo_lint::fix::FixMode;
use odoo_lint::fixer::fix_paths;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

fn settings(select: &str) -> Settings {
    let mut settings = Settings::default();
    settings.select = vec![select.to_string()];
    settings
}

fn write(root: &Path, path: &str, contents: impl AsRef<[u8]>) -> PathBuf {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}

/// A module `acme_mod` with the given manifest data files.
fn module(root: &Path, data: &str) -> PathBuf {
    write(
        root,
        "acme_mod/__manifest__.py",
        format!("{{'name': 'Mod', 'version': '17.0.1.0.0', 'license': 'AGPL-3', 'data': [{data}]}}\n"),
    );
    write(root, "acme_mod/__init__.py", "");
    root.join("acme_mod")
}

/// (rule name, file name, line) of the violations under `root`.
fn found(root: &Path, select: &str) -> Vec<(String, String, usize)> {
    found_with(root, &settings(select))
}

fn found_with(root: &Path, settings: &Settings) -> Vec<(String, String, usize)> {
    let mut found: Vec<_> = lint_paths(&[root.to_path_buf()], settings)
        .into_iter()
        .map(|v| {
            let name = Path::new(&v.file_path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (v.name, name, v.line)
        })
        .collect();
    found.sort();
    found
}

fn fixed(root: &Path, select: &str, file: &Path, mode: FixMode) -> String {
    let result = fix_paths(&[root.to_path_buf()], &settings(select), mode);
    result
        .changed
        .into_iter()
        .find(|(p, _, _)| p == file)
        .map_or_else(|| fs::read_to_string(file).unwrap(), |(_, _, new)| new)
}

fn found_one(rule: &str, file: &str, line: usize) -> Vec<(String, String, usize)> {
    vec![(rule.to_string(), file.to_string(), line)]
}

#[test]
fn manifest_syntax_error() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "acme_var/__manifest__.py", "NAME = 'x'\n{'name': NAME}\n");
    write(dir.path(), "acme_var/__init__.py", "");
    write(dir.path(), "acme_noinit/__manifest__.py", "{'name': 'No init'}\n");
    write(dir.path(), "acme_ok/__manifest__.py", "{'name': 'Ok'}\n");
    write(dir.path(), "acme_ok/__init__.py", "");
    let violations = lint_paths(&[dir.path().to_path_buf()], &settings("MOD001"));
    let mut modules: Vec<String> = violations
        .iter()
        .map(|v| {
            Path::new(&v.file_path)
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    modules.sort();
    assert_eq!(modules, vec!["acme_noinit", "acme_var"]);
}

#[test]
fn file_not_used() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path(), "'views/used.xml', './views/../security/rules.xml'");
    write(&module, "views/used.xml", "<odoo/>");
    write(&module, "views/unused.xml", "<odoo/>");
    write(&module, "Security/rules.xml", "<odoo/>");
    write(&module, "security/rules.xml", "<odoo/>");
    write(&module, "data/hooked.csv", "id\n");
    write(&module, "views/deeper/nested.xml", "<odoo/>");
    write(&module, "static/src/xml/widget.xml", "<templates/>");
    let manifest = module.join("__manifest__.py");
    let source = fs::read_to_string(&manifest)
        .unwrap()
        .replace("}\n", ", 'oca_data_manual': ['data/hooked.csv']}\n");
    fs::write(&manifest, source).unwrap();
    let violations = lint_paths(&[dir.path().to_path_buf()], &settings("MOD002"));
    let mut messages: Vec<&str> = violations.iter().map(|v| v.message.as_str()).collect();
    messages.sort();
    assert_eq!(
        messages,
        vec![
            "File \"acme_mod/Security/rules.xml\" is not referenced in the manifest.",
            "File \"acme_mod/views/unused.xml\" is not referenced in the manifest.",
        ]
    );
}

#[test]
fn csv_checks() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(
        dir.path(),
        "'security/ir.model.access.csv', 'data/more.csv', 'data/broken.csv'",
    );
    write(
        &module,
        "security/ir.model.access.csv",
        "id,name,model_id:id,group_id:id,perm_read\naccess_a,a,model_a,,1\n\naccess_b,b,model_b,,1\n",
    );
    write(&module, "data/more.csv", "id,name\n\"access_a\",\"multi\nline\"\n");
    write(&module, "data/broken.csv", b"id,name\nx,caf\xe9\n");
    assert_eq!(
        found(dir.path(), "MOD003"),
        found_one("csv-syntax-error", "broken.csv", 1)
    );
    // The first of the two records, at the line Python's csv module ends it on.
    assert_eq!(
        found(dir.path(), "MOD004"),
        found_one("csv-duplicate-record-id", "ir.model.access.csv", 2)
    );
}

#[test]
fn readme_and_weblate() {
    let dir = tempfile::tempdir().unwrap();
    let long_name = "acme_a_module_with_a_rather_long_technical_name_for_its_features";
    write(
        dir.path(),
        &format!("{long_name}/__manifest__.py"),
        "{'name': 'Long'}\n",
    );
    write(dir.path(), &format!("{long_name}/__init__.py"), "");
    write(dir.path(), &format!("{long_name}/README.md"), "# Long\n");
    // `git remote` sorts the remotes: `origin` comes first.
    write(
        dir.path(),
        ".git/config",
        "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = https://github.com/acme/x.git\n\
         [remote \"origin\"]\n\turl = git@github.com:acme/odoo-addons-for-the-whole-company.git\n",
    );
    assert_eq!(
        found(dir.path(), "MOD005"),
        found_one("prefer-readme-rst", "README.md", 1)
    );
    assert_eq!(
        found(dir.path(), "MOD006"),
        found_one("weblate-component-too-long", "__manifest__.py", 1)
    );
    let message = &lint_paths(&[dir.path().to_path_buf()], &settings("MOD006"))[0].message;
    assert!(
        message.contains(&format!(
            "'odoo-addons-for-the-whole-company-00.0-{long_name}' size 103"
        )),
        "{message}"
    );
}

#[test]
fn unwanted_and_large_files() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path(), "");
    write(&module, "static/description/icon.png", vec![0u8; 10 * 1024]);
    write(&module, "static/img/photo.jpg", vec![0u8; 1500 * 1024]);
    write(&module, "packaging/tool-1.0.x86_64.RPM", vec![0u8; 100]);
    write(&module, "data/backup.sql", "SELECT 1;");
    write(&module, "node_modules/lib/big.zip", vec![0u8; 2000 * 1024]);
    assert_eq!(
        found(dir.path(), "ODOO002"),
        vec![
            ("module-unwanted-file".to_string(), "backup.sql".to_string(), 1),
            ("module-unwanted-file".to_string(), "tool-1.0.x86_64.RPM".to_string(), 1),
        ]
    );
    assert_eq!(
        found(dir.path(), "ODOO003"),
        found_one("module-large-file", "photo.jpg", 1)
    );

    let config = OdooLintConfig::from_odoo_lint_toml(
        "[rules.module-large-file]\nmax-kib = 2000\n[rules.module-unwanted-file]\nextensions = [\".png\"]\n",
    )
    .unwrap();
    let mut configured = settings("ODOO");
    configured.config = config;
    assert_eq!(
        found_with(dir.path(), &configured),
        found_one("module-unwanted-file", "icon.png", 1)
    );
}

#[test]
fn python_checks_and_fixes() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path(), "");
    write(&module, "__init__.py", "from . import models\n");
    let python = r#"# Copyright 2024 Acme
# -*- coding: utf-8 -*-
# License AGPL-3.0 or later
import logging

from odoo import fields, models

_logger = logging.getLogger(__name__)


class Partner(models.Model):
    _inherit = "res.partner"

    date_order = fields.Date(string="Date Order", required=True)
    partner_id = fields.Many2one("res.partner", "Partner")
    tag_ids = fields.Many2many("res.partner.category", "Tag", "rel_table")
    label = fields.Char("Label", related="name")
    issuing_authority = fields.Char(
        string="Issuing Authority",
    )
    third = fields.Char(string="Third",)
    other = fields.Char("Something else")
"#;
    let file = write(&module, "models.py", python);
    assert_eq!(
        found(dir.path(), "MOD"),
        vec![
            ("field-string-redundant".to_string(), "models.py".to_string(), 14),
            ("field-string-redundant".to_string(), "models.py".to_string(), 15),
            ("field-string-redundant".to_string(), "models.py".to_string(), 16),
            ("field-string-redundant".to_string(), "models.py".to_string(), 18),
            ("field-string-redundant".to_string(), "models.py".to_string(), 21),
            ("unused-logger".to_string(), "models.py".to_string(), 8),
            ("use-header-comments".to_string(), "models.py".to_string(), 3),
        ]
    );
    let safe = fixed(dir.path(), "MOD", &file, FixMode::Safe);
    let expected = python
        .replace("_logger = logging.getLogger(__name__)\n", "")
        .replace(
            "fields.Date(string=\"Date Order\", required=True)",
            "fields.Date(required=True)",
        )
        .replace(
            "fields.Many2one(\"res.partner\", \"Partner\")",
            "fields.Many2one(\"res.partner\")",
        )
        // The only argument, with a trailing comma.
        .replace(
            "fields.Char(\n        string=\"Issuing Authority\",\n    )",
            "fields.Char()",
        )
        .replace("fields.Char(string=\"Third\",)", "fields.Char()");
    assert_eq!(safe, expected);
    let all = fixed(dir.path(), "MOD008", &file, FixMode::Unsafe);
    assert!(all.starts_with("# -*- coding: utf-8 -*-\nimport logging\n"), "{all}");
}
