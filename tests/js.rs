//! JavaScript upgrade rules on a module's assets.

use odoo_lint::fix::FixMode;
use odoo_lint::fixer::fix_paths;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

const JS: &str = r#"/** @odoo-module **/
import { patch, unpatch } from "@web/core/utils/patch";
import { registry } from "@web/core/registry";
import { useService } from "@web/core/utils/hooks";
import Widget from "@web/legacy/js/core/widget";
import { CharField } from "@web/views/fields/char/char_field";

patch(CharField.prototype, "acme_js.CharField", {
    setup() {
        this._super(...arguments);
        this.rpc = useService("rpc");
    },
});
unpatch(CharField.prototype, "acme_js.CharField");
registry.category("fields").add("acme_char", CharField);
registry.category("fields").add("acme_char2", { component: CharField });
"#;

fn module(root: &Path, version: &str) -> PathBuf {
    let module = root.join("acme_js");
    fs::create_dir_all(module.join("static/src/js")).unwrap();
    fs::create_dir_all(module.join("static/lib/x")).unwrap();
    fs::write(
        module.join("__manifest__.py"),
        format!("{{'name': 'Js', 'version': '{version}', 'license': 'AGPL-3'}}\n"),
    )
    .unwrap();
    fs::write(module.join("__init__.py"), "").unwrap();
    fs::write(module.join("static/src/js/field.js"), JS).unwrap();
    // A library the module ships: not checked.
    fs::write(module.join("static/lib/x/x.js"), JS).unwrap();
    module
}

fn settings() -> Settings {
    let mut settings = Settings::default();
    settings.select = vec![
        "U1722".into(),
        "U1723".into(),
        "U1724".into(),
        "U1812".into(),
        "U1813".into(),
    ];
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
fn rules_by_version() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "18.0.1.0.0");
    let expected: Vec<(String, usize)> = [
        ("U1722", 8),
        ("U1723", 2),
        ("U1723", 14),
        ("U1724", 15),
        ("U1812", 11),
        ("U1813", 5),
    ]
    .into_iter()
    .map(|(c, l)| (c.to_string(), l))
    .collect();
    assert_eq!(found(dir.path()), expected);

    // A 17.0 module gets the 17.0 rules only.
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "17.0.1.0.0");
    let codes: Vec<String> = found(dir.path()).into_iter().map(|(c, _)| c).collect();
    assert_eq!(codes, vec!["U1722", "U1723", "U1723", "U1724"]);
}

#[test]
fn the_patch_name_goes() {
    let dir = tempfile::tempdir().unwrap();
    let file = module(dir.path(), "17.0.1.0.0").join("static/src/js/field.js");
    let result = fix_paths(&[dir.path().to_path_buf()], &settings(), FixMode::Safe);
    let (_, _, new) = result.changed.into_iter().find(|(p, _, _)| *p == file).unwrap();
    assert_eq!(
        new,
        JS.replace(
            r#"patch(CharField.prototype, "acme_js.CharField", {"#,
            "patch(CharField.prototype, {"
        )
    );
}
