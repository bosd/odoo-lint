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
`addons-path` with them, Odoo's included. When a dependency is not found,
only terms of the module's code and data files that the template lacks are
reported (a new `_()` message or view label), since the labels of its models
may be wrong, and there is no fix.

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
    let (Ok(current_po), Ok(generated)) = (PoFile::parse(&current), PoFile::parse(&template.content)) else {
        if !template.incomplete {
            let fix = fix(&current, template.content);
            reporter
                .report(
                    &POT_OUTDATED,
                    &path,
                    1,
                    "The template differs from the source's; run `odl pot`",
                )
                .fix = Some(fix);
        }
        return;
    };
    let have: BTreeSet<String> = current_po
        .entries
        .iter()
        .filter(|e| !e.obsolete && !e.msgid.is_empty())
        .map(|e| e.msgid.clone())
        .collect();
    if template.incomplete {
        // Without the dependencies, the labels of the models may be wrong
        // or missing: only the terms of code and data files count, and only
        // those the template lacks.
        let missing = msgids(&generated, |reference| !reference.starts_with("model:ir.model"))
            .difference(&have)
            .count();
        if missing > 0 {
            reporter.report(
                &POT_OUTDATED,
                &path,
                1,
                format!(
                    "The template lacks {} of the source's code or data files; run `odl pot` \
                     with the module's dependencies on the addons path",
                    terms(missing)
                ),
            );
        }
        return;
    }
    let want = msgids(&generated, |_| true);
    let missing = want.difference(&have).count();
    let unused = have.difference(&want).count();
    let mut parts = Vec::new();
    if missing > 0 {
        parts.push(format!("lacks {} of the source", terms(missing)));
    }
    if unused > 0 {
        parts.push(format!("has {} the source no longer exports", terms(unused)));
    }
    let message = if parts.is_empty() {
        "The template's references, comments or layout differ from the source's".to_string()
    } else {
        format!("The template {}", parts.join(" and "))
    };
    let fix = fix(&current, template.content);
    reporter
        .report(&POT_OUTDATED, &path, 1, format!("{message}; run `odl pot`"))
        .fix = Some(fix);
}

fn fix(current: &str, content: String) -> Fix {
    Fix::safe(
        "Write the template from the source",
        vec![Edit::replace(0, current.len(), content)],
    )
}

fn terms(n: usize) -> String {
    if n == 1 {
        "1 term".to_string()
    } else {
        format!("{n} terms")
    }
}

/// The msgids of `po` with a reference `keep` accepts.
fn msgids(po: &PoFile, keep: impl Fn(&str) -> bool) -> BTreeSet<String> {
    po.entries
        .iter()
        .filter(|e| !e.obsolete && !e.msgid.is_empty() && e.occurrences.iter().any(|(r, _)| keep(r)))
        .map(|e| e.msgid.clone())
        .collect()
}
