//! Odoo 20.0's OWL 3: U2031-U2034, and U2035 (opt-in).

use odoo_lint::fix::FixMode;
use odoo_lint::fixer::fix_paths;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

const JS: &str = r#"import { Component, onMounted, useEffect, useExternalListener, useRef, useState } from "@odoo/owl";

export class Card extends Component {
    static template = "acme_owl3.Card";
    static props = { title: String };
    static defaultProps = { title: "" };

    setup() {
        this.state = useState({ open: false });
        this.root = useRef("root");
        useExternalListener(window, "resize", () => this.render());
        useEffect(
            (open) => console.log(open),
            () => [this.state.open]
        );
        onMounted(() => {});
    }
}
"#;

const XML: &str = r#"<templates>
    <t t-name="acme_owl3.Card">
        <div t-ref="root"><t t-esc="props.title"/></div>
        <t t-portal="'body'"><span>Hi</span></t>
    </t>
</templates>
"#;

fn module(root: &Path) -> PathBuf {
    let module = root.join("acme_owl3");
    fs::create_dir_all(module.join("static/src")).unwrap();
    fs::create_dir_all(module.join("models")).unwrap();
    fs::write(
        module.join("__manifest__.py"),
        "{'name': 'Owl3', 'version': '20.0.1.0.0', 'license': 'AGPL-3', \
         'assets': {'web.assets_backend': ['acme_owl3/static/src/**/*']}}\n",
    )
    .unwrap();
    fs::write(module.join("__init__.py"), "from . import models\n").unwrap();
    fs::write(module.join("models/__init__.py"), "from . import partner\n").unwrap();
    fs::write(
        module.join("models/partner.py"),
        "from odoo import models\n\n\nclass Partner(models.Model):\n    _inherit = \"res.partner\"\n    _rec_names_search = [\"name\"]\n",
    )
    .unwrap();
    fs::write(module.join("static/src/card.js"), JS).unwrap();
    fs::write(module.join("static/src/card.xml"), XML).unwrap();
    module
}

fn settings(select: &[&str]) -> Settings {
    let mut settings = Settings::default();
    settings.select = select.iter().map(|s| s.to_string()).collect();
    settings
}

#[test]
fn findings() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path());
    let mut found: Vec<(String, usize)> = lint_paths(&[dir.path().to_path_buf()], &settings(&["U20"]))
        .into_iter()
        .map(|v| (v.code, v.line))
        .collect();
    found.sort();
    let expected: Vec<(String, usize)> = [
        ("U2031", 5),
        ("U2031", 6),
        ("U2032", 1),
        ("U2032", 1),
        ("U2032", 1),
        ("U2033", 12),
        ("U2034", 3),
        ("U2034", 3),
        ("U2034", 4),
        ("U2036", 3),
    ]
    .into_iter()
    .map(|(c, l)| (c.to_string(), l))
    .collect();
    // U2035 is opt-in: a prefix does not select it ... except that `U20`
    // is a prefix selector, which does; see `opt_in` below.
    let without_opt_in: Vec<(String, usize)> = found.iter().filter(|(c, _)| c != "U2035").cloned().collect();
    assert_eq!(without_opt_in, expected);
}

#[test]
fn fixes() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let result = fix_paths(
        &[dir.path().to_path_buf()],
        &settings(&["U2032", "U2033", "U2034", "U2035"]),
        FixMode::Safe,
    );
    let changed = |file: &str| -> String {
        let path = module.join(file);
        let (_, _, new) = result.changed.iter().find(|(p, _, _)| *p == path).unwrap();
        new.clone()
    };
    let js = changed("static/src/card.js");
    assert!(
        js.starts_with(
            "import { Component, onMounted, useLayoutEffect, useListener, useRef, proxy } from \"@odoo/owl\";"
        ),
        "{js}"
    );
    assert!(js.contains("this.state = proxy({ open: false });"), "{js}");
    assert!(js.contains("useListener(window, \"resize\""), "{js}");
    assert!(js.contains("        useLayoutEffect(\n"), "{js}");
    // useRef has no mechanical replacement.
    assert!(js.contains("useRef(\"root\")"), "{js}");
    let xml = changed("static/src/card.xml");
    assert_eq!(
        xml,
        XML.replace("t-esc=", "t-out=").replace("t-portal=", "t-custom-portal=")
    );
    let python = changed("models/partner.py");
    assert!(python.contains("_rec_names_search = (\"name\",)"), "{python}");
}

#[test]
fn opt_in() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path());
    let codes: Vec<String> = lint_paths(&[dir.path().to_path_buf()], &settings(&["ALL"]))
        .into_iter()
        .map(|v| v.code)
        .collect();
    assert!(!codes.contains(&"U2035".to_string()), "{codes:?}");
    assert!(codes.contains(&"U2031".to_string()), "{codes:?}");
}
