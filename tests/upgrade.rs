//! Upgrade rules: findings and fixes per Odoo version step.

use odoo_lint::fix::FixMode;
use odoo_lint::fixer::fix_paths;
use odoo_lint::linter::lint_paths;
use odoo_lint::odoo_version::OdooVersion;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

fn settings(select: &str, version: OdooVersion) -> Settings {
    let mut settings = Settings::default();
    settings.target_version = version;
    settings.select = vec![select.to_string()];
    settings
}

fn write(root: &Path, path: &str, contents: &str) -> PathBuf {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}

/// A module for `version` with the given data files and Python file.
fn module(root: &Path, version: &str, xml: &[(&str, &str)], python: &str) -> PathBuf {
    let data: Vec<String> = xml.iter().map(|(name, _)| format!("'{name}'")).collect();
    write(
        root,
        "acme_up/__manifest__.py",
        &format!(
            "{{'name': 'Up', 'version': '{version}', 'license': 'AGPL-3', 'data': [{}]}}\n",
            data.join(", ")
        ),
    );
    write(root, "acme_up/__init__.py", "from . import models\n");
    write(root, "acme_up/models/__init__.py", "from . import partner\n");
    write(root, "acme_up/models/partner.py", python);
    for (name, contents) in xml {
        write(root, &format!("acme_up/{name}"), contents);
    }
    root.join("acme_up")
}

fn codes(root: &Path, select: &str, version: OdooVersion) -> Vec<(String, usize)> {
    lint_paths(&[root.to_path_buf()], &settings(select, version))
        .into_iter()
        .map(|v| (v.code, v.line))
        .collect()
}

fn fixed(root: &Path, select: &str, version: OdooVersion, file: &Path, mode: FixMode) -> String {
    let result = fix_paths(&[root.to_path_buf()], &settings(select, version), mode);
    result
        .changed
        .into_iter()
        .find(|(p, _, _)| p == file)
        .map_or_else(|| fs::read_to_string(file).unwrap(), |(_, _, new)| new)
}

const V18: OdooVersion = OdooVersion::new(18, 0);

const VIEWS_17: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<odoo>
    <record id="view_partner_list" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="arch" type="xml">
            <tree string="Partners" default_order="name">
                <field name="name"/>
                <field name="child_ids" mode="tree,kanban" context="{'tree_view_ref': 'acme_up.child_list'}">
                    <tree><field name="name"/></tree>
                </field>
            </tree>
        </field>
    </record>
    <record id="view_partner_form" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="inherit_id" ref="base.view_partner_form"/>
        <field name="arch" type="xml">
            <xpath expr="//field[@name='child_ids']/tree" position="inside">
                <field name="street"/>
            </xpath>
        </field>
    </record>
    <record id="action_partner" model="ir.actions.act_window">
        <field name="name">Partners</field>
        <field name="res_model">res.partner</field>
        <field name="view_mode">tree,form</field>
    </record>
    <record id="cron_daily" model="ir.cron">
        <field name="name">Daily</field>
        <field name="numbercall">-1</field>
        <field name="doall" eval="False"/>
    </record>
    <record id="view_partner_search" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="arch" type="xml">
            <search>
                <filter name="created" date="create_date" default_period="this_month,last_year"/>
            </search>
        </field>
    </record>
</odoo>
"#;

const PYTHON_17: &str = r#"from odoo import fields, models


class Partner(models.Model):
    _inherit = "res.partner"

    score = fields.Integer(group_operator="avg")

    def action_open(self):
        if not self.user_has_groups("base.group_system"):
            return False
        return {
            "type": "ir.actions.act_window",
            "res_model": "res.partner",
            "view_mode": "tree,form",
            "views": [(False, "tree"), (False, "form")],
            "context": {"tree_view_ref": "acme_up.child_list"},
        }

    def _name_search(self, name, domain=None, operator="ilike", limit=None, order=None):
        return super()._name_search(name, domain, operator, limit, order)
"#;

