//! PO rules of odoo-lint's own: they predict what Odoo's translation loader
//! (`odoo/tools/translate.py`, checked against Odoo 18.0 and 19.0) will do
//! with a file, where OCA's checks only look at the file itself.

use super::po::PoContext;
use crate::checker::Reporter;
use crate::po::{PoEntry, PoFile};
use crate::rules::{Check, Rule};
use regex::Regex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub const PO_NOT_IN_POT: Rule = Rule {
    code: "PO101",
    name: "po-not-in-pot",
    summary: "A translation is ignored because its `msgid` is missing from the module's `.pot`.",
    doc: r#"
## What it does

When a module has a template `i18n/<module>.pot`, reports translations in the
`.po` files next to it whose `msgid` (and `msgctxt`) is not in that template.

## Why is this bad?

Odoo merges every `.po` file with the module's `.pot` before loading it, the
way `msgmerge` does. Entries that are not in the template are marked
obsolete and skipped, without any message in the log: the translation simply
never shows up. This happens when terms are added to a `.po` file, or
translated in Weblate, while the `.pot` was not regenerated.

## How to fix

Regenerate the `.pot` from Odoo (Settings > Translations > Export, or
`odoo-bin --i18n-export`) after changing translatable terms, and commit it
with the `.po` files.
"#,
    check: Check::Po(check_not_in_pot),
    min_odoo: None,
    max_odoo: None,
};

pub const PO_UNKNOWN_OCCURRENCE: Rule = Rule {
    code: "PO102",
    name: "po-unknown-occurrence",
    summary: "Odoo cannot read a `#:` reference of a translation.",
    doc: r#"
## What it does

Checks the `#:` references of each entry the way Odoo reads them. Odoo only
understands:

- `model:<model>,<field>:<module>.<xmlid>` and `model_terms:...` for
  translations of records and views;
- `code:<path>:<line>` for `_()` in Python and JavaScript.

When a module has an `i18n/<module>.pot`, Odoo takes the references from the
template instead of from the `.po` files, so the `.pot` is checked and the
references in the `.po` files are not.

## Why is this bad?

For an unknown reference Odoo logs `malformed po file: unknown occurrence`
and ignores it. A `code:` reference without a line number is worse: reading
the file raises `ValueError`, so none of the module's code translations load
in that language.
"#,
    check: Check::Po(check_unknown_occurrences),
    min_odoo: None,
    max_odoo: None,
};

pub const PO_FILE_NAME: Rule = Rule {
    code: "PO103",
    name: "po-file-name",
    summary: "Odoo never loads a `.po` file with this name.",
    doc: r#"
## What it does

Checks that `.po` files in `i18n` and `i18n_extra` are named after a language
code the way Odoo builds the file names it loads: `nl.po`, `nl_BE.po`,
`es_419.po`, `sr@latin.po`.

## Why is this bad?

For a language such as `nl_BE` Odoo loads exactly `i18n/nl.po`,
`i18n/nl_BE.po` and the same names in `i18n_extra`. A file named `nl-BE.po`,
`nl_be.po` or `dutch.po` is never read.
"#,
    check: Check::Po(check_file_name),
    min_odoo: None,
    max_odoo: None,
};

pub const PO_FUZZY: Rule = Rule {
    code: "PO104",
    name: "po-fuzzy",
    summary: "A translation is marked `fuzzy`, but Odoo loads it anyway.",
    doc: r#"
## What it does

Reports translated entries flagged `#, fuzzy`.

## Why is this bad?

Gettext tools treat fuzzy translations as unreviewed drafts and skip them,
and Weblate exports suggestions that need editing this way. Odoo does not
look at the flag: users see the unreviewed text. Review the translation and
remove the flag, or clear the `msgstr`.
"#,
    check: Check::Po(check_fuzzy),
    min_odoo: None,
    max_odoo: None,
};

/// `msgid` with its context, as polib's `msgid_with_context` matches entries.
fn msgid_with_context(entry: &PoEntry) -> String {
    match entry.msgctxt.as_deref() {
        Some(context) if !context.is_empty() => format!("{context}\x04{}", entry.msgid),
        _ => entry.msgid.clone(),
    }
}

