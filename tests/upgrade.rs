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

const V19: OdooVersion = OdooVersion::new(19, 0);

const VIEWS_18: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<odoo>
    <record id="group_manager" model="res.groups">
        <field name="name">Manager</field>
        <field name="users" eval="[(4, ref('base.user_admin'))]"/>
    </record>
    <record id="menu_up" model="ir.ui.menu">
        <field name="name">Up</field>
        <field name="groups_id" eval="[(4, ref('group_manager'))]"/>
    </record>
    <record id="view_users_form" model="ir.ui.view">
        <field name="model">res.users</field>
        <field name="inherit_id" ref="base.view_users_form"/>
        <field name="arch" type="xml">
            <xpath expr="//field[@name='groups_id']" position="after">
                <field name="groups_id" groups="base.group_system"/>
            </xpath>
        </field>
    </record>
    <record id="view_partner_search" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="arch" type="xml">
            <search>
                <group expand="0" string="Group By">
                    <filter name="by_city" context="{'group_by': 'city'}"/>
                </group>
            </search>
        </field>
    </record>
    <record id="view_partner_form" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="arch" type="xml">
            <form>
                <field name="mobile"/>
                <div t-call="acme_up.card"/>
            </form>
        </field>
    </record>
</odoo>
"#;

const PYTHON_18: &str = r#"from odoo import api, fields, http, models
from odoo.http import request
from odoo.models import NewId
from odoo.osv import expression


class Partner(models.Model):
    _inherit = "res.partner"

    _sql_constraints = [
        ("ref_uniq", "unique(ref)", "The reference must be unique."),
    ]

    parent_id = fields.Many2one("res.partner", auto_join=True)

    @api.model
    def create(self, vals):
        return super().create(vals)

    @api.returns("self")
    def copy(self, default=None):
        return super().copy(default)

    def read_group(self, domain, fields, groupby, **kwargs):
        return super().read_group(domain, fields, groupby, **kwargs)

    def action_reset(self):
        uid = self._uid
        self.clear_caches()
        code = self.env["ir.sequence"].get("acme.up")
        users = self.env["res.users"].search([("groups_id", "<>", False), ("name", "ILIKE", "a")])
        self.env["res.partner"].name_search("a", args=[])
        return uid, code, users


class Controller(http.Controller):
    @http.route("/up", type="json", auth="user")
    def up(self):
        return request.uid
"#;

#[test]
fn findings_for_odoo_19() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "19.0.1.0.0", &[("views/views.xml", VIEWS_18)], PYTHON_18);
    write(
        dir.path(),
        "acme_up/__manifest__.py",
        "{'name': 'Up', 'version': '19.0.1.0.0', 'license': 'AGPL-3', \
         'data': ['views/views.xml'], 'update_xml': []}\n",
    );
    let mut found = codes(dir.path(), "U19", V19);
    found.sort();
    let expected: Vec<(String, usize)> = [
        ("U1901", 28),
        ("U1901", 39),
        ("U1902", 10),
        ("U1903", 17),
        ("U1904", 20),
        ("U1905", 24),
        ("U1906", 37),
        ("U1907", 31),
        ("U1908", 4),
        ("U1909", 14),
        ("U1910", 32),
        ("U1911", 29),
        ("U1913", 30),
        ("U1914", 31),
        ("U1914", 31),
        ("U1915", 3),
        ("U1916", 9),
        ("U1916", 15),
        ("U1916", 16),
        ("U1917", 24),
        ("U1918", 5),
        ("U1919", 1),
        ("U1920", 35),
        ("U1921", 34),
    ]
    .into_iter()
    .map(|(c, l)| (c.to_string(), l))
    .collect();
    assert_eq!(found, expected);
}

