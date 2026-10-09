//! ODOO005 (XML ids) and ODOO006 (models) across the addons path.

use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A module with the given manifest keys (`'data': [...]` and so on).
fn module(root: &Path, name: &str, depends: &[&str], keys: &str, python: &str) {
    let depends: Vec<String> = depends.iter().map(|d| format!("'{d}'")).collect();
    write(
        root,
        &format!("{name}/__manifest__.py"),
        &format!("{{'name': '{name}', 'depends': [{}], {keys}}}\n", depends.join(", ")),
    );
    write(root, &format!("{name}/__init__.py"), "from . import models\n");
    write(root, &format!("{name}/models.py"), python);
}

/// A miniature Odoo: `base` with groups and partners, `sale` with orders,
/// and `stock`, which `acme` does not depend on.
fn odoo(root: &Path) -> PathBuf {
    let core = root.join("core");
    module(
        &core,
        "base",
        &[],
        "'data': ['data/groups.xml']",
        "from odoo import fields, models\n\n\nclass Partner(models.Model):\n    _name = 'res.partner'\n    name = fields.Char()\n\n\nclass Users(models.Model):\n    _name = 'res.users'\n    _inherits = {'res.partner': 'partner_id'}\n\n\nclass Groups(models.Model):\n    _name = 'res.groups'\n\n\nclass View(models.Model):\n    _name = 'ir.ui.view'\n\n\nclass Action(models.Model):\n    _name = 'ir.actions.act_window'\n\n\nclass Rule(models.Model):\n    _name = 'ir.rule'\n",
    );
    write(
        &core,
        "base/data/groups.xml",
        "<odoo>\n    <record id=\"group_user\" model=\"res.groups\"><field name=\"name\">User</field></record>\n    <record id=\"user_demo\" model=\"res.users\"><field name=\"login\">demo</field></record>\n</odoo>\n",
    );
    module(
        &core,
        "sale",
        &["base"],
        "'data': ['views/views.xml']",
        "from odoo import fields, models\n\n\nclass Order(models.Model):\n    _name = 'sale.order'\n    name = fields.Char()\n",
    );
    write(
        &core,
        "sale/views/views.xml",
        "<odoo>\n    <record id=\"view_order_form\" model=\"ir.ui.view\"><field name=\"model\">sale.order</field></record>\n    <function model=\"sale.order\" name=\"_load_records\"><value eval=\"[{'xml_id': 'sale.order_demo', 'values': {}}]\"/></function>\n</odoo>\n",
    );
    module(
        &core,
        "stock",
        &["base"],
        "'data': ['security/groups.xml']",
        "from odoo import fields, models\n\n\nclass Picking(models.Model):\n    _name = 'stock.picking'\n    name = fields.Char()\n",
    );
    write(
        &core,
        "stock/security/groups.xml",
        "<odoo>\n    <record id=\"group_stock_user\" model=\"res.groups\"><field name=\"name\">Stock</field></record>\n</odoo>\n",
    );
    core
}