/// The template Odoo merges a `.po` file with (`get_pot_path` in Odoo):
/// `<module>.pot` next to the file, `<module>` being the folder above it.
pub fn pot_path_for(po_path: &Path) -> Option<PathBuf> {
    if po_path.extension()? != "po" {
        return None;
    }
    let module = po_path.parent()?.parent()?.file_name()?;
    let mut name = module.to_os_string();
    name.push(".pot");
    let pot = po_path.with_file_name(name);
    pot.is_file().then_some(pot)
}

fn read_pot(path: &Path) -> Option<PoFile> {
    let source = std::fs::read_to_string(path).ok()?;
    PoFile::parse(&source.replace("\r\n", "\n").replace('\r', "\n")).ok()
}

fn is_translated(entry: &PoEntry) -> bool {
    !entry.msgstr.is_empty() || entry.msgstr_plural.values().any(|s| !s.is_empty())
}

fn check_not_in_pot(ctx: &PoContext, reporter: &mut Reporter) {
    let Ok(po) = ctx.po else { return };
    let Some(pot_path) = pot_path_for(Path::new(ctx.file_path)) else {
        return;
    };
    let Some(pot) = read_pot(&pot_path) else { return };
    let known: HashSet<String> = pot.entries.iter().map(msgid_with_context).collect();
    let pot_name = pot_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    for entry in po.entries.iter().filter(|e| !e.obsolete && is_translated(e)) {
        if !known.contains(&msgid_with_context(entry)) {
            reporter.report_line(
                &PO_NOT_IN_POT,
                entry.msgid_line(),
                format!("Translation is ignored by Odoo: its msgid is not in {pot_name}"),
            );
        }
    }
}

static MODEL_OCCURRENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(model|model_terms):([\w.]+),(\w+):(\w+)\.([^ ]+)").unwrap());
static CODE_OCCURRENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^code:[\w/.]+").unwrap());
/// Kinds older Odoo versions exported; newer ones skip them with a notice.
static LEGACY_OCCURRENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(selection:[\w.]+,\w+|(sql_constraint|constraint):[\w.]+)").unwrap());

fn check_unknown_occurrences(ctx: &PoContext, reporter: &mut Reporter) {
    let Ok(po) = ctx.po else { return };
    // With a template next to it, Odoo uses the template's references.
    if pot_path_for(Path::new(ctx.file_path)).is_some() {
        return;
    }
    for entry in po.entries.iter().filter(|e| !e.obsolete) {
        let mut seen_code = false;
        for (path, line) in &entry.occurrences {
            if MODEL_OCCURRENCE.is_match(path) || LEGACY_OCCURRENCE.is_match(path) {
                continue;
            }
            if CODE_OCCURRENCE.is_match(path) {
                // Odoo reads one code reference per entry and converts its
                // line number with int().
                if !seen_code && (line.is_empty() || !line.chars().all(|c| c.is_ascii_digit())) {
                    reporter.report_line(
                        &PO_UNKNOWN_OCCURRENCE,
                        entry.msgid_line(),
                        format!(
                            "Reference `{path}` has no line number; Odoo fails to read the file's code translations (ValueError). Use `{path}:0`"
                        ),
                    );
                }
                seen_code = true;
                continue;
            }
            let shown = if line.is_empty() {
                path.clone()
            } else {
                format!("{path}:{line}")
            };
            reporter.report_line(
                &PO_UNKNOWN_OCCURRENCE,
                entry.msgid_line(),
                format!("Odoo cannot read reference `{shown}` and logs \"malformed po file: unknown occurrence\""),
            );
        }
    }
}

static LANGUAGE_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z]{2,3}(?:_(?:[A-Z]{2}|\d{3}))?(?:@[A-Za-z]+)?$").unwrap());