#[test]
fn fixes_for_odoo_19() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path(), "19.0.1.0.0", &[("views/views.xml", VIEWS_18)], PYTHON_18);
    let xml = fixed(dir.path(), "U19", V19, &module.join("views/views.xml"), FixMode::Safe);
    let expected = VIEWS_18
        .replace("name=\"users\"", "name=\"user_ids\"")
        .replace("name=\"groups_id\"", "name=\"group_ids\"")
        .replace("@name='groups_id'", "@name='group_ids'")
        .replace("<group expand=\"0\" string=\"Group By\">", "<group>");
    assert_eq!(xml, expected);

    let python = fixed(dir.path(), "U19", V19, &module.join("models/partner.py"), FixMode::Safe);
    let expected = PYTHON_18
        .replace(
            "    _sql_constraints = [\n        (\"ref_uniq\", \"unique(ref)\", \"The reference must be unique.\"),\n    ]",
            "    _ref_uniq = models.Constraint(\"unique(ref)\", \"The reference must be unique.\")",
        )
        .replace("auto_join=", "bypass_search_access=")
        .replace("    @api.returns(\"self\")\n", "")
        .replace("self._uid", "self.env.uid")
        .replace("self.clear_caches()", "self.env.registry.clear_cache()")
        .replace(".get(\"acme.up\")", ".next_by_code(\"acme.up\")")
        .replace("\"<>\"", "\"!=\"")
        .replace("\"ILIKE\"", "\"ilike\"")
        .replace("args=[]", "domain=[]")
        .replace("type=\"json\"", "type=\"jsonrpc\"")
        .replace("request.uid", "request.env.uid")
        .replace("from odoo.models import NewId", "from odoo.api import NewId");
    assert_eq!(python, expected);
}

#[test]
fn modules_still_on_18_are_left_alone_by_u19() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "18.0.1.0.0", &[("views/views.xml", VIEWS_18)], PYTHON_18);
    assert!(codes(dir.path(), "U19", V18).is_empty());
}

const V17: OdooVersion = OdooVersion::new(17, 0);

const VIEWS_16: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<odoo>
    <record id="view_order_form" model="ir.ui.view">
        <field name="model">sale.order</field>
        <field name="arch" type="xml">
            <form>
                <header>
                    <button name="action_confirm" states="draft,sent"/>
                    <button name="action_done" states="sale" attrs="{'invisible': ['|', ('locked', '=', True)]}"/>
                </header>
                <field name="partner_id" attrs="{'readonly': [('state', '!=', 'draft')], 'required': [('type', '=', 'b2b')]}"/>
                <field name="note" invisible="0" attrs="{'invisible': [('line_ids', '=', [])]}"/>
                <field name="line_ids">
                    <tree>
                        <field name="sequence" invisible="1"/>
                        <field name="discount" invisible="1" attrs="{'readonly': [('parent.state', 'in', ('sale', 'done'))], 'invisible': [('display_type', '!=', False)]}"/>
                    </tree>
                </field>
                <field name="origin" attrs="{'invisible': [('origin', 'ilike', 'x')]}"/>
                <field name="user_id" context="{'default_partner_id': active_id}"/>
            </form>
        </field>
    </record>
    <record id="view_order_form_inherit" model="ir.ui.view">
        <field name="model">sale.order</field>
        <field name="inherit_id" ref="sale.view_order_form"/>
        <field name="arch" type="xml">
            <xpath expr="//field[@name='origin']" position="attributes">
                <attribute name="attrs">{'invisible': [('state', '=', 'cancel')], 'readonly': True}</attribute>
            </xpath>
        </field>
    </record>
    <record id="view_order_calendar" model="ir.ui.view">
        <field name="model">sale.order</field>
        <field name="field_parent">child_ids</field>
        <field name="arch" type="xml">
            <calendar date_start="date_order" quick_add="False"/>
        </field>
    </record>
    <report id="report_order" model="sale.order" string="Order" name="acme_up.report_order"/>
</odoo>
"#;

const PYTHON_16: &str = r#"from odoo import api, fields, models
from odoo.exceptions import Warning
from odoo.tests.common import SavepointCase


class Order(models.Model):
    _inherit = "sale.order"

    note = fields.Text(states={"draft": [("readonly", False)]})

    def name_get(self):
        return [(r.id, r.name) for r in self]

    def _name_search(self, name, args=None, operator="ilike", limit=100, name_get_uid=None):
        return super()._name_search(name, args, operator, limit, name_get_uid)

    @api.onchange("partner_id")
    def _onchange_partner(self):
        return {"domain": {"user_id": [("share", "=", False)]}}

    def action_count(self):
        self.flush()
        label = self.partner_id.name_get()[0][1]
        count = self.env["sale.order"].search([("state", "=", "sale")], count=True)
        value = self.env["ir.default"].get("sale.order", "note")
        return label, count, value, self.env.norecompute()


