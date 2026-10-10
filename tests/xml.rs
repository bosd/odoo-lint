//! XML checks end to end: which files a module loads, version gating per
//! module, reporting positions and fixes.

use odoo_lint::fix::FixMode;
use odoo_lint::fixer::fix_paths;
use odoo_lint::linter::lint_paths;
use odoo_lint::odoo_version::OdooVersion;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

fn settings(select: &[&str]) -> Settings {
    let mut settings = Settings::default();
    settings.target_version = OdooVersion::new(17, 0);
    settings.select = select.iter().map(|s| s.to_string()).collect();
    settings
}

fn write(root: &Path, path: &str, contents: &str) -> PathBuf {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}

/// A module loading `files` (name, contents) as `data`.
fn module(root: &Path, version: &str, files: &[(&str, &str)]) -> PathBuf {
    let data: Vec<String> = files.iter().map(|(name, _)| format!("'{name}'")).collect();
    write(
        root,
        "acme_xml/__manifest__.py",
        &format!(
            "{{'name': 'XML', 'version': '{version}', 'license': 'AGPL-3', 'data': [{}]}}\n",
            data.join(", ")
        ),
    );
    write(root, "acme_xml/__init__.py", "");
    for (name, contents) in files {
        write(root, &format!("acme_xml/{name}"), contents);
    }
    root.join("acme_xml")
}

fn findings(root: &Path, select: &[&str]) -> Vec<(String, usize, String)> {
    lint_paths(&[root.to_path_buf()], &settings(select))
        .into_iter()
        .map(|v| {
            let file = Path::new(&v.file_path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (file, v.line, v.code)
        })
        .collect()
}

const BOOTSTRAP: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<odoo>
    <record id="view_partner_bootstrap" model="ir.ui.view">
        <field name="name">res.partner.bootstrap</field>
        <field name="model">res.partner</field>
        <field name="arch" type="xml">
            <form>
                <div class="ml-2 mr-auto pl-3 pr-0">
                    <span class="sr-only">Hidden label</span>
                    <span class="badge badge-pill">New</span>
                </div>
                <div class="row no-gutters ml-sm-3 text-md-right float-lg-left">
                    <div class="border-left rounded-right font-weight-bold font-italic">
                        <field name="comment" class="text-monospace"/>
                    </div>
                    <div class="dropdown-menu-right custom-select text-left"/>
                </div>
                <div class="mt-2 mb-3 mx-auto o-text-right text-lefty ml-6 o_ml-2 my-ml-3"/>
                <div class="form-group">ml-3 in text stays</div>
            </form>
        </field>
    </record>
</odoo>
"#;

#[test]
fn bootstrap_classes_are_renamed_from_odoo_15() {
    let dir = tempfile::tempdir().unwrap();
    let module_path = module(dir.path(), "15.0.1.0.0", &[("views/bootstrap.xml", BOOTSTRAP)]);
    let found = findings(dir.path(), &["XML101", "XML102"]);
    let lines: Vec<(usize, &str)> = found.iter().map(|(_, line, code)| (*line, code.as_str())).collect();
    assert_eq!(
        lines,
        vec![
            (8, "XML101"),
            (9, "XML101"),
            (10, "XML101"),
            (12, "XML101"),
            (13, "XML101"),
            (14, "XML101"),
            (16, "XML101"),
            (19, "XML102"),
        ]
    );

    let file = module_path.join("views/bootstrap.xml");
    let result = fix_paths(&[dir.path().to_path_buf()], &settings(&["XML101"]), FixMode::Safe);
    let (_, _, fixed) = result.changed.iter().find(|(p, _, _)| p == &file).unwrap();
    let expected = BOOTSTRAP
        .replace(r#""ml-2 mr-auto pl-3 pr-0""#, r#""ms-2 me-auto ps-3 pe-0""#)
        .replace(r#""sr-only""#, r#""visually-hidden""#)
        .replace(r#""badge badge-pill""#, r#""badge rounded-pill""#)
        .replace(
            r#""row no-gutters ml-sm-3 text-md-right float-lg-left""#,
            r#""row g-0 ms-sm-3 text-md-end float-lg-start""#,
        )
        .replace(
            r#""border-left rounded-right font-weight-bold font-italic""#,
            r#""border-start rounded-end fw-bold fst-italic""#,
        )
        .replace(r#""text-monospace""#, r#""font-monospace""#)
        .replace(
            r#""dropdown-menu-right custom-select text-left""#,
            r#""dropdown-menu-end form-select text-start""#,
        );
    assert_eq!(fixed, &expected);
}

#[test]
fn version_gates_follow_the_module() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "14.0.1.0.0", &[("views/bootstrap.xml", BOOTSTRAP)]);
    assert!(
        findings(dir.path(), &["XML101"]).is_empty(),
        "Bootstrap 4 is right in 14.0"
    );

    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "8_0.1.0.0", &[("views/bootstrap.xml", BOOTSTRAP)]);
    assert!(findings(dir.path(), &["XML101"]).is_empty(), "malformed version");

    // A short version is the current series (the target version here).
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "1.0", &[("views/bootstrap.xml", BOOTSTRAP)]);
    let mut settings = settings(&["XML101"]);
    settings.target_version = OdooVersion::new(16, 0);
    assert_eq!(lint_paths(&[dir.path().to_path_buf()], &settings).len(), 7);
}

const RECORDS_A: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<odoo>
    <record id="partner_a" model="res.partner">
        <field name="name">A</field>
        <field name="active">False</field>
        <field name="color">3</field>
    </record>
    <menuitem name="Root"
        id="acme_xml.menu_root"
    />
</odoo>
"#;

const RECORDS_B: &str = r#"<odoo>
    <!-- oca-hooks:disable=xml-tag-position -->
    <record model="res.partner" id="partner_a">
        <field name="name">A again</field>
    </record>
</odoo>
"#;

#[test]
fn records_across_files() {
    let dir = tempfile::tempdir().unwrap();
    module(
        dir.path(),
        "17.0.1.0.0",
        &[("data/a.xml", RECORDS_A), ("data/b.xml", RECORDS_B)],
    );
    // Not in the manifest: not checked.
    write(
        dir.path(),
        "acme_xml/data/unused.xml",
        "<odoo><record model='x'/></odoo>",
    );
    let found = findings(dir.path(), &["XML"]);
    assert_eq!(
        found,
        vec![
            ("a.xml".into(), 3, "XML005".into()),
            ("a.xml".into(), 5, "XML023".into()),
            ("a.xml".into(), 6, "XML024".into()),
            // libxml2 numbers an element by the line its start tag ends on.
            ("a.xml".into(), 10, "XML008".into()),
            ("a.xml".into(), 10, "XML009".into()),
            ("b.xml".into(), 1, "XML002".into()),
        ]
    );
}

#[test]
fn fixes_keep_the_layout() {
    let dir = tempfile::tempdir().unwrap();
    let module_path = module(
        dir.path(),
        "17.0.1.0.0",
        &[("data/a.xml", RECORDS_A), ("data/b.xml", RECORDS_B)],
    );
    let result = fix_paths(&[dir.path().to_path_buf()], &settings(&["XML"]), FixMode::Unsafe);
    for (path, _, new) in &result.changed {
        fs::write(path, new).unwrap();
    }
    let a = fs::read_to_string(module_path.join("data/a.xml")).unwrap();
    assert!(a.contains("<field name=\"active\" eval=\"False\" />"), "{a}");
    assert!(a.contains("<field name=\"color\" eval=\"3\" />"), "{a}");
    assert!(
        a.contains("<menuitem id=\"menu_root\"\n        name=\"Root\"\n    />"),
        "{a}"
    );
    let b = fs::read_to_string(module_path.join("data/b.xml")).unwrap();
    assert!(
        b.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<odoo>"),
        "{b}"
    );
    // Disabled in that file.
    assert!(b.contains("<record model=\"res.partner\" id=\"partner_a\">"), "{b}");

    let again = fix_paths(&[dir.path().to_path_buf()], &settings(&["XML"]), FixMode::Unsafe);
    assert!(again.changed.is_empty(), "{:?}", again.changed);
}

#[test]
fn only_requested_files_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let module_path = module(
        dir.path(),
        "17.0.1.0.0",
        &[("data/a.xml", RECORDS_A), ("data/b.xml", RECORDS_B)],
    );
    let only_b = lint_paths(&[module_path.join("data/b.xml")], &settings(&["XML"]));
    let files: Vec<String> = only_b.iter().map(|v| v.file_path.clone()).collect();
    assert!(
        !files.is_empty() && files.iter().all(|f| f.ends_with("b.xml")),
        "{files:?}"
    );
}

