//! Repository conventions: README fragments (MOD011), the module icon
//! (MOD012), tests (MOD013) and form view anchors (XML103).

mod common;

use common::write;
use odoo_lint::config::OdooLintConfig;
use odoo_lint::fix::FixMode;
use odoo_lint::fixer::fix_paths;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::Path;

fn module(root: &Path, name: &str) {
    write(
        root,
        &format!("{name}/__manifest__.py"),
        "{'name': 'Mod', 'version': '19.0.1.0.0', 'license': 'AGPL-3', 'data': ['views/views.xml']}\n",
    );
    write(root, &format!("{name}/__init__.py"), "from . import models\n");
    write(root, &format!("{name}/views/views.xml"), "<odoo/>\n");
}

fn settings(select: &str, toml: &str) -> Settings {
    let mut settings = Settings::default();
    settings.select = vec![select.into()];
    settings.config = OdooLintConfig::from_odoo_lint_toml(toml).unwrap();
    settings
}

/// `path` under `root`, with `/` on Windows too.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/")
}

/// (module or file, line, message) of the violations under `root`.
fn found(root: &Path, settings: &Settings) -> Vec<(String, usize, String)> {
    let mut found: Vec<(String, usize, String)> = lint_paths(&[root.to_path_buf()], settings)
        .into_iter()
        .map(|v| (relative(root, Path::new(&v.file_path)), v.line, v.message))
        .collect();
    found.sort();
    found
}

#[test]
fn readme_fragments() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "acme_frag");
    write(dir.path(), "acme_frag/README.md", "# Mod\n");
    module(dir.path(), "acme_done");
    write(dir.path(), "acme_done/readme/DESCRIPTION.md", "Does things.\n");
    assert_eq!(
        found(dir.path(), &settings("MOD011", "")),
        vec![(
            "acme_frag/__manifest__.py".into(),
            1,
            "The module has no readme/DESCRIPTION.md fragment to generate its README from".into()
        )]
    );
    // Opt-in: ALL leaves it out.
    assert!(found(dir.path(), &settings("ALL", ""))
        .iter()
        .all(|(_, _, m)| !m.contains("readme/")));
}

#[test]
fn tests_for_models() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "acme_untested");
    write(
        dir.path(),
        "acme_untested/models/task.py",
        "from odoo import models\n\n\nclass Task(models.Model):\n    _inherit = \"project.task\"\n",
    );
    module(dir.path(), "acme_tested");
    write(
        dir.path(),
        "acme_tested/models/task.py",
        "from odoo import models\n\n\nclass Task(models.Model):\n    _name = \"acme.task\"\n",
    );
    write(dir.path(), "acme_tested/tests/test_task.py", "");
    // Views only: nothing to test in Python.
    module(dir.path(), "acme_views");
    assert_eq!(
        found(dir.path(), &settings("MOD013", "")),
        vec![(
            "acme_untested/__manifest__.py".into(),
            1,
            "The module declares models but has no tests/test_*.py".into()
        )]
    );
}

#[test]
fn icons() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "tools/company.png", b"company icon");
    write(root, "tools/client.png", b"client icon");
    write(root, "tools/old.png", b"old company icon");
    // Missing; outdated (a file in `replace`); outdated (by hash); its own.
    module(root, "acme_none");
    module(root, "acme_old");
    write(root, "acme_old/static/description/icon.png", b"old company icon");
    module(root, "client/acme_hash");
    write(root, "client/acme_hash/static/description/icon.png", b"hashed");
    module(root, "acme_own");
    write(root, "acme_own/static/description/icon.png", b"an app icon");
    let toml = format!(
        "[rules.module-icon]\nicon = \"tools/company.png\"\nreplace = [\"oca-2015\", \"tools/old.png\", \"sha256:{}\"]\n\
         [rules.module-icon.paths]\n\"client/**\" = \"tools/client.png\"\n",
        sha256_hex(b"hashed")
    );
    let mut settings = settings("MOD012", &toml);
    settings.project_root = root.to_path_buf();
    assert_eq!(
        found(root, &settings),
        vec![
            (
                "acme_none/__manifest__.py".into(),
                1,
                "The module has no static/description/icon.png".into()
            ),
            (
                "acme_old/static/description/icon.png".into(),
                1,
                "The module icon is an outdated one (`tools/old.png`)".into()
            ),
            (
                "client/acme_hash/static/description/icon.png".into(),
                1,
                format!(
                    "The module icon is an outdated one (`sha256:{}`)",
                    sha256_hex(b"hashed")
                )
            ),
        ]
    );
    let result = fix_paths(&[root.to_path_buf()], &settings, FixMode::Safe);
    assert!(result.remaining.is_empty());
    assert!(result.changed.is_empty());
    let copies: Vec<(String, String)> = result
        .copies
        .iter()
        .map(|(from, to)| (relative(root, from), relative(root, to)))
        .collect();
    assert_eq!(
        copies,
        vec![
            (
                "tools/client.png".into(),
                "client/acme_hash/static/description/icon.png".into()
            ),
            (
                "tools/company.png".into(),
                "acme_none/static/description/icon.png".into()
            ),
            (
                "tools/company.png".into(),
                "acme_old/static/description/icon.png".into()
            ),
        ]
    );
    result.write().unwrap();
    assert_eq!(
        fs::read(root.join("acme_none/static/description/icon.png")).unwrap(),
        b"company icon"
    );
    assert_eq!(
        fs::read(root.join("acme_own/static/description/icon.png")).unwrap(),
        b"an app icon"
    );
    assert!(found(root, &settings).is_empty());
    // Without the table the rule does nothing.
    let mut plain = Settings::default();
    plain.select = vec!["MOD012".into()];
    fs::remove_file(root.join("acme_none/static/description/icon.png")).unwrap();
    assert!(found(root, &plain).is_empty());
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn form_container_anchors() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "acme_view");
    write(
        dir.path(),
        "acme_view/views/views.xml",
        r#"<?xml version="1.0" encoding="UTF-8" ?>
<odoo>
    <record id="view_task_form" model="ir.ui.view">
        <field name="model">project.task</field>
        <field name="inherit_id" ref="project.view_task_form2" />
        <field name="arch" type="xml">
            <xpath expr="//sheet" position="inside">
                <group><field name="name" /></group>
            </xpath>
            <xpath expr="//form">
                <div />
            </xpath>
            <sheet position="inside">
                <div />
            </sheet>
            <xpath expr="//sheet/notebook" position="inside">
                <page name="extra" />
            </xpath>
            <xpath expr="//sheet" position="before">
                <header />
            </xpath>
        </field>
    </record>
    <record id="settings_form" model="ir.ui.view">
        <field name="model">res.config.settings</field>
        <field name="inherit_id" ref="base.res_config_settings_view_form" />
        <field name="arch" type="xml">
            <xpath expr="//form" position="inside">
                <app name="acme" />
            </xpath>
        </field>
    </record>
</odoo>
"#,
    );
    let lines: Vec<usize> = found(dir.path(), &settings("XML103", ""))
        .into_iter()
        .map(|(_, l, _)| l)
        .collect();
    assert_eq!(lines, vec![7, 10, 13]);
}
