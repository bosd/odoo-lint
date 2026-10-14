//! ODOO014: actions read with `read()` instead of `_for_xml_id`.

use odoo_lint::fix::FixMode;
use odoo_lint::fixer::fix_paths;
use odoo_lint::linter::lint_paths;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

const PYTHON: &str = r#"from odoo import models


class Partner(models.Model):
    _inherit = "res.partner"

    def action_view_orders(self):
        action = self.env.ref("sale.action_orders").read()[0]
        action["domain"] = [("partner_id", "=", self.id)]
        return action

    def action_view_invoices(self):
        action = self.env.ref('account.action_move_out_invoice_type')
        action = action.read()[0]
        return action

    def action_view_quotations(self):
        quotations = self.env.ref("sale.action_quotations_with_onboarding")
        result = quotations.read()[0]
        result["name"] = quotations.name
        return result

    def action_fields(self):
        return self.env.ref("sale.action_orders").read(["res_model"])

    def partner_data(self):
        return self.env.ref("base.main_partner").read()[0]

    def action_board(self):
        action = self.env.ref("acme.board_action", raise_if_not_found=False)
        if not action:
            return False
        return action.read()[0]

    def sudo_read(self):
        return self.env.ref("sale.action_orders").sudo().read()[0]
"#;

fn module(root: &Path) -> PathBuf {
    let module = root.join("acme_action");
    fs::create_dir_all(module.join("models")).unwrap();
    fs::write(
        module.join("__manifest__.py"),
        "{'name': 'Action', 'version': '17.0.1.0.0', 'license': 'AGPL-3', 'depends': ['sale']}\n",
    )
    .unwrap();
    fs::write(module.join("__init__.py"), "from . import models\n").unwrap();
    fs::write(module.join("models/__init__.py"), "from . import partner\n").unwrap();
    fs::write(module.join("models/partner.py"), PYTHON).unwrap();
    module
}

fn settings() -> Settings {
    let mut settings = Settings::default();
    settings.select = vec!["ODOO014".to_string()];
    settings
}

#[test]
fn reads_of_actions_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path());
    let lines: Vec<usize> = lint_paths(&[dir.path().to_path_buf()], &settings())
        .into_iter()
        .map(|v| v.line)
        .collect();
    assert_eq!(lines, vec![8, 14, 19, 24, 33]);
}

#[test]
fn the_unsafe_fix_uses_for_xml_id() {
    let dir = tempfile::tempdir().unwrap();
    let file = module(dir.path()).join("models/partner.py");
    let result = fix_paths(&[dir.path().to_path_buf()], &settings(), FixMode::Unsafe);
    let (_, _, new) = result.changed.into_iter().find(|(p, _, _)| *p == file).unwrap();
    let expected = PYTHON
        .replace(
            r#"self.env.ref("sale.action_orders").read()[0]
        action["domain"]"#,
            r#"self.env["ir.actions.actions"]._for_xml_id("sale.action_orders")
        action["domain"]"#,
        )
        .replace(
            "        action = self.env.ref('account.action_move_out_invoice_type')\n        action = action.read()[0]\n",
            "        action = self.env['ir.actions.actions']._for_xml_id('account.action_move_out_invoice_type')\n",
        )
        .replace(
            "result = quotations.read()[0]",
            r#"result = self.env["ir.actions.actions"]._for_xml_id("sale.action_quotations_with_onboarding")"#,
        )
        .replace(
            "return action.read()[0]",
            r#"return self.env["ir.actions.actions"]._for_xml_id("acme.board_action")"#,
        );
    assert_eq!(new, expected);
}
