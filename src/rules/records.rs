//! ODOO009: fields that data records set (`<record>` in XML, the columns of
//! a CSV file) and that do not exist on the record's model. Silent without
//! the dependencies of a module (see `addons-path`).

use crate::index::{closure, module_index, modules_defining, Closure};
use crate::rules::module::{read_csv, ModuleContext, ModuleReporter};
use crate::rules::{Check, Rule};
use std::path::{Path, PathBuf};

pub const RECORD_FIELD_NOT_FOUND: Rule = Rule {
    code: "ODOO009",
    name: "record-field-not-found",
    summary: "A data record sets a field that does not exist on its model.",
    doc: r#"
## What it does

Reports `<field name="...">` in the `<record>`s of data and demo files, and
the columns of CSV data files, whose field the model does not have, as the
modules the module's `depends` reach define it.

## Why is this bad?

Odoo refuses the record: the module fails to install or update ("Invalid
field 'x' on model 'y'"). It typically happens after an upgrade, when Odoo
removed or renamed a field (`numbercall` on `ir.cron` in 18.0, `category_id`
on `res.groups` in 19.0), or when the field comes from a module that is not
in `depends`.

## Configuration

Like [ODOO004](ODOO004.md), the check needs the code of the dependencies,
Odoo's included, in `addons-path`; a module whose dependencies cannot all be
found is not checked. Fields whose name starts with `x_` (custom fields) are
not checked.
"#,
    check: Check::Module(check_record_fields),
    min_odoo: None,
    max_odoo: None,
};

struct Checker<'a> {
    closure: &'a Closure,
    /// Where the module and its dependencies are, to name the module that
    /// has a missing field.
    dirs: Vec<PathBuf>,
}

impl Checker<'_> {
    /// The message when `field` does not exist on `model`; `None` when it
    /// does, or when the model is unknown (ODOO006's).
    fn missing(&self, model: &str, field: &str) -> Option<String> {
        if field.starts_with("x_") || field.is_empty() || !field.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        let fields = self.closure.fields(model)?;
        if fields.contains_key(field) {
            return None;
        }
        let modules = modules_defining(&self.dirs, model, field);
        let hint = if modules.is_empty() {
            String::new()
        } else {
            format!(
                "; it is defined in `{}`, which `depends` does not reach",
                modules.join("`, `")
            )
        };
        Some(format!("Field `{field}` does not exist on `{model}`{hint}"))
    }

    fn xml(&self, path: &Path, text: &str, reporter: &mut ModuleReporter) {
        let Ok(doc) = roxmltree::Document::parse(text) else {
            return;
        };
        for record in doc.descendants().filter(|n| n.has_tag_name("record")) {
            let Some(model) = record.attribute("model") else {
                continue;
            };
            for field in record.children().filter(|n| n.has_tag_name("field")) {
                let Some(name) = field.attribute("name") else { continue };
                if let Some(message) = self.missing(model, name) {
                    let line = doc.text_pos_at(field.range().start).row as usize;
                    reporter.report(&RECORD_FIELD_NOT_FOUND, path, line, message);
                }
            }
        }
    }

    fn csv(&self, path: &Path, text: &str, reporter: &mut ModuleReporter) {
        let Some(model) = path.file_stem().and_then(|s| s.to_str()) else {
            return;
        };
        let Ok(records) = read_csv(text) else { return };
        let Some(header) = records.first() else { return };
        for column in &header.fields {
            // `partner_id:id`, `partner_id/id`, `line_ids/name`: the field
            // is what comes first.
            let field = column.trim().split(['/', ':']).next().unwrap_or_default();
            if field == "id" {
                continue;
            }
            if let Some(message) = self.missing(model, field) {
                reporter.report(&RECORD_FIELD_NOT_FOUND, path, header.line, message);
            }
        }
    }
}

fn check_record_fields(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let module = ctx.module;
    let local_base = module
        .path
        .parent()
        .is_some_and(|dir| dir.join("base").join("__manifest__.py").is_file());
    if ctx.settings.addons_path.is_empty() && !local_base {
        return;
    }
    let Some(closure) = closure(module, &ctx.settings.addons_path) else {
        return;
    };
    let Some(own) = module_index(&module.path) else { return };
    let mut dirs: Vec<PathBuf> = module.path.parent().map(PathBuf::from).into_iter().collect();
    dirs.extend(ctx.settings.addons_path.iter().cloned());
    let checker = Checker {
        closure: &closure,
        dirs,
    };
    for file in &own.data_files {
        let Ok(text) = ctx.sources.read_to_string(&file.path) else {
            continue;
        };
        match file.path.extension().and_then(|e| e.to_str()) {
            Some("xml") => checker.xml(&file.path, &text, reporter),
            Some("csv") => checker.csv(&file.path, &text, reporter),
            _ => {}
        }
    }
}
