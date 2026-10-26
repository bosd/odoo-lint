//! PO105: the languages a repository translates into, configured per
//! repository, must have a translation in every module with terms.

use super::module::{manifest_files, ModuleContext, ModuleReporter};
use crate::po::PoFile;
use crate::rules::{Check, Rule};
use crate::xml::is_installable;
use regex::Regex;
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

pub const PO_TRANSLATION_MISSING: Rule = Rule {
    code: "PO105",
    name: "po-translation-missing",
    summary: "A module with translatable terms lacks the translation of a configured language.",
    doc: r#"
## What it does

For each language the configuration lists, reports a module that has
translatable terms but no `i18n/<language>.po`, and a `.po` file that leaves
terms of the module's `.pot` untranslated (missing, or with an empty
`msgstr`). Does nothing without configured languages:

```toml
[tool.odoo-lint.rules.po-translation-missing]
languages = ["nl", "es"]
```

A language with a region (`nl_BE`) is satisfied by `nl_BE.po` or by the
base `nl.po`, which Odoo loads first; a language without one (`nl`) by `nl.po`
or by any `nl_*.po`.

Without a `.pot`, a module counts as having terms when its Python code
declares fields or calls `_()`/`_lt()`, when the XML files its manifest loads
have views, menus, actions or templates, or when its assets call `_t()`.

## Why is this bad?

Users of an untranslated language see the module's labels, menus and
messages in English. A module without a `.po` for the language shows nothing
wrong until a client opens it.

## How to fix

Export the terms (Settings > Translations > Export, or
`odoo-bin --i18n-export`), translate them and commit `i18n/<language>.po`
with the module's `.pot`.
"#,
    check: Check::Module(check_translations),
    min_odoo: None,
    max_odoo: None,
};

