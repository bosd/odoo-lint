//! End-to-end tests of `--fix`: a module on disk, fixed in memory.

use odoo_lint::fix::FixMode;
use odoo_lint::fixer::fix_paths;
use odoo_lint::odoo_version::OdooVersion;
use odoo_lint::settings::Settings;
use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST: &str = r#"{
    "name": "Fix",
    "license": "AGPL-3",
    "active": True,
    "application": False,
}
"#;

const MODEL: &str = r#"from odoo import _, fields, models
from odoo.exceptions import Warning


class Partner(models.Model):
    _inherit = "res.partner"

    code = fields.Char(select=True)

    def write(self, vals):
        self._cr.execute("SELECT 1")
        if not vals:
            raise Warning(_("Nothing to write"))
        super().write(vals)

    @staticmethod
    def helper():
        return _("Static")

    def labels(self):
        return [lambda self: _("Rebound")]
"#;

fn settings(select: &[&str]) -> Settings {
    let mut settings = Settings::default();
    settings.target_version = OdooVersion::new(19, 0);
    settings.select = select.iter().map(|s| s.to_string()).collect();
    settings
}

fn write(root: &Path, path: &str, contents: &str) -> PathBuf {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}

/// The new contents of `path` after fixing, or its contents if unchanged.
fn fixed(root: &Path, select: &[&str], mode: FixMode, path: &Path) -> String {
    let result = fix_paths(&[root.to_path_buf()], &settings(select), mode);
    result
        .changed
        .into_iter()
        .find(|(p, _, _)| p == path)
        .map_or_else(|| fs::read_to_string(path).unwrap(), |(_, _, new)| new)
}

fn module(dir: &Path) -> (PathBuf, PathBuf) {
    let manifest = write(dir, "acme_fix/__manifest__.py", MANIFEST);
    write(dir, "acme_fix/__init__.py", "from . import models\n");
    write(dir, "acme_fix/models/__init__.py", "from . import partner\n");
    let model = write(dir, "acme_fix/models/partner.py", MODEL);
    (manifest, model)
}

const PYTHON_RULES: &[&str] = &["C8116", "W8111", "W8165", "W8161", "R8101", "W8110"];

#[test]
fn safe_python_fixes() {
    let dir = tempfile::tempdir().unwrap();
    let (manifest, model) = module(dir.path());
    let result = fixed(dir.path(), PYTHON_RULES, FixMode::Safe, &model);
    assert_eq!(
        result,
        MODEL
            .replace("select=True", "index=True")
            .replace("self._cr", "self.env.cr")
            .replace("raise Warning(_(", "raise Warning(self.env._(")
    );
    let manifest = fixed(dir.path(), PYTHON_RULES, FixMode::Safe, &manifest);
    assert_eq!(manifest, "{\n    \"name\": \"Fix\",\n    \"license\": \"AGPL-3\",\n}\n");
}

#[test]
fn unsafe_python_fixes_and_idempotence() {
    let dir = tempfile::tempdir().unwrap();
    let (_, model) = module(dir.path());
    let result = fix_paths(&[dir.path().to_path_buf()], &settings(PYTHON_RULES), FixMode::Unsafe);
    for (path, _, new) in &result.changed {
        fs::write(path, new).unwrap();
    }
    let text = fs::read_to_string(&model).unwrap();
    assert!(text.contains("from odoo.exceptions import UserError\n"), "{text}");
    assert!(
        text.contains("raise UserError(self.env._(\"Nothing to write\"))"),
        "{text}"
    );
    assert!(text.contains("        return super().write(vals)\n"), "{text}");
    // `self` is not a record here: no fix.
    assert!(text.contains("return _(\"Static\")"), "{text}");
    assert!(text.contains("lambda self: _(\"Rebound\")"), "{text}");
    assert!(ruff_python_parser::parse_module(&text).is_ok());

    let again = fix_paths(&[dir.path().to_path_buf()], &settings(PYTHON_RULES), FixMode::Unsafe);
    assert!(again.changed.is_empty(), "{:?}", again.changed);
    let left: Vec<&str> = again.remaining.iter().map(|v| v.code.as_str()).collect();
    assert_eq!(left, vec!["W8161", "W8161"]);
}

