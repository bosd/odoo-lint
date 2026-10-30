//! PO106: a module's `.pot` must hold what `odl pot` writes from its source.

use super::module::{ModuleContext, ModuleReporter};
use crate::fix::{Edit, Fix};
use crate::po::PoFile;
use crate::rules::{Check, Rule};
use crate::sources::normalize_newlines;
use crate::xml::is_installable;
use std::collections::BTreeSet;

pub const POT_OUTDATED: Rule = Rule {
    code: "PO106",
    name: "pot-outdated",
    summary: "A module's translation template differs from what its source exports.",
    doc: r#"
## What it does

Builds the template of a module that has an `i18n/<module>.pot`, as
`odl pot` does, and reports the `.pot` when it differs: terms the source has
and the template lacks, terms the source no longer has, or other references
and comments. It is `odl pot --check` as a rule, for the editor and
`odl check`.

The labels a module's models inherit come from its dependencies: give
`addons-path` with them, Odoo's included. Without it, the fix is unsafe, as
the template may lack those labels.

A module without a `.pot` is left alone; `odl pot` creates one.

## Why is this bad?

Translators work from the template. A term missing from it stays
untranslated; one the source no longer has is translated in vain, and
[PO101](PO101.md) reports it in the `.po` files.

## How to fix

Run `odl pot` (or the fix) to write the template, and `odl pot --merge` to
bring the `.po` files in line with it.
"#,
    check: Check::Module(check_template),
    min_odoo: None,
    max_odoo: None,
};

fn check_template(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    if !is_installable(ctx.module) {
        return;
    }
    let path = ctx.module.path.join("i18n").join(format!("{}.pot", ctx.module.name));
    let Ok(current) = ctx.sources.read_to_string(&path) else {
        return;
    };
    let current = normalize_newlines(&current);
    let Some(template) =
        crate::pot::generate_installed(&ctx.module.path, ctx.settings.target_version, &ctx.settings.addons_path)
    else {
        return;
    };
    if current == template.content {
        return;
    }
    let message = match (PoFile::parse(&current), PoFile::parse(&template.content)) {
        (Ok(current), Ok(generated)) => {
            let ids = |po: &PoFile| -> BTreeSet<String> {
                po.entries
                    .iter()
                    .filter(|e| !e.obsolete && !e.msgid.is_empty())
                    .map(|e| e.msgid.clone())
                    .collect()
            };
            let (have, want) = (ids(&current), ids(&generated));
            let missing = want.difference(&have).count();
            let unused = have.difference(&want).count();
            let terms = |n: usize| {
                if n == 1 {
                    "1 term".to_string()
                } else {
                    format!("{n} terms")
                }
            };
            let mut parts = Vec::new();
            if missing > 0 {
                parts.push(format!("lacks {} of the source", terms(missing)));
            }
            if unused > 0 {
                parts.push(format!("has {} the source no longer exports", terms(unused)));
            }
            if parts.is_empty() {
                "The template's references, comments or layout differ from the source's".to_string()
            } else {
                format!("The template {}", parts.join(" and "))
            }
        }
        _ => "The template differs from the source's".to_string(),
    };
    let mut message = format!("{message}; run `odl pot`");
    if template.incomplete {
        message.push_str(" (dependencies not found on the addons path: labels may be missing)");
    }
    let edits = vec![Edit::replace(0, current.len(), template.content)];
    let title = "Write the template from the source";
    let fix = if template.incomplete {
        Fix::unsafe_(title, edits)
    } else {
        Fix::safe(title, edits)
    };
    reporter.report(&POT_OUTDATED, &path, 1, message).fix = Some(fix);
}
