//! `odl format`: XML files, whitespace only, content kept.

use odoo_lint::format::{run, Outcome};
use odoo_lint::settings::Settings;
use std::fs;

#[test]
fn formats_and_leaves_values_alone() {
    let dir = tempfile::tempdir().unwrap();
    let views = dir.path().join("acme/views/views.xml");
    let data = dir.path().join("acme/data/data.xml");
    fs::create_dir_all(views.parent().unwrap()).unwrap();
    fs::create_dir_all(data.parent().unwrap()).unwrap();
    fs::write(
        &views,
        "<odoo>\n<record id=\"view\" model=\"ir.ui.view\">\n<field name=\"arch\" type=\"xml\">\n<form><field name=\"name\" domain=\"[('a', '&lt;', 1)]\"/></form>\n</field>\n</record>\n</odoo>\n",
    )
    .unwrap();
    let cdata = "<odoo>\n<record id=\"p\" model=\"ir.config_parameter\">\n<field name=\"value\"><![CDATA[A<br/>]]></field>\n</record>\n</odoo>\n";
    fs::write(&data, cdata).unwrap();

    let results = run(&[dir.path().to_path_buf()], &Settings::default(), true).unwrap();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(matches!(&results[0], (p, Outcome::Refused(_)) if *p == data));
    assert!(matches!(&results[1], (p, Outcome::Formatted(_)) if *p == views));
    // `--check` writes nothing.
    assert!(fs::read_to_string(&views).unwrap().starts_with("<odoo>\n<record"));

    run(&[dir.path().to_path_buf()], &Settings::default(), false).unwrap();
    assert_eq!(
        fs::read_to_string(&views).unwrap(),
        "<odoo>\n    <record id=\"view\" model=\"ir.ui.view\">\n        <field name=\"arch\" type=\"xml\">\n            <form>\n                <field name=\"name\" domain=\"[('a', '&lt;', 1)]\" />\n            </form>\n        </field>\n    </record>\n</odoo>\n"
    );
    assert_eq!(fs::read_to_string(&data).unwrap(), cdata);
    // Formatted files stay as they are.
    let again = run(&[dir.path().to_path_buf()], &Settings::default(), false).unwrap();
    assert!(again.iter().all(|(_, o)| matches!(o, Outcome::Refused(_))), "{again:?}");
}
