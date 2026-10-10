//! ODOO008: inheritance specs checked against the parent view, as the
//! modules of `depends` build it.

use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn module(root: &Path, name: &str, depends: &[&str], views: &str) {
    let depends: Vec<String> = depends.iter().map(|d| format!("'{d}'")).collect();
    write(
        root,
        &format!("{name}/__manifest__.py"),
        &format!(
            "{{'name': '{name}', 'depends': [{}], 'data': ['views/views.xml']}}\n",
            depends.join(", ")
        ),
    );
    write(root, &format!("{name}/__init__.py"), "");
    write(root, &format!("{name}/views/views.xml"), views);
}

/// A miniature Odoo in `core/`: `base` with a partner form and a template,
/// `sale` extending both, and `extra` (a dependency of nothing) adding a
/// group to the form.
fn odoo(root: &Path) -> PathBuf {
    let core = root.join("core");
    module(
        &core,
        "base",
        &[],
        r#"<odoo>
    <record id="partner_form" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="arch" type="xml">
            <form>
                <header><button name="%(action_merge)d" type="action"/></header>
                <sheet>
                    <group name="main"><field name="name"/></group>
                </sheet>
            </form>
        </field>
    </record>
    <template id="layout"><div class="page"><t t-out="0"/></div></template>
</odoo>
"#,
    );
    module(
        &core,
        "sale",
        &["base"],
        r#"<odoo>
    <record id="partner_form" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="inherit_id" ref="base.partner_form"/>
        <field name="arch" type="xml">
            <field name="name" position="after">
                <field name="sale_warn"/>
            </field>
        </field>
    </record>
    <template id="layout_sale" inherit_id="base.layout">
        <xpath expr="//div[hasclass('page')]" position="inside"><span id="sale_note"/></xpath>
    </template>
</odoo>
"#,
    );
    module(
        &core,
        "extra",
        &["base"],
        r#"<odoo>
    <record id="partner_form" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="inherit_id" ref="base.partner_form"/>
        <field name="arch" type="xml">
            <group name="main" position="after"><group name="extra"/></group>
        </field>
    </record>
</odoo>
"#,
    );
    core
}

const VIEWS: &str = r#"<odoo>
    <record id="partner_form" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="inherit_id" ref="base.partner_form"/>
        <field name="arch" type="xml">
            <field name="sale_warn" position="after"><field name="ref"/></field>
            <xpath expr="//button[@name='%(base.action_merge)d']" position="attributes">
                <attribute name="invisible">1</attribute>
            </xpath>
            <xpath expr="//field[@name='sale_warn']/following-sibling::field[1]" position="after"/>
        </field>
    </record>
    <record id="partner_form_extra" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="inherit_id" ref="base.partner_form"/>
        <field name="arch" type="xml">
            <data>
                <field name="name" position="before"><field name="title"/></field>
                <xpath expr="//group[@name='extra']" position="inside"/>
            </data>
        </field>
    </record>
    <record id="partner_form_primary" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="inherit_id" ref="base.partner_form"/>
        <field name="mode">primary</field>
        <field name="arch" type="xml">
            <xpath expr="//field[@name='ref']" position="replace"/>
            <xpath expr="//sheet" position="replace"><sheet>$0<p/></sheet></xpath>
            <field name="nonexistent" position="after"/>
        </field>
    </record>
    <template id="layout_acme" inherit_id="base.layout">
        <xpath expr="//span[@id='sale_note']" position="replace"/>
        <xpath expr="//span[@id='sale_note']" position="after"/>
    </template>
</odoo>
"#;

fn messages(paths: &[PathBuf], addons_path: Vec<PathBuf>) -> Vec<(usize, String)> {
    let mut settings = Settings::default();
    settings.select = vec!["ODOO008".into()];
    settings.addons_path = addons_path;
    let mut found: Vec<(usize, String)> = lint_paths(paths, &settings)
        .into_iter()
        .map(|v| (v.line, v.message))
        .collect();
    found.sort();
    found
}

#[test]
fn specs_against_parent_views() {
    let dir = tempfile::tempdir().unwrap();
    let core = odoo(dir.path());
    let addons = dir.path().join("addons");
    module(&addons, "acme_sale", &["sale"], VIEWS);
    assert_eq!(
        messages(std::slice::from_ref(&addons), vec![core]),
        vec![
            (
                19,
                "`<xpath expr=\"//group[@name='extra']\">` matches nothing in `base.partner_form`, as the modules `depends` reaches build it".to_string()
            ),
            (
                30,
                "`<field name=\"nonexistent\">` matches nothing in `base.partner_form`, as the modules `depends` reaches build it".to_string()
            ),
            (
                35,
                "`<xpath expr=\"//span[@id='sale_note']\">` matches nothing in `base.layout`, as the modules `depends` reaches build it".to_string()
            ),
        ]
    );
}

#[test]
fn silent_without_all_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let core = odoo(dir.path());
    let addons = dir.path().join("addons");
    module(&addons, "acme_sale", &["sale", "somewhere_else"], VIEWS);
    assert!(messages(std::slice::from_ref(&addons), vec![core]).is_empty());
    module(&addons, "acme_sale", &["sale"], VIEWS);
    assert!(messages(&[addons], Vec::new()).is_empty());
}
