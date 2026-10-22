//! ODOO015-ODOO017: widgets and OWL templates against the assets of the
//! module and its dependencies.

use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// `core/web` registering a field widget and a template, and `addons/acme`
/// using them, and some that do not exist.
fn setup(root: &Path) -> PathBuf {
    let core = root.join("core");
    write(
        &core,
        "base/__manifest__.py",
        "{'name': 'base', 'version': '18.0.1.0'}\n",
    );
    write(&core, "base/__init__.py", "");
    write(
        &core,
        "web/__manifest__.py",
        "{'name': 'web', 'version': '18.0.1.0', 'depends': ['base'], 'assets': {'web.assets_backend': ['web/static/src/**/*']}}\n",
    );
    write(&core, "web/__init__.py", "");
    write(
        &core,
        "web/static/src/fields/tags.js",
        "import { registry } from \"@web/core/registry\";\nconst fieldRegistry = registry.category(\"fields\");\nfieldRegistry.add(\"many2many_tags\", {});\nregistry.category(\"fields\").add(\"list.handle\", {}).add(\"daterange\", {});\nregistry.category(\"view_widgets\").add(\"web_ribbon\", {});\n",
    );
    write(
        &core,
        "web/static/src/core/dialog/dialog.xml",
        "<templates>\n    <t t-name=\"web.Dialog\"><div/></t>\n</templates>\n",
    );
    let addons = root.join("addons");
    write(
        &addons,
        "acme/__manifest__.py",
        "{'name': 'acme', 'version': '18.0.1.0.0', 'license': 'AGPL-3', 'depends': ['web'], 'data': ['views/views.xml'], 'assets': {'web.assets_backend': ['acme/static/src/**/*']}}\n",
    );
    write(&addons, "acme/__init__.py", "");
    write(
        &addons,
        "acme/views/views.xml",
        "<odoo>\n    <record id=\"view\" model=\"ir.ui.view\">\n        <field name=\"model\">res.partner</field>\n        <field name=\"arch\" type=\"xml\">\n            <list>\n                <field name=\"sequence\" widget=\"handle\"/>\n                <field name=\"category_id\" widget=\"many2many_tags\"/>\n                <field name=\"name\" widget=\"acme_fancy\"/>\n                <widget name=\"web_ribbon\"/>\n                <widget name=\"acme_banner\"/>\n                <field name=\"date\" widget=\"daterange\"/>\n            </list>\n        </field>\n    </record>\n</odoo>\n",
    );
    write(
        &addons,
        "acme/static/src/deep/er/thing.js",
        "import { Component } from \"@odoo/owl\";\nexport class Thing extends Component {\n    static template = \"acme.Thing\";\n}\nexport class Other extends Component {\n    static template = \"acme.Missing\";\n}\nexport class Base extends Component {\n    static template = \"\";\n}\n",
    );
    write(
        &addons,
        "acme/static/src/deep/er/thing.xml",
        "<templates>\n    <t t-name=\"acme.Thing\"><t t-call=\"web.Dialog\"/><t t-call=\"web.Gone\"/></t>\n    <t t-inherit=\"web.Dialog\" t-inherit-mode=\"extension\"/>\n    <t t-inherit=\"web.Removed\" t-inherit-mode=\"extension\"/>\n</templates>\n",
    );
    addons.join("acme")
}

#[test]
fn widgets_and_templates() {
    let dir = tempfile::tempdir().unwrap();
    let acme = setup(dir.path());
    let mut settings = Settings::default();
    settings.select = vec!["ODOO015".into(), "ODOO016".into(), "ODOO017".into()];
    settings.addons_path = vec![dir.path().join("core")];
    let mut found: Vec<(String, String, usize)> = lint_paths(&[acme], &settings)
        .into_iter()
        .map(|v| {
            (
                v.code,
                Path::new(&v.file_path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                v.line,
            )
        })
        .collect();
    found.sort();
    let expected: Vec<(String, String, usize)> = [
        ("ODOO015", "views.xml", 8),
        ("ODOO015", "views.xml", 10),
        ("ODOO016", "thing.js", 6),
        ("ODOO017", "thing.xml", 2),
        ("ODOO017", "thing.xml", 4),
    ]
    .into_iter()
    .map(|(c, f, l)| (c.to_string(), f.to_string(), l))
    .collect();
    assert_eq!(found, expected);
}

#[test]
fn nothing_without_the_web_client_sources() {
    let dir = tempfile::tempdir().unwrap();
    let acme = setup(dir.path());
    fs::remove_dir_all(dir.path().join("core/web/static")).unwrap();
    let mut settings = Settings::default();
    settings.select = vec!["ODOO015".into(), "ODOO016".into(), "ODOO017".into()];
    settings.addons_path = vec![dir.path().join("core")];
    assert!(lint_paths(&[acme], &settings).is_empty());
}
