//! ODOO009: fields of data records, checked against the models of the
//! addons path.

mod common;

use common::write;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::path::{Path, PathBuf};

fn module(root: &Path, name: &str, depends: &[&str], python: &str, data: &[(&str, &str)]) {
    let depends: Vec<String> = depends.iter().map(|d| format!("'{d}'")).collect();
    let files: Vec<String> = data.iter().map(|(path, _)| format!("'{path}'")).collect();
    write(
        root,
        &format!("{name}/__manifest__.py"),
        format!(
            "{{'name': '{name}', 'depends': [{}], 'data': [{}]}}\n",
            depends.join(", "),
            files.join(", ")
        ),
    );
    write(root, &format!("{name}/__init__.py"), "from . import models\n");
    write(root, &format!("{name}/models.py"), python);
    for (path, contents) in data {
        write(root, &format!("{name}/{path}"), contents);
    }
}

/// `base` with partners and scheduled actions; `extra`, which nothing
/// depends on, adds a partner field.
fn odoo(root: &Path) -> PathBuf {
    let core = root.join("core");
    module(
        &core,
        "base",
        &[],
        "from odoo import fields, models\n\n\nclass Partner(models.Model):\n    _name = 'res.partner'\n\n    name = fields.Char()\n    parent_id = fields.Many2one('res.partner')\n\n\nclass Cron(models.Model):\n    _name = 'ir.cron'\n\n    name = fields.Char()\n    interval_number = fields.Integer()\n",
        &[],
    );
    module(
        &core,
        "extra",
        &["base"],
        "from odoo import fields, models\n\n\nclass Partner(models.Model):\n    _inherit = 'res.partner'\n\n    vip = fields.Boolean()\n",
        &[],
    );
    core
}

const DATA: &str = r#"<odoo>
    <record id="cron" model="ir.cron">
        <field name="name">Cleanup</field>
        <field name="interval_number" eval="1"/>
        <field name="numbercall">-1</field>
    </record>
    <record id="partner" model="res.partner">
        <field name="name">Acme</field>
        <field name="vip" eval="True"/>
        <field name="x_studio_code">A</field>
        <field name="display_name">Acme</field>
    </record>
    <record id="unknown" model="some.model">
        <field name="whatever">1</field>
    </record>
</odoo>
"#;

const CSV: &str = "id,name,parent_id:id,ref\npartner_b,B,partner,b\n";

fn messages(paths: &[PathBuf], addons_path: Vec<PathBuf>) -> Vec<(String, usize, String)> {
    let mut settings = Settings::default();
    settings.select = vec!["ODOO009".into()];
    settings.addons_path = addons_path;
    let mut found: Vec<(String, usize, String)> = lint_paths(paths, &settings)
        .into_iter()
        .map(|v| {
            let file = Path::new(&v.file_path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (file, v.line, v.message)
        })
        .collect();
    found.sort();
    found
}

#[test]
fn fields_of_data_records() {
    let dir = tempfile::tempdir().unwrap();
    let core = odoo(dir.path());
    let addons = dir.path().join("addons");
    module(
        &addons,
        "acme",
        &["base"],
        "",
        &[("data/data.xml", DATA), ("data/res.partner.csv", CSV)],
    );
    assert_eq!(
        messages(std::slice::from_ref(&addons), vec![core]),
        vec![
            (
                "data.xml".to_string(),
                5,
                "Field `numbercall` does not exist on `ir.cron`".to_string()
            ),
            (
                "data.xml".to_string(),
                9,
                "Field `vip` does not exist on `res.partner`; it is defined in `extra`, which `depends` does not reach"
                    .to_string()
            ),
            (
                "res.partner.csv".to_string(),
                1,
                "Field `ref` does not exist on `res.partner`".to_string()
            ),
        ]
    );
}

#[test]
fn silent_without_all_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let core = odoo(dir.path());
    let addons = dir.path().join("addons");
    module(
        &addons,
        "acme",
        &["base", "somewhere_else"],
        "",
        &[("data/data.xml", DATA)],
    );
    assert!(messages(std::slice::from_ref(&addons), vec![core]).is_empty());
    module(&addons, "acme", &["base"], "", &[("data/data.xml", DATA)]);
    assert!(messages(&[addons], Vec::new()).is_empty());
}
