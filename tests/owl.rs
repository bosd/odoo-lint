//! OWL001-OWL003: OWL templates in a module's assets.

use odoo_lint::fix::FixMode;
use odoo_lint::fixer::fix_paths;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

const TEMPLATES: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<templates xml:space="preserve">
    <t t-name="acme_owl.Lines">
        <ul>
            <li t-foreach="props.lines" t-as="line" t-on-click="state.line = line">
                <span t-raw="line.html"/>
            </li>
            <li t-foreach="props.lines" t-as="line" t-key="line.id" t-on-click="() => this.selectLine(line)"/>
            <button t-on-click="state.open = !state.open">Toggle</button>
            <button t-on-click.stop="onClick">Ok</button>
        </ul>
    </t>
</templates>
"#;

fn module(root: &Path, version: &str, templates: &str) -> PathBuf {
    let module = root.join("acme_owl");
    fs::create_dir_all(module.join("static/src/xml")).unwrap();
    fs::write(
        module.join("__manifest__.py"),
        format!(
            "{{'name': 'Owl', 'version': '{version}', 'license': 'AGPL-3', \
             'assets': {{'web.assets_backend': ['acme_owl/static/src/**/*']}}}}\n"
        ),
    )
    .unwrap();
    fs::write(module.join("__init__.py"), "").unwrap();
    fs::write(module.join("static/src/xml/lines.xml"), templates).unwrap();
    module
}

fn settings() -> Settings {
    let mut settings = Settings::default();
    settings.select = vec!["OWL".to_string()];
    settings
}

fn found(root: &Path) -> Vec<(String, usize)> {
    let mut found: Vec<(String, usize)> = lint_paths(&[root.to_path_buf()], &settings())
        .into_iter()
        .map(|v| (v.code, v.line))
        .collect();
    found.sort();
    found
}

#[test]
fn owl_templates_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "17.0.1.0.0", TEMPLATES);
    let expected: Vec<(String, usize)> = [("OWL001", 5), ("OWL002", 6), ("OWL003", 5), ("OWL003", 9)]
        .into_iter()
        .map(|(c, l)| (c.to_string(), l))
        .collect();
    assert_eq!(found(dir.path()), expected);
}

#[test]
fn the_unsafe_fixes() {
    let dir = tempfile::tempdir().unwrap();
    let file = module(dir.path(), "17.0.1.0.0", TEMPLATES).join("static/src/xml/lines.xml");
    let result = fix_paths(&[dir.path().to_path_buf()], &settings(), FixMode::Unsafe);
    let (_, _, new) = result.changed.into_iter().find(|(p, _, _)| *p == file).unwrap();
    assert_eq!(
        new,
        TEMPLATES
            .replacen(
                r#"t-foreach="props.lines" t-as="line" t-on-click"#,
                r#"t-foreach="props.lines" t-key="line_index" t-as="line" t-on-click"#,
                1
            )
            .replace("t-raw=", "t-out=")
    );
}

#[test]
fn before_17_only_templates_marked_owl() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "16.0.1.0.0", TEMPLATES);
    assert!(found(dir.path()).is_empty());
    let dir = tempfile::tempdir().unwrap();
    module(
        dir.path(),
        "16.0.1.0.0",
        &TEMPLATES.replace(r#"t-name="acme_owl.Lines""#, r#"t-name="acme_owl.Lines" owl="1""#),
    );
    assert_eq!(found(dir.path()).len(), 4);
}

#[test]
fn inheritance_specs_are_selectors() {
    let dir = tempfile::tempdir().unwrap();
    module(
        dir.path(),
        "17.0.1.0.0",
        "<templates>\n    <t t-name=\"acme_owl.Panel\" t-inherit=\"web.SearchPanel\" t-inherit-mode=\"primary\">\n        <li t-foreach=\"values\" position=\"replace\">\n            <li t-foreach=\"this.items\" t-as=\"item\" t-key=\"item.id\"/>\n        </li>\n        <xpath expr=\"//div[@t-foreach='x']\" position=\"inside\"/>\n    </t>\n</templates>\n",
    );
    assert!(found(dir.path()).is_empty(), "{:?}", found(dir.path()));
}