class TestOrder(SavepointCase):
    pass
"#;

#[test]
fn findings_for_odoo_17() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "17.0.1.0.0", &[("views/order.xml", VIEWS_16)], PYTHON_16);
    let mut found = codes(dir.path(), "U17", V17);
    found.sort();
    let expected: Vec<(String, usize)> = [
        ("U1701", 8),
        ("U1701", 9),
        ("U1701", 11),
        ("U1701", 12),
        ("U1701", 16),
        ("U1701", 19),
        ("U1701", 29),
        ("U1702", 15),
        ("U1702", 16),
        ("U1703", 40),
        ("U1704", 37),
        ("U1705", 20),
        ("U1707", 35),
        ("U1709", 11),
        ("U1709", 23),
        ("U1710", 14),
        ("U1711", 24),
        ("U1712", 22),
        ("U1713", 9),
        ("U1714", 3),
        ("U1714", 29),
        ("U1715", 2),
        ("U1716", 19),
        ("U1717", 26),
        ("U1718", 25),
    ]
    .into_iter()
    .map(|(c, l)| (c.to_string(), l))
    .collect();
    assert_eq!(found, expected);
}

#[test]
fn fixes_for_odoo_17() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path(), "17.0.1.0.0", &[("views/order.xml", VIEWS_16)], PYTHON_16);
    let views = module.join("views/order.xml");
    let xml = fixed(dir.path(), "U17", V17, &views, FixMode::Unsafe);
    let expected = VIEWS_16
        .replace(r#"states="draft,sent""#, r#"invisible="state not in ('draft', 'sent')""#)
        .replace(
            r#"states="sale" attrs="{'invisible': ['|', ('locked', '=', True)]}""#,
            r#"invisible="locked or state != 'sale'""#,
        )
        .replace(
            r#"attrs="{'readonly': [('state', '!=', 'draft')], 'required': [('type', '=', 'b2b')]}""#,
            r#"readonly="state != 'draft'" required="type == 'b2b'""#,
        )
        .replace(
            r#"invisible="0" attrs="{'invisible': [('line_ids', '=', [])]}""#,
            r#"invisible="not line_ids""#,
        )
        .replace(
            r#"<field name="sequence" invisible="1"/>"#,
            r#"<field name="sequence" column_invisible="1"/>"#,
        )
        .replace(
            r#"invisible="1" attrs="{'readonly': [('parent.state', 'in', ('sale', 'done'))], 'invisible': [('display_type', '!=', False)]}""#,
            r#"column_invisible="1" readonly="parent.state in ('sale', 'done')" invisible="display_type""#,
        )
        .replace("{'default_partner_id': active_id}", "{'default_partner_id': context.get('active_id')}")
        .replace(
            r#"<attribute name="attrs">{'invisible': [('state', '=', 'cancel')], 'readonly': True}</attribute>"#,
            "<attribute name=\"invisible\">state == 'cancel'</attribute>\n                <attribute name=\"readonly\">True</attribute>",
        )
        .replace("        <field name=\"field_parent\">child_ids</field>\n", "")
        .replace(r#"quick_add="False""#, r#"quick_create="False""#);
    assert_eq!(xml, expected);

    let python = fixed(dir.path(), "U17", V17, &module.join("models/partner.py"), FixMode::Safe);
    let expected = PYTHON_16
        .replace("SavepointCase", "TransactionCase")
        .replace("self.flush()", "self.env.flush_all()")
        .replace("self.partner_id.name_get()[0][1]", "self.partner_id.display_name")
        .replace(
            ".search([(\"state\", \"=\", \"sale\")], count=True)",
            ".search_count([(\"state\", \"=\", \"sale\")])",
        )
        .replace(".get(\"sale.order\", \"note\")", "._get(\"sale.order\", \"note\")");
    assert_eq!(python, expected);
}

const V16: OdooVersion = OdooVersion::new(16, 0);

const VIEWS_15: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<odoo>
    <record id="view_partner_form" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="inherit_id" ref="base.view_partner_form"/>
        <field name="groups_id" eval="[(4, ref('base.group_system'))]"/>
        <field name="arch" type="xml">
            <field name="vat" position="after"><field name="ref"/></field>
        </field>
    </record>
    <record id="view_partner_admin" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="inherit_id" ref="base.view_partner_form"/>
        <field name="mode">primary</field>
        <field name="groups_id" eval="[(4, ref('base.group_system'))]"/>
        <field name="arch" type="xml">
            <field name="vat" position="after"><field name="ref"/></field>
        </field>
    </record>
    <template id="portal_layout" inherit_id="portal.portal_layout" groups="base.group_user">
        <xpath expr="//div" position="inside"><span>Hi</span></xpath>
    </template>
    <record id="mail_template_order" model="mail.template">
        <field name="name">Order</field>
        <field name="body_html" type="xml"><div>Hello</div></field>
    </record>
</odoo>
"#;

const PYTHON_15: &str = r#"from odoo import http, models
from odoo.http import request
from odoo.osv.query import Query


class Partner(models.Model):
    _inherit = "res.partner"

    def fields_view_get(self, view_id=None, view_type="form", toolbar=False, submenu=False):
        return super().fields_view_get(view_id, view_type, toolbar, submenu)

    def action_translate(self):
        self.env["ir.translation"].search([])
        return self.search(args=[("name", "=", "x")], limit=1), Query


class Controller(http.Controller):
    @http.route("/up", type="json", auth="user")
    def up(self):
        data = request.jsonrequest
        request.context = dict(request.context, lang="nl_NL")
        return self.env["ir.http"].binary_content(model="res.partner"), data
"#;

#[test]
fn findings_for_odoo_16() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "16.0.1.0.0", &[("views/partner.xml", VIEWS_15)], PYTHON_15);
    write(
        dir.path(),
        "acme_up/__manifest__.py",
        "{'name': 'Up', 'version': '16.0.1.0.0', 'license': 'AGPL-3', \
         'data': ['views/partner.xml'], 'qweb': ['static/src/xml/*.xml'], \
         'assets': {'web.assets_qweb': ['acme_up/static/src/xml/*.xml'], \
         'web.assets_frontend': [('include', 'web._assets_common_styles')]}}\n",
    );
    let mut found = codes(dir.path(), "U16", V16);
    found.sort();
    let expected: Vec<(String, usize)> = [
        ("U1601", 6),
        ("U1601", 20),
        ("U1602", 25),
        ("U1603", 1),
        ("U1604", 1),
        ("U1605", 1),
        ("U1606", 13),
        ("U1607", 20),
        ("U1607", 21),
        ("U1608", 22),
        ("U1609", 14),
        ("U1610", 3),
        ("U1611", 9),
    ]
    .into_iter()
    .map(|(c, l)| (c.to_string(), l))
    .collect();
    assert_eq!(found, expected);
}