const CHATTERS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<odoo>
    <record id="standard" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="arch" type="xml">
            <form>
                <div class="oe_chatter">
                    <field name="message_follower_ids" widget="mail_followers" />
                    <field name="activity_ids" widget="mail_activity" />
                    <field name="message_ids" widget="mail_thread" />
                </div>
            </form>
        </field>
    </record>
    <record id="partial" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="arch" type="xml">
            <form>
                <div class="oe_chatter"><field name="message_follower_ids"/><field name="message_ids"/></div>
            </form>
        </field>
    </record>
    <record id="options" model="ir.ui.view">
        <field name="model">res.partner</field>
        <field name="arch" type="xml">
            <form>
                <div class="oe_chatter">
                    <field name="message_ids" options="{'post_refresh': 'recipients'}"/>
                </div>
            </form>
        </field>
    </record>
</odoo>
"#;

#[test]
fn standard_chatters_become_the_chatter_tag() {
    let dir = tempfile::tempdir().unwrap();
    let module_path = module(dir.path(), "18.0.1.0.0", &[("views/chatter.xml", CHATTERS)]);
    let file = module_path.join("views/chatter.xml");
    let mut settings = settings(&["XML015"]);
    settings.target_version = OdooVersion::new(18, 0);
    let standard = CHATTERS.replace(
        "<div class=\"oe_chatter\">\n                    <field name=\"message_follower_ids\" widget=\"mail_followers\" />\n                    <field name=\"activity_ids\" widget=\"mail_activity\" />\n                    <field name=\"message_ids\" widget=\"mail_thread\" />\n                </div>",
        "<chatter />",
    );
    let partial = standard.replace(
        "<div class=\"oe_chatter\"><field name=\"message_follower_ids\"/><field name=\"message_ids\"/></div>",
        "<chatter/>",
    );
    for (mode, expected) in [(FixMode::Safe, &standard), (FixMode::Unsafe, &partial)] {
        let result = fix_paths(&[dir.path().to_path_buf()], &settings, mode);
        let (_, _, fixed) = result.changed.iter().find(|(p, _, _)| p == &file).unwrap();
        assert_eq!(fixed, expected);
        // The block with options stays, to do by hand.
        assert_eq!(result.remaining.len(), if mode == FixMode::Safe { 2 } else { 1 });
    }
}
