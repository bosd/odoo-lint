//! U2036: component templates read the component through `this` in OWL 3.

use odoo_lint::fix::FixMode;
use odoo_lint::fixer::fix_paths;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

const JS: &str = r#"import { Component } from "@odoo/owl";
export class Card extends Component {
    static template = "acme_this.Card";
}
"#;

const XML: &str = r#"<templates>
    <t t-name="acme_this.Card">
        <div t-att-class="{ 'o_open': state.open }" t-on-click="toggle">
            <t t-foreach="props.lines" t-as="line" t-key="line.id">
                <span t-attf-class="o_{{ line.kind }} o_{{ mode }}" t-out="line.name gt 1 and label"/>
            </t>
            <t t-set="total" t-value="sum"/>
            <Total value="total" onChange="(v) => this.update(v, extra)" values="{ items }"/>
        </div>
    </t>
    <t t-name="acme_this.Row">
        <span t-out="row.name"/>
    </t>
    <t t-name="acme_this.Body">
        <t t-call="acme_this.Row"/>
    </t>
</templates>
"#;

fn module(root: &Path) -> PathBuf {
    let module = root.join("acme_this");
    fs::create_dir_all(module.join("static/src")).unwrap();
    fs::write(
        module.join("__manifest__.py"),
        "{'name': 'This', 'version': '20.0.1.0.0', 'license': 'AGPL-3', \
         'assets': {'web.assets_backend': ['acme_this/static/src/**/*']}}\n",
    )
    .unwrap();
    fs::write(module.join("__init__.py"), "").unwrap();
    fs::write(module.join("static/src/card.js"), JS).unwrap();
    // A second component renders the template others t-call.
    fs::write(
        module.join("static/src/row.js"),
        "import { Component } from \"@odoo/owl\";\nexport class Row extends Component {\n    static template = \"acme_this.Row\";\n}\n",
    )
    .unwrap();
    fs::write(module.join("static/src/card.xml"), XML).unwrap();
    module
}

fn settings() -> Settings {
    let mut settings = Settings::default();
    settings.select = vec!["U2036".into()];
    settings
}

#[test]
fn component_templates_get_this() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let messages: Vec<String> = lint_paths(&[dir.path().to_path_buf()], &settings())
        .into_iter()
        .map(|v| format!("{}: {}", v.line, v.message))
        .collect();
    assert_eq!(
        messages,
        vec![
            "3: `t-att-class` reads `state` from the component: OWL 3 needs `this.`",
            "3: `t-on-click` reads `toggle` from the component: OWL 3 needs `this.`",
            "4: `t-foreach` reads `props` from the component: OWL 3 needs `this.`",
            "5: `t-attf-class` reads `mode` from the component: OWL 3 needs `this.`",
            "5: `t-out` reads `label` from the component: OWL 3 needs `this.`",
            "7: `t-value` reads `sum` from the component: OWL 3 needs `this.`",
            "8: `onChange` reads `extra` from the component: OWL 3 needs `this.`",
            "8: `values` reads `items` from the component: OWL 3 needs `this.`",
        ]
    );
    let result = fix_paths(&[dir.path().to_path_buf()], &settings(), FixMode::Safe);
    let file = module.join("static/src/card.xml");
    let (_, _, new) = result.changed.iter().find(|(p, _, _)| *p == file).unwrap();
    assert_eq!(
        new,
        &XML.replace("state.open", "this.state.open")
            .replace("t-on-click=\"toggle\"", "t-on-click=\"this.toggle\"")
            .replace("\"props.lines\"", "\"this.props.lines\"")
            .replace("o_{{ mode }}", "o_{{ this.mode }}")
            .replace("and label", "and this.label")
            .replace("t-value=\"sum\"", "t-value=\"this.sum\"")
            .replace("v, extra)", "v, this.extra)")
            .replace("{ items }", "{ items: this.items }")
    );
}
