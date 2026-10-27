//! ODOO010 and ODOO011: methods named in fields and buttons, checked
//! against the models of the addons path.

mod common;

use common::write;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

fn module(root: &Path, name: &str, depends: &[&str], python: &str, views: Option<&str>) {
    let depends: Vec<String> = depends.iter().map(|d| format!("'{d}'")).collect();
    let data = if views.is_some() { "'views/views.xml'" } else { "" };
    write(
        root,
        &format!("{name}/__manifest__.py"),
        format!(
            "{{'name': '{name}', 'depends': [{}], 'data': [{data}]}}\n",
            depends.join(", ")
        ),
    );
    write(root, &format!("{name}/__init__.py"), "from . import models\n");
    write(root, &format!("{name}/models.py"), python);
    if let Some(views) = views {
        write(root, &format!("{name}/views/views.xml"), views);
    }
}

/// A miniature Odoo: `odoo/models.py` with `BaseModel`, and in
/// `odoo/addons` the modules `base` (partners) and `sale` (orders, lines
/// and a mixin through a plain Python class).
fn odoo(root: &Path) -> PathBuf {
    write(
        root,
        "odoo/models.py",
        "class BaseModel:\n    def write(self, vals):\n        pass\n\n    def action_archive(self):\n        pass\n\n\nclass Model(BaseModel):\n    pass\n",
    );
    let addons = root.join("odoo/addons");
    module(
        &addons,
        "base",
        &[],
        "from odoo import fields, models\n\n\nclass Partner(models.Model):\n    _name = 'res.partner'\n\n    name = fields.Char()\n\n    def action_partner(self):\n        pass\n",
        None,
    );
    module(
        &addons,
        "sale",
        &["base"],
        "from odoo import fields, models\n\n\nclass Order(models.Model):\n    _name = 'sale.order'\n\n    partner_id = fields.Many2one('res.partner')\n    line_ids = fields.One2many('sale.order.line', 'order_id')\n    total = fields.Float(compute='_compute_total')\n\n    def _compute_total(self):\n        pass\n\n    def action_confirm(self):\n        pass\n\n\nclass Line(models.Model):\n    _name = 'sale.order.line'\n\n    order_id = fields.Many2one('sale.order')\n\n    def action_line(self):\n        pass\n\n\nclass Unknown(models.Model, SomeMixin):\n    _name = 'sale.mixed'\n",
        None,
    );
    addons
}

const PYTHON: &str = r#"from odoo import api, fields, models


class Order(models.Model):
    _inherit = "sale.order"

    margin = fields.Float(compute="_compute_margin", inverse="_inverse_margin")
    note = fields.Char(compute="_compute_total", search="_search_note")
    state = fields.Selection("_selection_state")
    kind = fields.Selection([("a", "A")])

    def _compute_margin(self):
        pass


class Mixed(models.Model):
    _inherit = "sale.mixed"

    other = fields.Char(compute="_from_the_mixin")
"#;

const VIEWS: &str = r#"<odoo>
    <record id="order_form" model="ir.ui.view">
        <field name="model">sale.order</field>
        <field name="arch" type="xml">
            <form>
                <button name="action_confirm" type="object"/>
                <button name="action_archive" type="object"/>
                <button name="action_cancel" type="object"/>
                <button name="%(base.action)d" type="action"/>
                <field name="line_ids">
                    <list>
                        <button name="action_line" type="object"/>
                        <button name="action_confirm" type="object"/>
                    </list>
                </field>
                <field name="partner_id"/>
            </form>
        </field>
    </record>
    <record id="order_list" model="ir.ui.view">
        <field name="model">sale.order.line</field>
        <field name="arch" type="xml">
            <list>
                <groupby name="order_id"><button name="action_confirm" type="object"/></groupby>
            </list>
        </field>
    </record>
    <record id="order_form_inherit" model="ir.ui.view">
        <field name="model">sale.order</field>
        <field name="inherit_id" ref="order_form"/>
        <field name="arch" type="xml">
            <xpath expr="//field[@name='line_ids']/list" position="inside">
                <button name="action_line" type="object"/>
            </xpath>
            <xpath expr="/form" position="inside">
                <button name="action_missing" type="object"/>
            </xpath>
            <button name="action_line" position="replace">
                <button name="action_line_edit" type="object"/>
            </button>
            <xpath expr="/form" position="inside">
                <button name="action_close" type="object" special="cancel"/>
            </xpath>
        </field>
    </record>
</odoo>
"#;

fn messages(paths: &[PathBuf], addons_path: Vec<PathBuf>) -> Vec<(usize, String)> {
    let mut settings = Settings::default();
    settings.select = vec!["ODOO010".into(), "ODOO011".into()];
    settings.addons_path = addons_path;
    let mut found: Vec<(usize, String)> = lint_paths(paths, &settings)
        .into_iter()
        .map(|v| (v.line, format!("{} {}", v.code, v.message)))
        .collect();
    found.sort();
    found
}

#[test]
fn methods_of_fields_and_buttons() {
    let dir = tempfile::tempdir().unwrap();
    let core = odoo(dir.path());
    let addons = dir.path().join("addons");
    module(&addons, "acme_sale", &["sale"], PYTHON, Some(VIEWS));
    assert_eq!(
        messages(std::slice::from_ref(&addons), vec![core]),
        vec![
            (
                7,
                "ODOO010 Method `_inverse_margin` (`inverse` of `margin`) does not exist on `sale.order`".to_string()
            ),
            (
                8,
                "ODOO010 Method `_search_note` (`search` of `note`) does not exist on `sale.order`".to_string()
            ),
            (
                8,
                "ODOO011 Method `action_cancel` does not exist on `sale.order`".to_string()
            ),
            (
                9,
                "ODOO010 Method `_selection_state` (`selection` of `state`) does not exist on `sale.order`".to_string()
            ),
            (
                13,
                "ODOO011 Method `action_confirm` does not exist on `sale.order.line`".to_string()
            ),
            (
                36,
                "ODOO011 Method `action_missing` does not exist on `sale.order`".to_string()
            ),
        ]
    );
}

#[test]
fn silent_without_odoo_source() {
    let dir = tempfile::tempdir().unwrap();
    let core = odoo(dir.path());
    fs::remove_file(dir.path().join("odoo/models.py")).unwrap();
    let addons = dir.path().join("addons");
    module(&addons, "acme_sale", &["sale"], PYTHON, Some(VIEWS));
    assert!(messages(std::slice::from_ref(&addons), vec![core]).is_empty());
}

#[test]
fn abstract_models_may_leave_methods_to_their_heirs() {
    let dir = tempfile::tempdir().unwrap();
    let core = odoo(dir.path());
    let addons = dir.path().join("addons");
    module(
        &addons,
        "acme_abstract",
        &["base"],
        "from odoo import fields, models\n\n\nclass Abstract(models.AbstractModel):\n    _name = 'acme.abstract'\n\n    data = fields.Char(compute='_compute_data')\n\n\nclass Concrete(models.Model):\n    _name = 'acme.concrete'\n    _inherit = 'acme.abstract'\n\n    other = fields.Char(compute='_compute_other')\n\n    def _compute_data(self):\n        pass\n",
        None,
    );
    assert_eq!(
        messages(&[addons.join("acme_abstract")], vec![core]),
        vec![(
            14,
            "ODOO010 Method `_compute_other` (`compute` of `other`) does not exist on `acme.concrete`".to_string()
        )]
    );
}