fn check_file_name(ctx: &PoContext, reporter: &mut Reporter) {
    let path = Path::new(ctx.file_path);
    if path.extension().is_none_or(|e| e != "po") || !matches!(ctx.data_section, "i18n" | "i18n_extra") {
        return;
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !LANGUAGE_FILE.is_match(&stem) {
        reporter.report_line(
            &PO_FILE_NAME,
            1,
            format!(
                "Odoo never loads `{stem}.po`: name translation files after a language code, such as `nl.po` or `nl_BE.po`"
            ),
        );
    }
}

fn check_fuzzy(ctx: &PoContext, reporter: &mut Reporter) {
    let Ok(po) = ctx.po else { return };
    for entry in po.entries.iter().filter(|e| !e.obsolete && is_translated(e)) {
        if entry.flags.iter().any(|f| f == "fuzzy") {
            reporter.report_line(
                &PO_FUZZY,
                entry.msgid_line(),
                "Fuzzy translation is loaded by Odoo as if it was reviewed",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Violation;
    use crate::linter::lint_po_source;
    use crate::settings::Settings;
    use std::fs;

    const HEADER: &str = "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\n";

    fn run(rule: &Rule, path: &str, source: &str) -> Vec<Violation> {
        lint_po_source(path, source, &[rule], &Settings::default())
    }

    #[test]
    fn not_in_pot() {
        let root = tempfile::tempdir().unwrap();
        let i18n = root.path().join("acme_sale/i18n");
        fs::create_dir_all(&i18n).unwrap();
        fs::write(
            i18n.join("acme_sale.pot"),
            format!("{HEADER}#. module: acme_sale\nmsgid \"Known\"\nmsgstr \"\"\n"),
        )
        .unwrap();
        let po = format!(
            "{HEADER}#. module: acme_sale\nmsgid \"Known\"\nmsgstr \"Bekend\"\n\n#. module: acme_sale\nmsgid \"New\"\nmsgstr \"Nieuw\"\n\n#. module: acme_sale\nmsgid \"Untranslated\"\nmsgstr \"\"\n"
        );
        let path = i18n.join("nl.po");
        let v = run(&PO_NOT_IN_POT, path.to_str().unwrap(), &po);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 10);
        assert_eq!(
            v[0].message,
            "Translation is ignored by Odoo: its msgid is not in acme_sale.pot"
        );
        // Without a template nothing is merged, so nothing is reported.
        assert!(run(&PO_NOT_IN_POT, "other/i18n/nl.po", &po).is_empty());
    }

    #[test]
    fn occurrences() {
        let po = format!(
            "{HEADER}#. module: m\n#: model:ir.model.fields,field_description:m.field_a\n#: model_terms:ir.ui.view,arch_db:m.view_form\n#: code:addons/m/models/a.py:0\n#: selection:res.partner,type\nmsgid \"A\"\nmsgstr \"\"\n\n#. module: m\n#: code:addons/m/models/a.py\n#: ../addons/m/models/a.py:12\n#: model:ir.model.fields,field_description:field_without_module\nmsgid \"B\"\nmsgstr \"\"\n"
        );
        let v: Vec<_> = run(&PO_UNKNOWN_OCCURRENCE, "m/i18n/nl.po", &po)
            .into_iter()
            .map(|v| v.message)
            .collect();
        assert_eq!(v.len(), 3, "{v:#?}");
        assert!(v[0].starts_with("Reference `code:addons/m/models/a.py` has no line number"));
        assert!(v[1].contains("`../addons/m/models/a.py:12`"));
    }

    #[test]
    fn file_names() {
        let src = HEADER.to_string();
        for good in [
            "m/i18n/nl.po",
            "m/i18n/nl_BE.po",
            "m/i18n_extra/es_419.po",
            "m/i18n/sr@latin.po",
            "m/i18n/kab_DZ.po",
        ] {
            assert!(run(&PO_FILE_NAME, good, &src).is_empty(), "{good}");
        }
        for bad in ["m/i18n/nl-BE.po", "m/i18n/nl_be.po", "m/i18n/dutch.po", "m/i18n/NL.po"] {
            assert_eq!(run(&PO_FILE_NAME, bad, &src).len(), 1, "{bad}");
        }
        assert!(run(&PO_FILE_NAME, "m/i18n/m.pot", &src).is_empty());
        assert!(run(&PO_FILE_NAME, "m/other/x.po", &src).is_empty());
    }

    #[test]
    fn fuzzy() {
        let po = format!(
            "{HEADER}#, fuzzy\nmsgid \"A\"\nmsgstr \"a\"\n\n#, fuzzy\nmsgid \"B\"\nmsgstr \"\"\n\nmsgid \"C\"\nmsgstr \"c\"\n"
        );
        assert_eq!(run(&PO_FUZZY, "m/i18n/nl.po", &po).len(), 1);
    }
}