#[test]
fn findings_for_odoo_18() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "18.0.1.0.0", &[("views/partner.xml", VIEWS_17)], PYTHON_17);
    let mut found = codes(dir.path(), "U18", V18);
    found.sort();
    assert_eq!(
        found,
        vec![
            ("U1801".into(), 6),
            ("U1801".into(), 9),
            ("U1802".into(), 26),
            ("U1803".into(), 8),
            ("U1803".into(), 18),
            ("U1804".into(), 30),
            ("U1804".into(), 31),
            ("U1805".into(), 37),
            ("U1807".into(), 7),
            ("U1808".into(), 10),
            ("U1809".into(), 15),
            ("U1809".into(), 16),
            ("U1809".into(), 17),
            ("U1810".into(), 20),
        ]
    );
}

#[test]
fn modules_still_on_17_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "17.0.1.0.0", &[("views/partner.xml", VIEWS_17)], PYTHON_17);
    // XML rules follow the module's version; Python rules the target.
    let found = codes(dir.path(), "U18", OdooVersion::new(17, 0));
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn fixes_for_odoo_18() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path(), "18.0.1.0.0", &[("views/partner.xml", VIEWS_17)], PYTHON_17);
    let views = module.join("views/partner.xml");
    let xml = fixed(dir.path(), "U18", V18, &views, FixMode::Safe);
    let expected = VIEWS_17
        .replace(
            "<tree string=\"Partners\" default_order=\"name\">",
            "<list string=\"Partners\" default_order=\"name\">",
        )
        .replace(
            "            </tree>\n        </field>",
            "            </list>\n        </field>",
        )
        .replace(
            "<tree><field name=\"name\"/></tree>",
            "<list><field name=\"name\"/></list>",
        )
        .replace("mode=\"tree,kanban\"", "mode=\"list,kanban\"")
        .replace("'tree_view_ref'", "'list_view_ref'")
        .replace("child_ids']/tree\"", "child_ids']/list\"")
        .replace(
            "<field name=\"view_mode\">tree,form</field>",
            "<field name=\"view_mode\">list,form</field>",
        )
        .replace("        <field name=\"numbercall\">-1</field>\n", "")
        .replace("        <field name=\"doall\" eval=\"False\"/>\n", "")
        .replace(
            "default_period=\"this_month,last_year\"",
            "default_period=\"month,year-1\"",
        );
    assert_eq!(xml, expected);

    let python = fixed(dir.path(), "U18", V18, &module.join("models/partner.py"), FixMode::Safe);
    let expected = PYTHON_17
        .replace("group_operator=", "aggregator=")
        .replace("self.user_has_groups(", "self.env.user.has_groups(")
        .replace("\"view_mode\": \"tree,form\"", "\"view_mode\": \"list,form\"")
        .replace("(False, \"tree\")", "(False, \"list\")")
        .replace("\"tree_view_ref\"", "\"list_view_ref\"");
    assert_eq!(python, expected);
}

#[test]
fn a_limited_cron_is_an_unsafe_fix() {
    let dir = tempfile::tempdir().unwrap();
    let cron = r#"<odoo>
    <record id="cron_once" model="ir.cron">
        <field name="name">Once</field>
        <field name="numbercall">1</field>
    </record>
</odoo>
"#;
    let module = module(dir.path(), "18.0.1.0.0", &[("data/cron.xml", cron)], "");
    let file = module.join("data/cron.xml");
    assert_eq!(fixed(dir.path(), "U1804", V18, &file, FixMode::Safe), cron);
    assert!(!fixed(dir.path(), "U1804", V18, &file, FixMode::Unsafe).contains("numbercall"));
}

#[test]
fn data_files_with_a_data_root() {
    let dir = tempfile::tempdir().unwrap();
    let views = r#"<data>
    <record id="view_list" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="arch" type="xml">
            <tree><field name="name"/></tree>
        </field>
    </record>
</data>
"#;
    module(dir.path(), "18.0.1.0.0", &[("views/list.xml", views)], "");
    assert_eq!(codes(dir.path(), "U1801", V18), vec![("U1801".into(), 5)]);
}