#[test]
fn warning_import_next_to_user_error() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "acme_fix/__manifest__.py",
        "{\"name\": \"Fix\", \"license\": \"AGPL-3\"}\n",
    );
    let source = "from odoo.exceptions import UserError, Warning\n\n\ndef check():\n    raise Warning('x')\n";
    let path = write(dir.path(), "acme_fix/tools.py", source);
    let text = fixed(dir.path(), &["R8101"], FixMode::Unsafe, &path);
    assert_eq!(
        text,
        "from odoo.exceptions import UserError\n\n\ndef check():\n    raise UserError('x')\n"
    );
}

const POT: &str = r#"# Translation of Odoo Server.
msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"

#. module: acme_fix
#: model:ir.model.fields,field_description:acme_fix.field_res_partner__code
msgid "Code"
msgstr ""
"#;

const PO: &str = r#"msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"

#: model:ir.model.fields,field_description:acme_fix.field_res_partner__code
msgid "Code"
msgstr "Code NL"

#. module: acme_fix
#: code:addons/acme_fix/models/partner.py
msgid "Nothing to write"
msgstr "Niets te schrijven"
"#;

#[test]
fn po_fixes() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "acme_fix/__manifest__.py", MANIFEST);
    let pot = write(dir.path(), "acme_fix/i18n/acme_fix.pot", POT);
    let po = write(dir.path(), "acme_fix/i18n/nl.po", PO);
    let select = &["PO"];

    let result = fix_paths(&[dir.path().to_path_buf()], &settings(select), FixMode::Unsafe);
    for (path, _, new) in &result.changed {
        fs::write(path, new).unwrap();
    }
    let po_text = fs::read_to_string(&po).unwrap();
    assert!(po_text.contains("#. module: acme_fix\n#: model:"), "{po_text}");
    let pot_text = fs::read_to_string(&pot).unwrap();
    // Copied untranslated, with the line number Odoo needs.
    assert!(
        pot_text.contains("#: code:addons/acme_fix/models/partner.py:0\nmsgid \"Nothing to write\"\nmsgstr \"\"\n"),
        "{pot_text}"
    );
    let again = fix_paths(&[dir.path().to_path_buf()], &settings(select), FixMode::Unsafe);
    assert!(again.changed.is_empty(), "{:?}", again.changed);
    assert!(again.remaining.is_empty(), "{:?}", again.remaining);
}

#[test]
fn po_duplicates_merge_one_reference_per_line() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "acme_fix/__manifest__.py", MANIFEST);
    let pot = write(
        dir.path(),
        "acme_fix/i18n/acme_fix.pot",
        r#"msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"

#. module: acme_fix
#: model:ir.model,name:acme_fix.model_acme_machine
msgid "Machine"
msgstr ""

#. module: acme_fix
#: model:ir.model.fields,field_description:acme_fix.field_sale_order__machine_id
#: model_terms:ir.ui.view,arch_db:acme_fix.view_order_form
msgid "Machine"
msgstr ""
"#,
    );

    let result = fix_paths(&[dir.path().to_path_buf()], &settings(&["PO005"]), FixMode::Safe);
    for (path, _, new) in &result.changed {
        fs::write(path, new).unwrap();
    }
    assert!(
        fs::read_to_string(&pot).unwrap().ends_with(
            "#. module: acme_fix\n\
             #: model:ir.model,name:acme_fix.model_acme_machine\n\
             #: model:ir.model.fields,field_description:acme_fix.field_sale_order__machine_id\n\
             #: model_terms:ir.ui.view,arch_db:acme_fix.view_order_form\n\
             msgid \"Machine\"\nmsgstr \"\"\n"
        ),
        "{}",
        fs::read_to_string(&pot).unwrap()
    );
}

