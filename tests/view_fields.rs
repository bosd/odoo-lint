//! ODOO004: fields in views, checked against the models of the addons path.

use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn module(root: &Path, name: &str, depends: &[&str], python: &str, views: Option<&str>) {
    let depends: Vec<String> = depends.iter().map(|d| format!("'{d}'")).collect();
    let data = if views.is_some() { "'views/views.xml'" } else { "" };
    write(
        root,
        &format!("{name}/__manifest__.py"),
        &format!(
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

/// A miniature Odoo in `core/`: `base` with partners, and `sale` with orders
/// and lines; `extra` (not a dependency of anything) adds a partner field.
fn odoo(root: &Path) -> PathBuf {
    let core = root.join("core");
    module(
        &core,
        "base",
        &[],
        "from odoo import fields, models\n\n\nclass Partner(models.Model):\n    _name = 'res.partner'\n\n    name = fields.Char()\n    country_id: 'Country' = fields.Many2one('res.country')\n",
        None,
    );
    module(
        &core,
        "sale",
        &["base"],
        "from odoo import fields, models\n\n\nclass Order(models.Model):\n    _name = 'sale.order'\n    _inherit = ['mail.thread']\n\n    partner_id = fields.Many2one('res.partner')\n    order_line = fields.One2many('sale.order.line', 'order_id')\n\n\nclass Line(models.Model):\n    _name = 'sale.order.line'\n\n    order_id = fields.Many2one('sale.order')\n    product_uom_qty = fields.Float()\n\n\nclass Thread(models.AbstractModel):\n    _name = 'mail.thread'\n\n    message_ids = fields.One2many('mail.message', 'res_id')\n\n\nclass Message(models.Model):\n    _name = 'mail.message'\n\n    body = fields.Html()\n",
        None,
    );
    module(
        &core,
        "extra",
        &["base"],
        "from odoo import fields, models\n\n\nclass Partner(models.Model):\n    _inherit = 'res.partner'\n\n    vip = fields.Boolean()\n",
        None,
    );
    core
}

const VIEWS: &str = r#"<odoo>
    <record id="order_form" model="ir.ui.view">
        <field name="model">sale.order</field>
        <field name="arch" type="xml">
            <form>
                <field name="partner_id"/>
                <field name="partner_idd"/>
                <field name="message_ids"/>
                <field name="display_name"/>
                <field name="order_line">
                    <list>
                        <field name="product_uom_qty"/>
                        <field name="partner_id"/>
                    </list>
                </field>
            </form>
        </field>
    </record>
    <record id="order_form_inherit" model="ir.ui.view">
        <field name="model">sale.order</field>
        <field name="inherit_id" ref="order_form"/>
        <field name="arch" type="xml">
            <xpath expr="//field[@name='partner_id']" position="after">
                <field name="product_uom_qty"/>
                <field name="vip"/>
            </xpath>
            <xpath expr="//field[@name='order_line']/list/field[@name='product_uom_qty']" position="after">
                <field name="name"/>
            </xpath>
            <field name="partner_id" position="attributes">
                <attribute name="nonexistent">1</attribute>
            </field>
        </field>
    </record>
    <record id="partner_form" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="arch" type="xml">
            <form><field name="vip"/><field name="country_id"/></form>
        </field>
    </record>
</odoo>
"#;

fn messages(paths: &[PathBuf], addons_path: Vec<PathBuf>) -> Vec<(usize, String)> {
    let mut settings = Settings::default();
    settings.select = vec!["ODOO004".into()];
    settings.addons_path = addons_path;
    let mut found: Vec<(usize, String)> = lint_paths(paths, &settings)
        .into_iter()
        .map(|v| (v.line, v.message))
        .collect();
    found.sort();
    found
}

#[test]
fn fields_in_views() {
    let dir = tempfile::tempdir().unwrap();
    let core = odoo(dir.path());
    let addons = dir.path().join("addons");
    module(&addons, "acme_sale", &["sale"], "", Some(VIEWS));
    assert_eq!(
        messages(std::slice::from_ref(&addons), vec![core]),
        vec![
            (7, "Field `partner_idd` does not exist on `sale.order`".to_string()),
            (13, "Field `partner_id` does not exist on `sale.order.line`".to_string()),
            (
                25,
                "Field `vip` does not exist on `sale.order` or the models of its x2many fields".to_string()
            ),
            (28, "Field `name` does not exist on `sale.order.line`".to_string()),
            (
                38,
                "Field `vip` does not exist on `res.partner`; it is defined in `extra`, which `depends` does not reach"
                    .to_string()
            ),
        ]
    );
}

#[test]
fn silent_without_all_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let core = odoo(dir.path());
    let addons = dir.path().join("addons");
    module(&addons, "acme_sale", &["sale", "somewhere_else"], "", Some(VIEWS));
    assert!(messages(std::slice::from_ref(&addons), vec![core]).is_empty());
    // Without an addons path, `sale` cannot be found either.
    module(&addons, "acme_sale", &["sale"], "", Some(VIEWS));
    assert!(messages(&[addons], Vec::new()).is_empty());
}
