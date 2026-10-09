//! Go to definition across the addons path.

use odoo_lint::definition::{definition, Definition};
use odoo_lint::linter::modules_of;
use odoo_lint::sources::Sources;
use std::fs;
use std::path::{Path, PathBuf};

fn write(root: &Path, path: &str, contents: &str) -> PathBuf {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}

/// A miniature Odoo in `core/` and a module `acme` in `addons/`.
fn setup(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let core = root.join("core");
    write(
        &core,
        "base/__manifest__.py",
        "{'name': 'base', 'data': ['data/groups.xml']}\n",
    );
    write(&core, "base/__init__.py", "");
    write(
        &core,
        "base/models.py",
        "from odoo import fields, models\n\n\nclass Partner(models.Model):\n    _name = 'res.partner'\n\n    name = fields.Char()\n    country_id = fields.Many2one('res.country')\n\n\nclass Country(models.Model):\n    _name = 'res.country'\n\n    code = fields.Char()\n",
    );
    write(
        &core,
        "base/data/groups.xml",
        "<odoo>\n\n    <record id=\"group_user\" model=\"res.groups\"/>\n</odoo>\n",
    );
    write(
        &core,
        "sale/__manifest__.py",
        "{'name': 'sale', 'depends': ['base'], 'data': ['views/views.xml']}\n",
    );
    write(&core, "sale/__init__.py", "");
    write(
        &core,
        "sale/models.py",
        "from odoo import fields, models\n\n\nclass Order(models.Model):\n    _name = 'sale.order'\n\n    partner_id = fields.Many2one('res.partner')\n    order_line = fields.One2many('sale.order.line', 'order_id')\n\n\nclass Line(models.Model):\n    _name = 'sale.order.line'\n\n    order_id = fields.Many2one('sale.order')\n    qty = fields.Float()\n",
    );
    write(
        &core,
        "sale/views/views.xml",
        "<odoo>\n    <record id=\"view_order_form\" model=\"ir.ui.view\">\n        <field name=\"model\">sale.order</field>\n    </record>\n    <record id=\"action_orders\" model=\"ir.actions.act_window\"/>\n</odoo>\n",
    );
    let addons = root.join("addons");
    write(
        &addons,
        "acme/__manifest__.py",
        "{'name': 'acme', 'depends': ['sale'], 'data': ['views/views.xml']}\n",
    );
    write(&addons, "acme/__init__.py", "");
    let python = write(
        &addons,
        "acme/models.py",
        r#"from odoo import api, fields, models


class Order(models.Model):
    _inherit = "sale.order"

    country = fields.Char(related="partner_id.country_id.code")

    @api.depends("partner_id.country_id")
    def _compute_x(self):
        return self.env["res.partner"], self.env.ref("base.group_user")
"#,
    );
    let views = write(
        &addons,
        "acme/views/views.xml",
        r#"<odoo>
    <record id="view_order_form_inherit" model="ir.ui.view">
        <field name="model">sale.order</field>
        <field name="inherit_id" ref="sale.view_order_form"/>
        <field name="arch" type="xml">
            <form>
                <field name="partner_id" groups="base.group_user"/>
                <field name="order_line"><list><field name="qty"/></list></field>
                <button name="%(sale.action_orders)d" type="action"/>
            </form>
        </field>
    </record>
    <menuitem id="menu_orders" action="sale.action_orders"/>
    <record id="partner_x" model="res.partner">
        <field name="country_id" eval="ref('base.group_user')"/>
    </record>
</odoo>
"#,
    );
    (core, python, views)
}

/// The definition of the first occurrence of `needle` in `file` (cursor
/// `inside` characters into it).
fn go(core: &Path, file: &Path, needle: &str, inside: usize) -> Option<(String, usize)> {
    let text = fs::read_to_string(file).unwrap();
    let offset = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} not in {}", file.display()))
        + inside;
    let module = modules_of(&[file.to_path_buf()], &Sources::default())
        .into_iter()
        .next()
        .unwrap();
    definition(&module, &[core.to_path_buf()], file, &text, offset).map(|Definition { path, line }| {
        // Definitions use canonical paths (`/private/var` on macOS).
        let root = core.parent().unwrap().canonicalize().unwrap();
        let relative = path
            .canonicalize()
            .unwrap()
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        (relative, line)
    })
}

fn at(path: &str, line: usize) -> Option<(String, usize)> {
    Some((path.to_string(), line))
}

#[test]
fn from_xml() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _, views) = setup(dir.path());
    assert_eq!(
        go(&core, &views, "sale.view_order_form\"", 3),
        at("core/sale/views/views.xml", 2)
    );
    assert_eq!(
        go(&core, &views, "base.group_user\"/>", 3),
        at("core/base/data/groups.xml", 3)
    );
    assert_eq!(
        go(&core, &views, "sale.action_orders)d", 3),
        at("core/sale/views/views.xml", 5)
    );
    assert_eq!(
        go(&core, &views, "sale.action_orders\"/>", 3),
        at("core/sale/views/views.xml", 5)
    );
    assert_eq!(
        go(&core, &views, "base.group_user')", 3),
        at("core/base/data/groups.xml", 3)
    );
    // Models.
    assert_eq!(go(&core, &views, "sale.order</field>", 2), at("core/sale/models.py", 4));
    assert_eq!(go(&core, &views, "res.partner\">", 2), at("core/base/models.py", 4));
    // Fields: of the view's model, of a sub-view's comodel, of a record.
    assert_eq!(
        go(&core, &views, "partner_id\" groups", 2),
        at("core/sale/models.py", 7)
    );
    assert_eq!(go(&core, &views, "qty\"", 1), at("core/sale/models.py", 15));
    assert_eq!(go(&core, &views, "country_id\" eval", 2), at("core/base/models.py", 8));
}

#[test]
fn from_python() {
    let dir = tempfile::tempdir().unwrap();
    let (core, python, _) = setup(dir.path());
    assert_eq!(go(&core, &python, "sale.order\"", 2), at("core/sale/models.py", 4));
    assert_eq!(go(&core, &python, "res.partner\"]", 2), at("core/base/models.py", 4));
    assert_eq!(
        go(&core, &python, "base.group_user\")", 2),
        at("core/base/data/groups.xml", 3)
    );
    // Each segment of a path: `partner_id`, then `country_id` on res.partner.
    assert_eq!(
        go(&core, &python, "partner_id.country_id\")", 2),
        at("core/sale/models.py", 7)
    );
    assert_eq!(
        go(&core, &python, "partner_id.country_id\")", 13),
        at("core/base/models.py", 8)
    );
    assert_eq!(
        go(&core, &python, "partner_id.country_id.code", 24),
        at("core/base/models.py", 14)
    );
    // Nothing to go to.
    assert_eq!(go(&core, &python, "_compute_x", 2), None);
}