/// Python that Odoo exports terms from: field declarations, `_()` and `_lt()`.
static PY_TERMS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bfields\.[A-Z]\w*\(|(?:^|[^\w.])_(?:lt)?\(|\benv\._\(").unwrap());
/// XML records and templates with translatable texts.
static XML_TERMS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<menuitem\b|model=["']ir\.(?:ui\.view|actions\.\w+)["']|<template\b|\bt-name="#).unwrap()
});
static JS_TERMS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b_t\(").unwrap());

fn check_translations(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let Some(languages) = ctx
        .settings
        .config
        .rules()
        .po_translation_missing
        .as_ref()
        .and_then(|c| c.languages.as_ref())
    else {
        return;
    };
    if languages.is_empty() || !is_installable(ctx.module) {
        return;
    }
    let i18n = ctx.module.path.join("i18n");
    let pot_path = i18n.join(format!("{}.pot", ctx.module.name));
    let pot = ctx
        .sources
        .read_to_string(&pot_path)
        .ok()
        .and_then(|text| PoFile::parse(&text).ok());
    // The terms of the `.pot`, by (msgctxt, msgid).
    let terms: Option<Vec<(Option<String>, String)>> = pot.as_ref().map(|pot| {
        pot.entries
            .iter()
            .filter(|e| !e.obsolete && !e.msgid.is_empty())
            .map(|e| (e.msgctxt.clone(), e.msgid.clone()))
            .collect()
    });
    let has_terms = match &terms {
        Some(terms) => !terms.is_empty(),
        None => has_terms(ctx),
    };
    if !has_terms {
        return;
    }
    let po_files: Vec<String> = std::fs::read_dir(&i18n)
        .map(|entries| {
            let mut names: Vec<String> = entries
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .filter(|name| name.ends_with(".po"))
                .collect();
            names.sort();
            names
        })
        .unwrap_or_default();
    for language in languages {
        let matching: Vec<&String> = po_files
            .iter()
            .filter(|name| covers(name.trim_end_matches(".po"), language))
            .collect();
        if matching.is_empty() {
            reporter.report(
                &PO_TRANSLATION_MISSING,
                &ctx.module.manifest_path,
                1,
                format!(
                    "The module has translatable terms but no `i18n/{language}.po`; \
                     the configuration requires `{language}`"
                ),
            );
            continue;
        }
        let Some(terms) = &terms else { continue };
        // The translated terms of every file that covers the language.
        let mut translated: HashSet<(Option<String>, String)> = HashSet::new();
        let mut first: Option<(&String, PoFile)> = None;
        for name in &matching {
            let Some(po) = ctx
                .sources
                .read_to_string(&i18n.join(name))
                .ok()
                .and_then(|text| PoFile::parse(&text).ok())
            else {
                continue;
            };
            for entry in po.entries.iter().filter(|e| !e.obsolete) {
                let done = if entry.msgid_plural.is_empty() {
                    !entry.msgstr.is_empty()
                } else {
                    entry.msgstr_plural.values().any(|s| !s.is_empty())
                };
                if done {
                    translated.insert((entry.msgctxt.clone(), entry.msgid.clone()));
                }
            }
            if first.is_none() {
                first = Some((name, po));
            }
        }
        let Some((name, po)) = first else { continue };
        let missing: Vec<&(Option<String>, String)> = terms.iter().filter(|t| !translated.contains(*t)).collect();
        if missing.is_empty() {
            continue;
        }
        // The entry of the first untranslated term, or the top of the file.
        let line = po
            .entries
            .iter()
            .find(|e| e.msgctxt == missing[0].0 && e.msgid == missing[0].1)
            .map_or(1, |e| e.msgid_line());
        reporter.report(
            &PO_TRANSLATION_MISSING,
            &i18n.join(name),
            line,
            format!(
                "{} of the {} terms of `{}.pot` have no `{language}` translation, e.g. \"{}\"",
                missing.len(),
                terms.len(),
                ctx.module.name,
                shorten(&missing[0].1)
            ),
        );
    }
}

/// Whether the `.po` file `stem` (`nl`, `nl_BE`) holds the translation of
/// the configured `language`.
fn covers(stem: &str, language: &str) -> bool {
    match language.split_once('_') {
        // Odoo also reads `es_419.po` for the Spanishes of Latin America and
        // `zh_TW.po` for Hong Kong Chinese (`get_base_langs`).
        Some(("es", _)) if stem == "es_419" && language != "es_ES" => true,
        Some(_) if stem == "zh_TW" && language == "zh_HK" => true,
        Some((base, _)) => stem == language || stem == base,
        None => stem == language || stem.split_once('_').is_some_and(|(base, _)| base == language),
    }
}

fn shorten(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    match line.char_indices().nth(60) {
        Some((at, _)) => format!("{}...", &line[..at]),
        None if line.len() < text.len() => format!("{line}..."),
        None => line.to_string(),
    }
}

/// Whether a module without a `.pot` has terms Odoo would export.
fn has_terms(ctx: &ModuleContext) -> bool {
    let path = &ctx.module.path;
    if python_files(path).iter().any(|file| {
        ctx.sources
            .read_to_string(file)
            .is_ok_and(|text| PY_TERMS.is_match(&text))
    }) {
        return true;
    }
    manifest_files(ctx.module)
        .into_iter()
        .filter(|(_, section)| matches!(*section, "data" | "update_xml" | "init_xml" | "qweb" | "assets"))
        .any(|(file, _)| {
            let regex = if file.ends_with(".xml") {
                &XML_TERMS
            } else if file.ends_with(".js") {
                &JS_TERMS
            } else {
                return false;
            };
            ctx.sources
                .read_to_string(&path.join(&file))
                .is_ok_and(|text| regex.is_match(&text))
        })
}

/// The module's Python files, without its tests, migrations and manifest.
fn python_files(module: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut dirs = vec![module.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !name.starts_with('.') && !matches!(&*name, "tests" | "migrations" | "upgrades" | "static") {
                    dirs.push(path);
                }
            } else if name.ends_with(".py") && !matches!(&*name, "__manifest__.py" | "__openerp__.py") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_and_files() {
        assert!(covers("nl", "nl"));
        assert!(covers("nl_BE", "nl"));
        assert!(covers("nl", "nl_BE"));
        assert!(covers("nl_BE", "nl_BE"));
        assert!(!covers("nl_NL", "nl_BE"));
        assert!(!covers("es", "nl"));
        assert!(!covers("nld", "nl"));
        assert!(covers("es_419", "es_MX"));
        assert!(!covers("es_419", "es_ES"));
        assert!(covers("zh_TW", "zh_HK"));
    }

    #[test]
    fn terms_in_python() {
        assert!(PY_TERMS.is_match("name = fields.Char()"));
        assert!(PY_TERMS.is_match("raise UserError(_(\"No\"))"));
        assert!(PY_TERMS.is_match("x = self.env._(\"No\")"));
        assert!(!PY_TERMS.is_match("from . import models"));
        assert!(!PY_TERMS.is_match("self.foo_(1)"));
    }
}
