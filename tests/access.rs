//! ODOO013: one2many and many2many fields in views whose comodel only
//! administrators may read.

mod common;

use common::write;
use odoo_lint::config::OdooLintConfig;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::path::{Path, PathBuf};

const BASE_MANIFEST: &str =
    "{'name': 'base', 'depends': [], 'data': ['security/groups.xml', 'security/ir.model.access.csv', 'views/users.xml']}\n";

/// The Users form, which only the Settings menus open.
const BASE_VIEWS: &str = r#"<odoo>
    <record id="view_users_form" model="ir.ui.view">
        <field name="model">acme.product</field>
        <field name="arch" type="xml"><form><sheet/></form></field>
    </record>
</odoo>
"#;

const BASE_MODELS: &str = r#"from odoo import fields, models


class Fields(models.Model):
    _name = "ir.model.fields"


class Parameter(models.Model):
    _name = "ir.config_parameter"


class Tag(models.Model):
    _name = "res.tag"


class Employee(models.Model):
    _name = "hr.employee"

    def fetch(self, field_names):
        return super().fetch(field_names)
"#;

const GROUPS: &str = r#"<odoo>
    <record id="group_user" model="res.groups"/>
    <record id="group_erp_manager" model="res.groups"/>
    <record id="group_system" model="res.groups">
        <field name="implied_ids" eval="[Command.link(ref('group_erp_manager'))]"/>
    </record>
</odoo>
"#;

const BASE_ACCESS: &str = "id,name,model_id:id,group_id:id,perm_read,perm_write,perm_create,perm_unlink
access_fields_manager,fields manager,model_ir_model_fields,group_erp_manager,1,1,1,1
access_fields_user,fields user,model_ir_model_fields,group_user,0,0,0,0
access_parameter,parameter,model_ir_config_parameter,group_system,1,1,1,1
access_tag,tag,model_res_tag,group_user,1,0,0,0
access_employee,employee,model_hr_employee,group_system,1,1,1,1
";

const ACME_MODELS: &str = r#"from odoo import fields, models


class Product(models.Model):
    _name = "acme.product"

    field_ids = fields.Many2many("ir.model.fields")
    field_id = fields.Many2one("ir.model.fields")
    gated_ids = fields.Many2many("ir.model.fields", groups="base.group_system")
    parameter_ids = fields.Many2many("ir.config_parameter")
    tag_ids = fields.Many2many("res.tag")
    employee_ids = fields.Many2many("hr.employee")


class Secret(models.Model):
    _name = "acme.secret"

    field_ids = fields.Many2many("ir.model.fields")
"#;

const ACME_ACCESS: &str = "id,name,model_id:id,group_id:id,perm_read,perm_write,perm_create,perm_unlink
access_product,product,model_acme_product,base.group_user,1,0,0,0
access_secret,secret,model_acme_secret,base.group_erp_manager,1,1,1,1
";

const VIEWS: &str = r#"<odoo>
    <record id="product_form" model="ir.ui.view">
        <field name="model">acme.product</field>
        <field name="arch" type="xml">
            <form>
                <field name="field_ids" widget="many2many_tags"/>
                <field name="field_ids" groups="base.group_system"/>
                <group groups="base.group_system"><field name="field_ids"/></group>
                <field name="field_id"/>
                <field name="field_ids" invisible="1"/>
                <field name="parameter_ids"/>
                <field name="gated_ids"/>
                <field name="tag_ids"/>
                <field name="employee_ids"/>
            </form>
        </field>
    </record>
    <record id="product_search" model="ir.ui.view">
        <field name="model">acme.product</field>
        <field name="arch" type="xml">
            <search><field name="field_ids"/></search>
        </field>
    </record>
    <record id="secret_form" model="ir.ui.view">
        <field name="model">acme.secret</field>
        <field name="arch" type="xml">
            <form><field name="field_ids"/></form>
        </field>
    </record>
    <record id="users_form" model="ir.ui.view">
        <field name="model">acme.product</field>
        <field name="inherit_id" ref="base.view_users_form"/>
        <field name="arch" type="xml">
            <sheet position="inside"><field name="field_ids"/></sheet>
        </field>
    </record>
</odoo>
"#;

fn setup(dir: &Path) -> (PathBuf, PathBuf) {
    let core = dir.join("core");
    write(&core, "base/__manifest__.py", BASE_MANIFEST);
    write(&core, "base/__init__.py", "from . import models\n");
    write(&core, "base/models.py", BASE_MODELS);
    write(&core, "base/security/groups.xml", GROUPS);
    write(&core, "base/security/ir.model.access.csv", BASE_ACCESS);
    write(&core, "base/views/users.xml", BASE_VIEWS);
    let addons = dir.join("addons");
    write(
        &addons,
        "acme/__manifest__.py",
        "{'name': 'acme', 'depends': ['base'], 'data': ['security/ir.model.access.csv', 'views/views.xml']}\n",
    );
    write(&addons, "acme/__init__.py", "from . import models\n");
    write(&addons, "acme/models.py", ACME_MODELS);
    write(&addons, "acme/security/ir.model.access.csv", ACME_ACCESS);
    write(&addons, "acme/views/views.xml", VIEWS);
    (core, addons)
}

fn findings(addons: &Path, settings: &Settings) -> Vec<(usize, String)> {
    let mut found: Vec<(usize, String)> = lint_paths(&[addons.to_path_buf()], settings)
        .into_iter()
        .filter(|v| v.code == "ODOO013")
        .map(|v| (v.line, v.message))
        .collect();
    found.sort();
    found
}

fn settings(core: &Path, config: &str) -> Settings {
    let mut settings = Settings::default();
    settings.config = toml::from_str::<OdooLintConfig>(config).unwrap();
    settings.addons_path = vec![core.to_path_buf()];
    settings
}

#[test]
fn x2many_fields_to_admin_only_models() {
    let dir = tempfile::tempdir().unwrap();
    let (core, addons) = setup(dir.path());
    let found = findings(&addons, &settings(&core, ""));
    let lines: Vec<usize> = found.iter().map(|(line, _)| *line).collect();
    assert_eq!(lines, vec![6, 10, 11], "{found:?}");
    assert!(found[0].1.contains("`field_ids` shows `ir.model.fields`"), "{found:?}");
    assert!(found[1].1.contains("`invisible` does not stop"), "{found:?}");
    assert!(found[2].1.contains("`ir.config_parameter`"), "{found:?}");
}

#[test]
fn models_restricted_in_the_config() {
    let dir = tempfile::tempdir().unwrap();
    let (core, addons) = setup(dir.path());
    let config = "[rules.view-field-restricted-comodel]\nmodels = [\"res.tag\"]\n";
    let lines: Vec<usize> = findings(&addons, &settings(&core, config))
        .into_iter()
        .map(|(line, _)| line)
        .collect();
    assert_eq!(lines, vec![6, 10, 11, 13]);
}

#[test]
fn silent_without_the_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let (_, addons) = setup(dir.path());
    assert!(findings(&addons, &Settings::default()).is_empty());
}
