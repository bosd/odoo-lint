//! `odl pot`: writing and checking module templates.

use odoo_lint::odoo_version::OdooVersion;
use odoo_lint::pot::run;
use std::fs;
use std::path::Path;

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn writes_then_checks_the_template() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "acme/__manifest__.py",
        "{'name': 'Acme', 'version': '18.0.1.0.0'}\n",
    );
    write(
        dir.path(),
        "acme/models/partner.py",
        "from odoo import _\n\n\ndef check():\n    raise UserError(_(\"Nothing to write\"))\n",
    );
    write(
        dir.path(),
        "acme/static/src/widget.js",
        "import { _t } from \"@web/core/l10n/translation\";\nconst label = _t(\"Open\");\n",
    );
    let paths = [dir.path().to_path_buf()];

    let changes = run(&paths, true, None, OdooVersion::new(19, 0)).unwrap();
    assert_eq!(changes.len(), 1);
    assert!(!changes[0].written);
    assert!(!dir.path().join("acme/i18n/acme.pot").exists());

    let changes = run(&paths, false, None, OdooVersion::new(19, 0)).unwrap();
    assert!(changes[0].written);
    let pot = fs::read_to_string(dir.path().join("acme/i18n/acme.pot")).unwrap();
    // The series comes from the manifest version, not the default.
    assert!(pot.contains("\"Project-Id-Version: Odoo Server 18.0\\n\"\n"), "{pot}");
    assert!(
        pot.ends_with(
            "#. module: acme\n#. odoo-python\n#: code:addons/acme/models/partner.py:0\nmsgid \"Nothing to write\"\nmsgstr \"\"\n\n\
             #. module: acme\n#. odoo-javascript\n#: code:addons/acme/static/src/widget.js:0\nmsgid \"Open\"\nmsgstr \"\"\n"
        ),
        "{pot}"
    );

    assert!(run(&paths, true, None, OdooVersion::new(19, 0)).unwrap().is_empty());
}