fn messages(root: &Path, addons_path: Vec<PathBuf>) -> Vec<(String, usize, String)> {
    let mut settings = Settings::default();
    settings.select = vec!["ODOO005".into(), "ODOO006".into()];
    settings.addons_path = addons_path;
    let mut found: Vec<(String, usize, String)> = lint_paths(&[root.to_path_buf()], &settings)
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
fn references_across_modules() {
    let dir = tempfile::tempdir().unwrap();
    let core = odoo(dir.path());
    let addons = dir.path().join("addons");
    module(
        &addons,
        "acme",
        &["sale"],
        "'data': ['security/ir.model.access.csv', 'views/views.xml', 'security/groups.xml'], 'demo': ['demo/demo.xml']",
        "from odoo import fields, models\n\n\nclass Order(models.Model):\n    _inherit = ['sale.order', 'stock.picking']\n\n\nclass Ticket(models.Model):\n    _name = 'acme.ticket'\n    name = fields.Char()\n\n    def action(self):\n        self.env.ref('stock.group_stock_user')\n        self.env.ref('sale.nope')\n        self.env.ref('stock.nope', raise_if_not_found=False)\n        return self.env.user.has_group('base.group_user')\n",
    );
    write(
        &addons,
        "acme/security/ir.model.access.csv",
        "id,name,model_id:id,group_id:id,perm_read\naccess_ticket,ticket,model_acme_ticket,base.group_user,1\naccess_typo,typo,model_acme_tickett,group_acme,1\n",
    );
    write(
        &addons,
        "acme/views/views.xml",
        r#"<odoo>
    <record id="view_ticket" model="ir.ui.view">
        <field name="model">acme.ticket</field>
        <field name="inherit_id" ref="sale.view_order_form"/>
        <field name="arch" type="xml">
            <form><button name="%(action_ticket)d" type="action" groups="base.group_user,!stock.group_stock_user"/></form>
        </field>
    </record>
    <record id="action_ticket" model="ir.actions.act_window">
        <field name="res_model">acme.unknown</field>
        <field name="binding_model_id" ref="model_acme_ticket"/>
        <field name="field_id" ref="field_acme_ticket__id"/>
    </record>
    <record id="partner_rule" model="ir.rule">
        <field name="partner_id" ref="base.user_demo_res_partner"/>
        <field name="groups" eval="[(4, ref('group_acme')), (4, ref('demo_only')), (4, ref('base.gone', False))]"/>
    </record>
    <template id="ticket_page"><t t-call="sale.missing_template"/><t t-call="Inline"/><t t-call="stock.page"/></template>
</odoo>
"#,
    );
    write(
        &addons,
        "acme/security/groups.xml",
        "<odoo>\n    <record id=\"group_acme\" model=\"res.groups\"><field name=\"name\">Acme</field></record>\n</odoo>\n",
    );
    write(
        &addons,
        "acme/demo/demo.xml",
        "<odoo>\n    <record id=\"demo_only\" model=\"res.groups\"><field name=\"name\">Demo</field></record>\n</odoo>\n",
    );
    let found = messages(&addons, vec![core]);
    let expected: Vec<(String, usize, String)> = [
        (
            "ir.model.access.csv",
            3,
            "XML id `acme.group_acme` is defined in `security/groups.xml`, which Odoo loads later",
        ),
        (
            "ir.model.access.csv",
            3,
            "XML id `acme.model_acme_tickett` does not exist",
        ),
        (
            "models.py",
            4,
            "Model `stock.picking` comes from `stock`, which `depends` does not reach",
        ),
        // Python looks references up when it runs: `stock.group_stock_user`
        // (outside depends) may be guarded; `sale.nope` cannot exist.
        ("models.py", 14, "XML id `sale.nope` does not exist"),
        (
            "views.xml",
            6,
            "XML id `acme.action_ticket` is defined further down in this file; Odoo loads it later",
        ),
        (
            "views.xml",
            6,
            "XML id `stock.group_stock_user` comes from `stock`, which `depends` does not reach",
        ),
        (
            "views.xml",
            10,
            "Model `acme.unknown` does not exist in the modules `depends` reaches",
        ),
        (
            "views.xml",
            16,
            "XML id `acme.demo_only` is defined in `demo/demo.xml`, which Odoo loads later",
        ),
        (
            "views.xml",
            16,
            "XML id `acme.group_acme` is defined in `security/groups.xml`, which Odoo loads later",
        ),
        ("views.xml", 18, "XML id `sale.missing_template` does not exist"),
    ]
    .into_iter()
    .map(|(f, l, m)| (f.to_string(), l, m.to_string()))
    .collect();
    assert_eq!(found, expected);
}

#[test]
fn silent_without_all_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let core = odoo(dir.path());
    let addons = dir.path().join("addons");
    module(
        &addons,
        "acme",
        &["sale", "elsewhere"],
        "'data': []",
        "from odoo import models\n\n\nclass O(models.Model):\n    _inherit = 'nothing.here'\n",
    );
    assert!(messages(&addons, vec![core]).is_empty());
}
