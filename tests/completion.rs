//! Completion of XML ids, models and fields across the addons path.

use odoo_lint::completion::{completion, Kind};
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

/// A miniature Odoo in `core/` and a module `acme` (depending on `sale`)
/// in `addons/`; returns the core and acme's folder.
fn setup(root: &Path) -> (PathBuf, PathBuf) {
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
        "<odoo>\n    <record id=\"group_user\" model=\"res.groups\"/>\n</odoo>\n",
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
        "<odoo>\n    <record id=\"view_order_form\" model=\"ir.ui.view\">\n        <field name=\"model\">sale.order</field>\n    </record>\n</odoo>\n",
    );
    let addons = root.join("addons");
    write(
        &addons,
        "acme/__manifest__.py",
        "{'name': 'acme', 'depends': ['sale'], 'data': ['views/views.xml']}\n",
    );
    write(&addons, "acme/__init__.py", "");
    write(
        &addons,
        "acme/views/views.xml",
        "<odoo>\n    <record id=\"acme_action\" model=\"ir.actions.act_window\"/>\n</odoo>\n",
    );
    (core, addons.join("acme"))
}

/// Completes at the `|` in `text`, as the content of `file` in acme: the
/// labels of the given kind, and the text the completion replaces.
fn complete(root: &Path, file: &str, text: &str) -> (Vec<(String, Kind)>, String) {
    let (core, acme) = setup(root);
    let offset = text.find('|').unwrap();
    let text = text.replacen('|', "", 1);
    let path = write(&acme, file, &text);
    let module = modules_of(std::slice::from_ref(&path), &Sources::default()).remove(0);
    let found = completion(&module, &[core], &path, &text, offset).expect("a completion");
    let labels = found.candidates.into_iter().map(|c| (c.label, c.kind)).collect();
    (labels, text[found.start..offset].to_string())
}

fn names(found: &[(String, Kind)]) -> Vec<&str> {
    found.iter().map(|(label, _)| label.as_str()).collect()
}

#[test]
fn fields_along_a_path_in_python() {
    let dir = tempfile::tempdir().unwrap();
    let (found, typed) = complete(
        dir.path(),
        "models.py",
        "from odoo import fields, models\n\n\nclass Order(models.Model):\n    _inherit = 'sale.order'\n\n    code = fields.Char(related='partner_id.co|')\n",
    );
    assert_eq!(typed, "co");
    assert!(names(&found).contains(&"country_id"), "{found:?}");
    assert!(names(&found).contains(&"name"));
    assert!(!names(&found).contains(&"order_line"));
}

#[test]
fn models_in_an_unclosed_string() {
    let dir = tempfile::tempdir().unwrap();
    let (found, typed) = complete(
        dir.path(),
        "models.py",
        "from odoo import models\n\n\nclass Order(models.Model):\n    _inherit = 'sale.order'\n\n    def x(self):\n        return self.env['res.|\n",
    );
    assert_eq!(typed, "res.");
    assert!(found.iter().all(|(_, kind)| *kind == Kind::Model));
    assert!(names(&found).contains(&"res.partner"));
    assert!(names(&found).contains(&"sale.order.line"));
}

#[test]
fn fields_of_a_sub_view_while_typing() {
    let dir = tempfile::tempdir().unwrap();
    let view = "<odoo>\n    <record id=\"order_form\" model=\"ir.ui.view\">\n        <field name=\"model\">sale.order</field>\n        <field name=\"arch\" type=\"xml\">\n            <form>\n                <field name=\"order_line\"><list><field name=\"q|</list></field>\n            </form>\n        </field>\n    </record>\n</odoo>\n";
    let (found, typed) = complete(dir.path(), "views/views.xml", view);
    assert_eq!(typed, "q");
    assert!(names(&found).contains(&"qty"), "{found:?}");
    assert!(!names(&found).contains(&"partner_id"));
}

#[test]
fn xml_ids_and_models_in_xml() {
    let dir = tempfile::tempdir().unwrap();
    let (found, typed) = complete(
        dir.path(),
        "views/views.xml",
        "<odoo>\n    <record id=\"x\" model=\"ir.ui.view\">\n        <field name=\"inherit_id\" ref=\"sale.|\"/>\n    </record>\n    <record id=\"acme_action\" model=\"ir.actions.act_window\"/>\n</odoo>\n",
    );
    assert_eq!(typed, "sale.");
    assert!(names(&found).contains(&"sale.view_order_form"), "{found:?}");
    assert!(names(&found).contains(&"base.group_user"));
    // The module's own ids, as it writes them.
    assert!(names(&found).contains(&"acme_action"));

    let dir = tempfile::tempdir().unwrap();
    let (found, _) = complete(
        dir.path(),
        "views/views.xml",
        "<odoo>\n    <record id=\"p\" model=\"res.|\"/>\n</odoo>\n",
    );
    assert!(names(&found).contains(&"res.country"));
}

#[test]
fn fields_of_records_after_a_dot() {
    let dir = tempfile::tempdir().unwrap();
    let (found, typed) = complete(
        dir.path(),
        "models.py",
        "from odoo import models\n\n\nclass Order(models.Model):\n    _inherit = 'sale.order'\n\n    def action_x(self):\n        for order in self:\n            order.|\n",
    );
    assert_eq!(typed, "");
    assert!(names(&found).contains(&"partner_id"), "{found:?}");
    assert!(names(&found).contains(&"order_line"));

    let (found, typed) = complete(
        dir.path(),
        "models.py",
        "from odoo import models\n\n\nclass Order(models.Model):\n    _inherit = 'sale.order'\n\n    def action_x(self):\n        return self.partner_id.co|\n",
    );
    assert_eq!(typed, "co");
    assert!(names(&found).contains(&"country_id"), "{found:?}");
    assert!(!names(&found).contains(&"order_line"));
}
