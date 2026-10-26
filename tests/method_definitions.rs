//! Go to definition and find references for methods: calls on records,
//! the methods fields name and `<button type="object">`.

mod common;

use common::write;
use odoo_lint::definition::{definition, Definition};
use odoo_lint::linter::modules_of;
use odoo_lint::sources::Sources;
use std::fs;
use std::path::{Path, PathBuf};

const SALE: &str = "from odoo import fields, models


class Order(models.Model):
    _name = 'sale.order'

    order_line = fields.One2many('sale.order.line', 'order_id')

    def action_confirm(self):
        return True


class Line(models.Model):
    _name = 'sale.order.line'

    order_id = fields.Many2one('sale.order')

    def action_split(self):
        return True
";

const ACME: &str = r#"from odoo import fields, models


class Order(models.Model):
    _inherit = "sale.order"

    total = fields.Float(compute="_compute_total")

    def _compute_total(self):
        for order in self:
            order.total = 0

    def action_confirm(self):
        self.order_line.action_split()
        return super().action_confirm()

    def action_all(self):
        for order in self:
            order.action_confirm()
"#;

const VIEWS: &str = r#"<odoo>
    <record id="view_order_form" model="ir.ui.view">
        <field name="model">sale.order</field>
        <field name="arch" type="xml">
            <form>
                <button name="action_confirm" type="object"/>
                <field name="order_line">
                    <list><button name="action_split" type="object"/></list>
                </field>
            </form>
        </field>
    </record>
</odoo>
"#;

fn setup(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let core = root.join("core");
    write(&core, "base/__manifest__.py", "{'name': 'base'}\n");
    write(&core, "base/__init__.py", "");
    write(&core, "sale/__manifest__.py", "{'name': 'sale', 'depends': ['base']}\n");
    write(&core, "sale/__init__.py", "");
    write(&core, "sale/models.py", SALE);
    let addons = root.join("addons");
    write(
        &addons,
        "acme/__manifest__.py",
        "{'name': 'acme', 'depends': ['sale'], 'data': ['views.xml']}\n",
    );
    write(&addons, "acme/__init__.py", "");
    let python = write(&addons, "acme/models.py", ACME);
    let views = write(&addons, "acme/views.xml", VIEWS);
    (core, python, views)
}

fn relative(core: &Path, path: &Path) -> String {
    let root = core.parent().unwrap().canonicalize().unwrap();
    path.canonicalize()
        .unwrap()
        .strip_prefix(&root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

/// The definition of the name at the end of the first `needle` in `file`.
fn go(core: &Path, file: &Path, needle: &str) -> Option<String> {
    let text = fs::read_to_string(file).unwrap();
    let offset = text.find(needle).unwrap_or_else(|| panic!("{needle} not found")) + needle.len() - 1;
    let module = modules_of(&[file.to_path_buf()], &Sources::default()).remove(0);
    definition(&module, &[core.to_path_buf()], file, &text, offset)
        .map(|Definition { path, line }| format!("{}:{line}", relative(core, &path)))
}

#[test]
fn definitions_of_methods() {
    let dir = tempfile::tempdir().unwrap();
    let (core, python, views) = setup(dir.path());
    // `compute=` names a method of the class's model.
    assert_eq!(
        go(&core, &python, "compute=\"_compute_total").as_deref(),
        Some("addons/acme/models.py:9")
    );
    // Calls on records: through a path, on `super()`, in a loop.
    assert_eq!(
        go(&core, &python, "order_line.action_split").as_deref(),
        Some("core/sale/models.py:18")
    );
    assert_eq!(
        go(&core, &python, "super().action_confirm").as_deref(),
        Some("core/sale/models.py:9")
    );
    assert_eq!(
        go(&core, &python, "order.action_confirm").as_deref(),
        Some("core/sale/models.py:9")
    );
    // An override goes to the method it overrides.
    assert_eq!(
        go(&core, &python, "def action_confirm").as_deref(),
        Some("core/sale/models.py:9")
    );
    // Buttons, in a sub-view too.
    assert_eq!(
        go(&core, &views, "\"action_confirm").as_deref(),
        Some("core/sale/models.py:9")
    );
    assert_eq!(
        go(&core, &views, "\"action_split").as_deref(),
        Some("core/sale/models.py:18")
    );
}

#[test]
fn references_of_a_method() {
    let dir = tempfile::tempdir().unwrap();
    let (core, python, _) = setup(dir.path());
    let sale = core.join("sale/models.py");
    let text = fs::read_to_string(&sale).unwrap();
    let offset = text.find("action_confirm").unwrap() + 2;
    let module = modules_of(std::slice::from_ref(&sale), &Sources::default()).remove(0);
    let addons = python.parent().unwrap().parent().unwrap().to_path_buf();
    let found: Vec<String> =
        odoo_lint::usages::references(&module, &[core.clone(), addons], &sale, &text, offset, true)
            .into_iter()
            .map(|usage| {
                let content = fs::read_to_string(&usage.path).unwrap();
                assert_eq!(&content[usage.start..usage.end], "action_confirm");
                let line = content[..usage.start].matches('\n').count() + 1;
                format!("{}:{line}", relative(&core, &usage.path))
            })
            .collect();
    assert_eq!(
        found,
        vec![
            // The override, `super()`'s call and the call in the loop.
            "addons/acme/models.py:13",
            "addons/acme/models.py:15",
            "addons/acme/models.py:19",
            "addons/acme/views.xml:6",
            "core/sale/models.py:9",
        ]
    );
}
