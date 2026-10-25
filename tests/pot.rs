//! `odl pot`: writing and checking module templates.

use odoo_lint::odoo_version::OdooVersion;
use odoo_lint::pot::{run, Installation};
use std::fs;
use std::path::Path;

const NOTHING_ELSE: Installation = Installation {
    addons_path: &[],
    installed: &[],
    skip_auto_install: false,
};

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

    let changes = run(&paths, true, None, OdooVersion::new(19, 0), &NOTHING_ELSE).unwrap();
    assert_eq!(changes.len(), 1);
    assert!(!changes[0].written);
    assert!(!dir.path().join("acme/i18n/acme.pot").exists());

    let changes = run(&paths, false, None, OdooVersion::new(19, 0), &NOTHING_ELSE).unwrap();
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

    assert!(run(&paths, true, None, OdooVersion::new(19, 0), &NOTHING_ELSE)
        .unwrap()
        .is_empty());
}

mod model_terms {
    use super::*;

    /// A module with a model, an extension of another one and constants in
    /// another file, and its generated template.
    fn template() -> String {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "acme/__manifest__.py",
            "{'name': 'Acme', 'version': '18.0.1.0.0', 'depends': ['base']}\n",
        );
        write(
            dir.path(),
            "acme/constants.py",
            "STATES = [('draft', 'Draft')] + [('done', 'Done')]\n",
        );
        write(
            dir.path(),
            "acme/models/order.py",
            r#"from odoo import fields, models

from ..constants import STATES


class AcmeOrder(models.Model):
    _name = "acme.order"
    _description = "Acme Order"
    _sql_constraints = [("name_uniq", "unique(name)", "The name must be unique.")]

    name = fields.Char(help="What the order is called.")
    partner_id = fields.Many2one("res.partner")
    state = fields.Selection(STATES)


class ResPartner(models.Model):
    _inherit = "res.partner"

    order_count = fields.Integer(string="Orders")
"#,
        );
        let paths = [dir.path().to_path_buf()];
        run(&paths, false, None, OdooVersion::new(18, 0), &NOTHING_ELSE).unwrap();
        fs::read_to_string(dir.path().join("acme/i18n/acme.pot")).unwrap()
    }

    fn entry(xml_id_reference: &str, msgid: &str) -> String {
        format!("#: model:{xml_id_reference}\nmsgid \"{msgid}\"\n")
    }

    #[test]
    fn describes_the_model() {
        let pot = template();
        assert!(
            pot.contains(&entry("ir.model,name:acme.model_acme_order", "Acme Order")),
            "{pot}"
        );
    }

    #[test]
    fn labels_a_field_from_its_name() {
        let pot = template();
        assert!(
            pot.contains(&entry(
                "ir.model.fields,field_description:acme.field_acme_order__partner_id",
                "Partner"
            )),
            "{pot}"
        );
    }

    #[test]
    fn exports_the_help_of_a_field() {
        let pot = template();
        assert!(
            pot.contains(&entry(
                "ir.model.fields,help:acme.field_acme_order__name",
                "What the order is called."
            )),
            "{pot}"
        );
    }

    #[test]
    fn reads_a_selection_from_an_imported_constant() {
        let pot = template();
        assert!(
            pot.contains(&entry(
                "ir.model.fields.selection,name:acme.selection__acme_order__state__done",
                "Done"
            )),
            "{pot}"
        );
    }

    #[test]
    fn exports_constraint_messages() {
        let pot = template();
        assert!(
            pot.contains(&entry(
                "ir.model.constraint,message:acme.constraint_acme_order_name_uniq",
                "The name must be unique."
            )),
            "{pot}"
        );
    }

    #[test]
    fn gives_an_extension_its_own_fields() {
        let pot = template();
        assert!(pot.contains("acme.field_res_partner__order_count"), "{pot}");
    }

    #[test]
    fn gives_the_id_fields_to_the_defining_module_before_19() {
        let pot = template();
        assert!(pot.contains("acme.field_acme_order__display_name"), "{pot}");
        assert!(!pot.contains("acme.field_res_partner__display_name"), "{pot}");
    }

    #[test]
    fn leaves_out_fields_the_extension_does_not_define() {
        let pot = template();
        assert!(!pot.contains("acme.field_res_partner__create_uid"), "{pot}");
    }
}
