//! Module conventions of OCA and of most companies, each a choice of the
//! repository: README fragments (MOD011), the module icon (MOD012) and
//! tests (MOD013).

use super::module::{ModuleContext, ModuleReporter};
use crate::fix::Fix;
use crate::rules::{Check, Rule};
use crate::xml::is_installable;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

// --- MOD011 readme-fragments-missing -----------------------------------------

pub const README_FRAGMENTS_MISSING: Rule = Rule {
    code: "MOD011",
    name: "readme-fragments-missing",
    summary: "The module has no `readme/DESCRIPTION` fragment to build its README from.",
    doc: r#"
## What it does

Reports an installable module without `readme/DESCRIPTION.md` or
`readme/DESCRIPTION.rst`. Opt-in: select it by code or name.

## Why is this bad?

OCA's `oca-gen-addon-readme` and `odl readme` build `README.rst` or
`README.md` and `static/description/index.html` from the fragments in
`readme/`, starting with the description. A hand-written README satisfies
C8112, but is not regenerated, and drifts from the module.

## How to fix

Move the description into `readme/DESCRIPTION.md` (and usage, configuration
and so on into their own fragments), then run `odl readme`.
"#,
    check: Check::Module(check_readme_fragments),
    min_odoo: None,
    max_odoo: None,
};

fn check_readme_fragments(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    if !is_installable(ctx.module) {
        return;
    }
    let readme = ctx.module.path.join("readme");
    if ["DESCRIPTION.md", "DESCRIPTION.rst"]
        .iter()
        .any(|name| readme.join(name).is_file())
    {
        return;
    }
    reporter.report(
        &README_FRAGMENTS_MISSING,
        &ctx.module.manifest_path,
        1,
        "The module has no readme/DESCRIPTION.md fragment to generate its README from",
    );
}

// --- MOD012 module-icon ------------------------------------------------------

/// Icons known by name in `replace`: OCA's template icons before 2025-02.
const KNOWN_ICONS: &[(&str, &str)] = &[
    // The grey cube of 2015, in most OCA modules.
    (
        "oca-2015",
        "eb104f25ab9a14e1742831df1e0428a52736f242afc4c69ea10150599b5f643a",
    ),
    // The first new logo, replaced a week later.
    (
        "oca-2025-01",
        "5b8f892e42545d510e69807036180533c2d9c89e435ce2d777a834add4b621d8",
    ),
];

pub const MODULE_ICON: Rule = Rule {
    code: "MOD012",
    name: "module-icon",
    summary: "The module has no icon, or an outdated one the configuration replaces.",
    doc: r#"
## What it does

With `[rules.module-icon]` configured, reports an installable module without
`static/description/icon.png`, and one whose icon is listed in `replace`.
Every other icon is the module's own and is left alone. Does nothing
without the table:

```toml
[tool.odoo-lint.rules.module-icon]
icon = "tools/icons/company.png"
replace = ["oca-2015", "oca-2025-01", "sha256:0123...", "tools/icons/old.png"]

[tool.odoo-lint.rules.module-icon.paths]
"modules/client/**" = "tools/icons/client.png"
```

`icon` is the icon modules should have; `paths` sets another one for the
modules under a glob (the longest that matches). `replace` lists outdated
icons: `oca-2015` (OCA's grey cube) and `oca-2025-01` (its first new logo),
the SHA-256 of a file, or a file. Paths are relative to the configuration
file.

## Why is this bad?

Odoo shows the icon in the Apps menu; without one it shows a generic
placeholder, and an old icon shows a brand that is no longer used.

## Fix safety

Safe: `odl check --fix` copies the configured icon into the module. Editors
do not offer it, as it is a file copy rather than a text edit. Without an
icon for the module there is no fix.
"#,
    check: Check::Module(check_icon),
    min_odoo: None,
    max_odoo: None,
};

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn check_icon(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let Some(config) = ctx.settings.config.rules().module_icon.as_ref() else {
        return;
    };
    if !is_installable(ctx.module) {
        return;
    }
    let root = &ctx.settings.project_root;
    let wanted: Option<PathBuf> = config
        .paths
        .iter()
        .flatten()
        .filter(|(pattern, _)| ctx.settings.matches_glob(pattern, &ctx.module.path))
        // The most specific glob, the longest, wins.
        .max_by_key(|(pattern, _)| pattern.len())
        .map(|(_, icon)| icon)
        .or(config.icon.as_ref())
        .map(|icon| root.join(icon));
    let target = ctx.module.path.join("static/description/icon.png");
    let fix = |title: &str| {
        wanted
            .as_ref()
            .filter(|icon| icon.is_file())
            .map(|icon| Fix::copy_file(title, icon.clone(), target.clone()))
    };
    let Ok(current) = ctx.sources.read(&target) else {
        let violation = reporter.report(
            &MODULE_ICON,
            &ctx.module.manifest_path,
            1,
            "The module has no static/description/icon.png",
        );
        violation.fix = fix("Add the icon");
        return;
    };
    let hash = sha256_hex(&current);
    // Already the wanted icon.
    if wanted
        .as_ref()
        .and_then(|icon| std::fs::read(icon).ok())
        .is_some_and(|icon| icon == current)
    {
        return;
    }
    let outdated = config.replace.iter().flatten().find(|entry| {
        let entry = entry.as_str();
        if let Some((_, known)) = KNOWN_ICONS.iter().find(|(name, _)| *name == entry) {
            return *known == hash;
        }
        if let Some(digest) = entry.strip_prefix("sha256:") {
            return digest.eq_ignore_ascii_case(&hash);
        }
        std::fs::read(root.join(entry)).is_ok_and(|bytes| bytes == current)
    });
    if let Some(outdated) = outdated {
        let violation = reporter.report(
            &MODULE_ICON,
            &target,
            1,
            format!("The module icon is an outdated one (`{outdated}`)"),
        );
        violation.fix = fix("Replace the icon");
    }
}

// --- MOD013 module-without-tests -------------------------------------------

pub const MODULE_WITHOUT_TESTS: Rule = Rule {
    code: "MOD013",
    name: "module-without-tests",
    summary: "A module with Python models has no tests.",
    doc: r#"
## What it does

Reports an installable module that declares Python models (a class with
`_name` or `_inherit`) but has no `tests/test_*.py`. Opt-in: select it by
code or name.

## Why is this bad?

Fields, computes and overrides break silently on the next change or
upgrade; tests are what the CI runs to catch it.
"#,
    check: Check::Module(check_tests),
    min_odoo: None,
    max_odoo: None,
};

fn check_tests(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    if !is_installable(ctx.module) {
        return;
    }
    let has_tests = std::fs::read_dir(ctx.module.path.join("tests")).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("test_") && name.ends_with(".py")
        })
    });
    if has_tests || !declares_models(ctx, &ctx.module.path) {
        return;
    }
    reporter.report(
        &MODULE_WITHOUT_TESTS,
        &ctx.module.manifest_path,
        1,
        "The module declares models but has no tests/test_*.py",
    );
}

/// Whether a Python file of the module (tests and migrations aside)
/// declares a model.
fn declares_models(ctx: &ModuleContext, module: &Path) -> bool {
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
            } else if name.ends_with(".py")
                && ctx.sources.read_to_string(&path).is_ok_and(|text| {
                    text.lines().any(|line| {
                        let line = line.trim_start();
                        line.starts_with("_name =") || line.starts_with("_inherit =")
                    })
                })
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
