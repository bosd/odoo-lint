//! ODOO012: fields in views without the widget that suits them.

mod common;

use common::write;
use odoo_lint::fix::FixMode;
use odoo_lint::fixer::fix_paths;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::path::{Path, PathBuf};

fn module(root: &Path, name: &str, depends: &[&str], python: &str, views: &str) -> PathBuf {
    let depends: Vec<String> = depends.iter().map(|d| format!("'{d}'")).collect();
    write(
        root,
        &format!("{name}/__manifest__.py"),
        format!(
            "{{'name': '{name}', 'depends': [{}], 'data': ['views/views.xml']}}\n",
            depends.join(", ")
        ),
    );
    write(root, &format!("{name}/__init__.py"), "from . import models\n");
    write(root, &format!("{name}/models.py"), python);
    write(root, &format!("{name}/views/views.xml"), views)
}

const BASE: &str = "from odoo import fields, models\n\n\nclass Partner(models.Model):\n    _name = 'res.partner'\n\n    name = fields.Char()\n\n\nclass Users(models.Model):\n    _name = 'res.users'\n\n\nclass Tag(models.Model):\n    _name = 'res.tag'\n";

const MODEL: &str = r#"from odoo import fields, models


class Task(models.Model):
    _name = "acme.task"

    partner_id = fields.Many2one("res.partner")
    user_id = fields.Many2one("res.users")
    tag_ids = fields.Many2many("res.tag")
    state = fields.Selection([("draft", "Draft")])
    priority = fields.Selection([("0", "Normal"), ("1", "High")])
    color = fields.Integer()
    filter_domain = fields.Char()
"#;

const VIEWS: &str = r#"<odoo>
    <record id="task_form" model="ir.ui.view">
        <field name="model">acme.task</field>
        <field name="arch" type="xml">
            <form>
                <header><field name="state"/></header>
                <sheet>
                    <field name="partner_id"/>
                    <field name="user_id" widget="many2one_avatar_user"/>
                    <field name="tag_ids"/>
                    <field name="tag_ids"><list><field name="id"/></list></field>
                    <field name="priority"/>
                    <field name="color" invisible="1"/>
                    <field name="filter_domain"/>
                </sheet>
            </form>
        </field>
    </record>
    <record id="task_list" model="ir.ui.view">
        <field name="model">acme.task</field>
        <field name="arch" type="xml">
            <list>
                <field name="partner_id"/>
                <field name="user_id"/>
                <field name="color"/>
            </list>
        </field>
    </record>
    <record id="task_search" model="ir.ui.view">
        <field name="model">acme.task</field>
        <field name="arch" type="xml">
            <search><field name="user_id"/></search>
        </field>
    </record>
    <record id="task_search_inherit" model="ir.ui.view">
        <field name="model">acme.task</field>
        <field name="inherit_id" ref="task_search"/>
        <field name="arch" type="xml">
            <field name="user_id" position="after"><field name="tag_ids"/></field>
        </field>
    </record>
    <record id="task_form_inherit" model="ir.ui.view">
        <field name="model">acme.task</field>
        <field name="inherit_id" ref="task_form"/>
        <field name="arch" type="xml">
            <xpath expr="//sheet" position="inside"><field name="user_id"/></xpath>
        </field>
    </record>
</odoo>
"#;

fn settings(core: &Path) -> Settings {
    let mut settings = Settings::default();
    settings.select = vec!["ODOO012".into()];
    settings.addons_path = vec![core.to_path_buf()];
    settings
}

#[test]
fn widgets_that_suit_fields() {
    let dir = tempfile::tempdir().unwrap();
    let core = dir.path().join("core");
    module(&core, "base", &[], BASE, "<odoo/>\n");
    let addons = dir.path().join("addons");
    let views = module(&addons, "acme", &["base"], MODEL, VIEWS);
    let mut found: Vec<(usize, String)> = lint_paths(std::slice::from_ref(&addons), &settings(&core))
        .into_iter()
        .map(|v| (v.line, v.message))
        .collect();
    found.sort();
    let lines: Vec<(usize, &str)> = found
        .iter()
        .map(|(line, message)| (*line, message.split('"').nth(1).unwrap()))
        .collect();
    assert_eq!(
        lines,
        vec![
            (6, "statusbar"),
            (10, "many2many_tags"),
            (12, "priority"),
            (14, "domain"),
            (24, "many2one_avatar_user"),
            (25, "color_picker"),
            (46, "many2one_avatar_user"),
        ]
    );
    // Not in `ALL`: an opt-in review rule.
    let mut all = settings(&core);
    all.select = vec!["ALL".into()];
    assert!(!lint_paths(std::slice::from_ref(&addons), &all)
        .iter()
        .any(|v| v.code == "ODOO012"));

    let result = fix_paths(std::slice::from_ref(&addons), &settings(&core), FixMode::Unsafe);
    let (_, _, fixed) = result.changed.iter().find(|(p, _, _)| p == &views).unwrap();
    assert!(fixed.contains(r#"<field name="state" widget="statusbar"/>"#), "{fixed}");
    assert!(
        fixed.contains(r#"<field name="filter_domain" widget="domain"/>"#),
        "{fixed}"
    );
}