#[test]
fn fixes_for_odoo_16() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path(), "16.0.1.0.0", &[("views/partner.xml", VIEWS_15)], PYTHON_15);
    write(
        dir.path(),
        "acme_up/__manifest__.py",
        "{'name': 'Up', 'version': '16.0.1.0.0', 'license': 'AGPL-3', \
         'data': ['views/partner.xml'], 'assets': {'web.assets_qweb': ['acme_up/static/src/xml/*.xml']}}\n",
    );
    let xml = fixed(dir.path(), "U16", V16, &module.join("views/partner.xml"), FixMode::Safe);
    assert_eq!(
        xml,
        VIEWS_15.replace(r#"name="body_html" type="xml""#, r#"name="body_html" type="html""#)
    );
    let manifest = fixed(dir.path(), "U16", V16, &module.join("__manifest__.py"), FixMode::Safe);
    assert!(
        manifest.contains("'web.assets_backend': ['acme_up/static/src/xml/*.xml']"),
        "{manifest}"
    );
    let python = fixed(dir.path(), "U16", V16, &module.join("models/partner.py"), FixMode::Safe);
    let expected = PYTHON_15
        .replace("odoo.osv.query", "odoo.tools.query")
        .replace("search(args=", "search(domain=")
        .replace("request.jsonrequest", "request.get_json_data()");
    assert_eq!(python, expected);
}
