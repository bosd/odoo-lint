//! Go to definition and find references on attribute access
//! (`order.partner_id`), with the model of the records the code shows.

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

const BASE: &str = "from odoo import fields, models


class Partner(models.Model):
    _name = 'res.partner'

    name = fields.Char()
    country_id = fields.Many2one('res.country')


class Country(models.Model):
    _name = 'res.country'

    code = fields.Char()


class Users(models.Model):
    _name = 'res.users'

    partner_id = fields.Many2one('res.partner')
";

const SALE: &str = "from odoo import fields, models


class Order(models.Model):
    _name = 'sale.order'

    partner_id = fields.Many2one('res.partner')
    order_line = fields.One2many('sale.order.line', 'order_id')


class Line(models.Model):
    _name = 'sale.order.line'

    order_id = fields.Many2one('sale.order')
    qty = fields.Float()
";

const ACME: &str = r#"from odoo import models


class Order(models.Model):
    _inherit = "sale.order"

    def action_x(self):
        for order in self:
            order.partner_id.country_id.code
        lines = self.order_line.filtered(lambda l: l.qty > 0)
        partners = self.env["res.partner"].sudo().search([])
        user = self.env.user
        owners = [line.order_id.partner_id for line in lines]
        countries = self.mapped("partner_id.country_id")
        return partners.country_id, countries.code, user.partner_id, owners, lines[0].qty


def helper(order):
    return order.partner_id
"#;

fn setup(root: &Path) -> (PathBuf, PathBuf) {
    let core = root.join("core");
    write(&core, "base/__manifest__.py", "{'name': 'base'}\n");
    write(&core, "base/__init__.py", "");
    write(&core, "base/models.py", BASE);
    write(&core, "sale/__manifest__.py", "{'name': 'sale', 'depends': ['base']}\n");
    write(&core, "sale/__init__.py", "");
    write(&core, "sale/models.py", SALE);
    let addons = root.join("addons");
    write(
        &addons,
        "acme/__manifest__.py",
        "{'name': 'acme', 'depends': ['sale']}\n",
    );
    write(&addons, "acme/__init__.py", "");
    let python = write(&addons, "acme/models.py", ACME);
    (core, python)
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

/// The definition of the name at the end of the first `needle` in acme.
fn go(core: &Path, file: &Path, needle: &str) -> Option<String> {
    let text = fs::read_to_string(file).unwrap();
    let offset = text.find(needle).unwrap_or_else(|| panic!("{needle} not found")) + needle.len() - 1;
    let module = modules_of(&[file.to_path_buf()], &Sources::default()).remove(0);
    definition(&module, &[core.to_path_buf()], file, &text, offset)
        .map(|Definition { path, line }| format!("{}:{line}", relative(core, &path)))
}

#[test]
fn definitions_through_the_model_of_the_records() {
    let dir = tempfile::tempdir().unwrap();
    let (core, python) = setup(dir.path());
    let go = |needle: &str| go(&core, &python, needle);
    // `for order in self`, along a path.
    assert_eq!(go("order.partner_id").as_deref(), Some("core/sale/models.py:7"));
    assert_eq!(go("partner_id.country_id").as_deref(), Some("core/base/models.py:8"));
    assert_eq!(go("country_id.code").as_deref(), Some("core/base/models.py:14"));
    // `self`, a lambda in `filtered`, `env[...]` through `sudo().search()`.
    assert_eq!(go("self.order_line").as_deref(), Some("core/sale/models.py:8"));
    assert_eq!(go("l.qty").as_deref(), Some("core/sale/models.py:15"));
    assert_eq!(go("partners.country_id").as_deref(), Some("core/base/models.py:8"));
    // `env.user`, a comprehension over a variable, `mapped`, an index.
    assert_eq!(go("user.partner_id").as_deref(), Some("core/base/models.py:20"));
    assert_eq!(go("line.order_id").as_deref(), Some("core/sale/models.py:14"));
    assert_eq!(go("countries.code").as_deref(), Some("core/base/models.py:14"));
    assert_eq!(go("lines[0].qty").as_deref(), Some("core/sale/models.py:15"));
    // A parameter of a plain function: its model is unknown.
    assert_eq!(go("return order.partner_id"), None);
}

#[test]
fn references_include_attribute_access() {
    let dir = tempfile::tempdir().unwrap();
    let (core, python) = setup(dir.path());
    let sale = core.join("sale/models.py");
    let text = fs::read_to_string(&sale).unwrap();
    let offset = text.find("partner_id").unwrap() + 2;
    let module = modules_of(std::slice::from_ref(&sale), &Sources::default()).remove(0);
    let found: Vec<String> = odoo_lint::usages::references(
        &module,
        &[core.clone(), python.parent().unwrap().parent().unwrap().to_path_buf()],
        &sale,
        &text,
        offset,
        false,
    )
    .into_iter()
    .map(|usage| {
        let content = fs::read_to_string(&usage.path).unwrap();
        assert_eq!(&content[usage.start..usage.end], "partner_id");
        let line = content[..usage.start].matches('\n').count() + 1;
        format!("{}:{line}", relative(&core, &usage.path))
    })
    .collect();
    // In acme: `order.partner_id`, `line.order_id.partner_id` and the path
    // in `mapped`; not `user.partner_id` (res.users) nor `helper`'s.
    let acme: Vec<&String> = found.iter().filter(|f| f.starts_with("addons/")).collect();
    assert_eq!(
        acme,
        vec![
            "addons/acme/models.py:9",
            "addons/acme/models.py:13",
            "addons/acme/models.py:14"
        ],
        "{found:?}"
    );
}