#[test]
fn env_translation_drops_the_unused_import() {
    let cases = [
        // Every use fixed: `_` leaves the import.
        (
            "from odoo import _, api, models\n\n\nclass A(models.Model):\n    _inherit = \"res.partner\"\n\n    def a(self):\n        return _(\"A\") + _(\"B\")\n",
            "from odoo import api, models\n\n\nclass A(models.Model):\n    _inherit = \"res.partner\"\n\n    def a(self):\n        return self.env._(\"A\") + self.env._(\"B\")\n",
        ),
        // `_` last in a parenthesized list.
        (
            "from odoo import (\n    models,\n    _,\n)\n\n\nclass A(models.Model):\n    _inherit = \"res.partner\"\n\n    def a(self):\n        return _(\"A\")\n",
            "from odoo import (\n    models,\n)\n\n\nclass A(models.Model):\n    _inherit = \"res.partner\"\n\n    def a(self):\n        return self.env._(\"A\")\n",
        ),
        // `_` alone in its statement.
        (
            "from odoo.tools.translate import _\nfrom odoo import models\n\n\nclass A(models.Model):\n    _inherit = \"res.partner\"\n\n    def a(self):\n        return _(\"A\")\n",
            "from odoo import models\n\n\nclass A(models.Model):\n    _inherit = \"res.partner\"\n\n    def a(self):\n        return self.env._(\"A\")\n",
        ),
        // A use without a fix, or a suppressed one, keeps the import.
        (
            "from odoo import _, models\n\n\ndef label():\n    return _(\"A\")\n\n\nclass A(models.Model):\n    _inherit = \"res.partner\"\n\n    def a(self):\n        return _(\"B\")\n",
            "from odoo import _, models\n\n\ndef label():\n    return _(\"A\")\n\n\nclass A(models.Model):\n    _inherit = \"res.partner\"\n\n    def a(self):\n        return self.env._(\"B\")\n",
        ),
        (
            "from odoo import _, models\n\n\nclass A(models.Model):\n    _inherit = \"res.partner\"\n\n    def a(self):\n        return _(\"A\") + _(\"B\")  # noqa: W8161\n",
            "from odoo import _, models\n\n\nclass A(models.Model):\n    _inherit = \"res.partner\"\n\n    def a(self):\n        return _(\"A\") + _(\"B\")  # noqa: W8161\n",
        ),
    ];
    for (source, expected) in cases {
        let dir = tempfile::tempdir().unwrap();
        module(dir.path());
        let path = write(dir.path(), "acme_fix/models/partner.py", source);
        assert_eq!(
            fixed(dir.path(), &["W8161"], FixMode::Safe, &path),
            expected,
            "{source}"
        );
    }
}

#[test]
fn xml_eval_fix_replaces_a_start_tag_over_several_lines_whole() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "acme_fix/__manifest__.py",
        "{'name': 'Fix', 'license': 'AGPL-3', 'version': '18.0.1.0.0', 'data': ['views/v.xml']}\n",
    );
    write(dir.path(), "acme_fix/__init__.py", "");
    let view = write(
        dir.path(),
        "acme_fix/views/v.xml",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<odoo>\n    <record id=\"r\" model=\"res.partner\">\n        <field\n            name=\"active\">True</field>\n    </record>\n</odoo>\n",
    );

    let result = fix_paths(&[dir.path().to_path_buf()], &settings(&["XML023"]), FixMode::Safe);
    for (path, _, new) in &result.changed {
        fs::write(path, new).unwrap();
    }

    assert!(
        fs::read_to_string(&view)
            .unwrap()
            .contains("        <field\n            name=\"active\" eval=\"True\" />\n"),
        "{}",
        fs::read_to_string(&view).unwrap()
    );
}

#[test]
fn module_findings_are_not_reported_twice_after_a_fix() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "acme_fix/__manifest__.py", MANIFEST);
    write(dir.path(), "acme_fix/__init__.py", "");
    write(dir.path(), "acme_fix/dump.sql", "SELECT 1;\n");

    let result = fix_paths(&[dir.path().to_path_buf()], &settings(&["ALL"]), FixMode::Safe);

    let codes: Vec<&str> = result.remaining.iter().map(|v| v.code.as_str()).collect();
    let sql = codes.iter().filter(|c| **c == "ODOO002").count();
    assert_eq!(sql, 1, "{codes:?}");
}
