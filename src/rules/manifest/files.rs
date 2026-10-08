//! Manifest checks against the module's files: README (C8112), required files
//! (C8115, C8118 for apps), migrations (E8145) and the data files the
//! manifest lists (F8101, W8125).

use crate::checker::{ManifestContext, Reporter};
use crate::config::list_or;
use crate::rules::{Check, Rule};
use ruff_python_ast::Expr;
use ruff_text_size::{Ranged, TextSize};

const README_FILES: &[&str] = &["README.rst", "README.md", "README.txt"];
const DEFAULT_README_TEMPLATE_URL: &str =
    "https://github.com/OCA/maintainer-tools/blob/master/template/module/README.rst";
const DEFAULT_REQUIRED_FILES_APP: &[&str] = &["static/description/index.html"];
/// Manifest keys that list data files, current and historical.
const DATA_KEYS: &[&str] = &["data", "demo", "demo_xml", "init_xml", "test", "update_xml"];

pub const MISSING_README: Rule = Rule {
    code: "C8112",
    name: "missing-readme",
    summary: "The module has no README.",
    doc: r#"
## What it does

Checks that the module folder has a `README.rst`, `README.md` or `README.txt`.

## Why is this bad?

Odoo shows the README as the module description in the Apps menu, and it is
the first place users and reviewers look. The message links to the OCA
template.

## Configuration

```toml
[tool.odoo-lint.rules.missing-readme]
template-url = "https://example.com/readme-template"
```
"#,
    check: Check::Manifest(check_readme),
    min_odoo: None,
    max_odoo: None,
};

pub const MISSING_ODOO_FILE: Rule = Rule {
    code: "C8115",
    name: "missing-odoo-file",
    summary: "A file required for paid apps is missing (configurable list).",
    doc: r#"
## What it does

For manifests with a `price`, checks that each configured file exists in the
module. The list is empty by default; [C8118](C8118.md) covers the files the
Odoo Apps store requires.

## Configuration

```toml
[tool.odoo-lint.rules.missing-odoo-file]
files = ["static/description/icon.png"]
```
"#,
    check: Check::Manifest(check_required_files),
    min_odoo: None,
    max_odoo: None,
};

pub const MISSING_ODOO_FILE_APP: Rule = Rule {
    code: "C8118",
    name: "missing-odoo-file-app",
    summary: "A paid app has no `static/description/index.html`.",
    doc: r#"
## What it does

For manifests with a `price`, checks that the files the Odoo Apps store needs
exist. By default that is `static/description/index.html`, the app's store
page.

## Configuration

```toml
[tool.odoo-lint.rules.missing-odoo-file-app]
files = ["static/description/index.html", "static/description/banner.png"]
```
"#,
    check: Check::Manifest(check_required_files_app),
    min_odoo: None,
    max_odoo: None,
};

pub const MANIFEST_BEHIND_MIGRATIONS: Rule = Rule {
    code: "E8145",
    name: "manifest-behind-migrations",
    summary: "The manifest version is lower than a migration folder.",
    doc: r#"
## What it does

Compares the manifest `version` with the folders in the module's
`migrations` directory and reports when a migration folder has a higher
version.

## Why is this bad?

Odoo only runs migration scripts up to the module's version, so a version
bump that was forgotten means the new migration never runs.

## Example

`migrations/17.0.1.1.0/post-migration.py` exists, but the manifest says:

```python
{
    "version": "17.0.1.0.0",
}
```

Use instead: `"version": "17.0.1.1.0"`.
"#,
    check: Check::Manifest(check_behind_migrations),
    min_odoo: None,
    max_odoo: None,
};

pub const RESOURCE_NOT_EXIST: Rule = Rule {
    code: "F8101",
    name: "resource-not-exist",
    summary: "A data file listed in the manifest does not exist.",
    doc: r#"
## What it does

Checks that every file in the manifest's `data`, `demo` and older data keys
(`demo_xml`, `init_xml`, `test`, `update_xml`) exists, relative to the module.

## Why is this bad?

Installing or updating the module fails with a missing file error.
"#,
    check: Check::Manifest(check_resource_not_exist),
    min_odoo: None,
    max_odoo: None,
};

pub const MANIFEST_DATA_DUPLICATED: Rule = Rule {
    code: "W8125",
    name: "manifest-data-duplicated",
    summary: "A data file is listed more than once in the manifest.",
    doc: r#"
## What it does

Reports files listed more than once under the same data key of the manifest,
with the lines of the repeats.

## Why is this bad?

Odoo loads the file twice, which is slow at best and, for files that create
records without an XML id, creates duplicates.
"#,
    check: Check::Manifest(check_data_duplicated),
    min_odoo: None,
    max_odoo: None,
};

fn check_readme(ctx: &ManifestContext, reporter: &mut Reporter) {
    if README_FILES.iter().any(|name| ctx.module.path.join(name).is_file()) {
        return;
    }
    let url = ctx
        .settings
        .config
        .rules()
        .missing_readme
        .as_ref()
        .and_then(|c| c.template_url.clone())
        .unwrap_or_else(|| DEFAULT_README_TEMPLATE_URL.to_string());
    reporter.report(
        &MISSING_README,
        ctx.manifest.dict().start(),
        format!("Missing ./README.rst file. Template here: {url}"),
    );
}

fn missing_files<'a>(ctx: &ManifestContext, files: &'a [String]) -> impl Iterator<Item = String> + 'a {
    let module_path = ctx.module.path.clone();
    let module_name = ctx.module.name.clone();
    files
        .iter()
        .filter(move |file| !module_path.join(file).is_file())
        .map(move |file| format!("{module_name}/{file}"))
}

fn check_required_files(ctx: &ManifestContext, reporter: &mut Reporter) {
    if ctx.manifest.get("price").is_none() {
        return;
    }
    let files = list_or(
        ctx.settings.config.rules().missing_odoo_file.as_ref(),
        |c| c.files.as_ref(),
        &[],
    );
    for file in missing_files(ctx, &files).collect::<Vec<_>>() {
        reporter.report(
            &MISSING_ODOO_FILE,
            ctx.manifest.dict().start(),
            format!("Missing {file} file"),
        );
    }
}

fn check_required_files_app(ctx: &ManifestContext, reporter: &mut Reporter) {
    if ctx.manifest.get("price").is_none() {
        return;
    }
    let files = list_or(
        ctx.settings.config.rules().missing_odoo_file_app.as_ref(),
        |c| c.files.as_ref(),
        DEFAULT_REQUIRED_FILES_APP,
    );
    for file in missing_files(ctx, &files).collect::<Vec<_>>() {
        reporter.report(
            &MISSING_ODOO_FILE_APP,
            ctx.manifest.dict().start(),
            format!("Missing {file} file for modules with price"),
        );
    }
}

/// `17.0.1.0.0` -> `[17, 0, 1, 0, 0]`; `None` unless all parts are integers.
fn version_tuple(version: &str) -> Option<Vec<u64>> {
    version.split('.').map(|part| part.trim().parse().ok()).collect()
}

fn check_behind_migrations(ctx: &ManifestContext, reporter: &mut Reporter) {
    let version = ctx.manifest.get_str("version").unwrap_or("");
    let Ok(entries) = std::fs::read_dir(ctx.module.path.join("migrations")) else {
        return;
    };
    let mut folders: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    // Highest version first, as pylint-odoo sorts the folder names.
    folders.sort_unstable_by(|a, b| b.cmp(a));
    for folder in folders {
        let (Some(migration), Some(manifest)) = (version_tuple(&folder), version_tuple(version)) else {
            continue;
        };
        if migration > manifest {
            reporter.report(
                &MANIFEST_BEHIND_MIGRATIONS,
                ctx.manifest.dict().start(),
                format!("Manifest version ({version}) is lower than migration scripts ({folder})"),
            );
            break;
        }
    }
}

/// Groups the string items of a data list by value, in first-seen order.
fn group_files(list: &Expr) -> Vec<(&str, Vec<TextSize>)> {
    let mut groups: Vec<(&str, Vec<TextSize>)> = Vec::new();
    for (value, expr) in super::string_elements(list) {
        match groups.iter_mut().find(|(v, _)| *v == value) {
            Some((_, offsets)) => offsets.push(expr.start()),
            None => groups.push((value, vec![expr.start()])),
        }
    }
    groups
}

fn check_resource_not_exist(ctx: &ManifestContext, reporter: &mut Reporter) {
    for key in DATA_KEYS {
        let Some(list) = ctx.manifest.get(key) else { continue };
        for (file, offsets) in group_files(list) {
            if !ctx.module.path.join(file).is_file() {
                reporter.report(
                    &RESOURCE_NOT_EXIST,
                    offsets[0],
                    format!("File \"{key}\": \"{file}\" not found."),
                );
            }
        }
    }
}

fn check_data_duplicated(ctx: &ManifestContext, reporter: &mut Reporter) {
    for key in DATA_KEYS {
        let Some(list) = ctx.manifest.get(key) else { continue };
        for (file, offsets) in group_files(list) {
            if offsets.len() < 2 {
                continue;
            }
            let lines: Vec<String> = offsets[1..].iter().map(|o| reporter.line_of(*o).to_string()).collect();
            reporter.report(
                &MANIFEST_DATA_DUPLICATED,
                offsets[0],
                format!(
                    "The file \"{file}\" is duplicated in lines {} from manifest key \"{key}\"",
                    lines.join(", ")
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::run_manifest_rule_in_dir;
    use crate::diagnostics::Violation;
    use crate::settings::Settings;
    use std::fs;
    use std::path::Path;

    fn module(manifest: &str, files: &[&str]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("acme_sale");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("__manifest__.py"), manifest).unwrap();
        for file in files {
            let path = dir.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        root
    }

    fn run(rule: &Rule, root: &Path) -> Vec<Violation> {
        run_manifest_rule_in_dir(rule, &root.join("acme_sale"), &Settings::default())
    }

    #[test]
    fn readme() {
        let without = module("{'name': 'x'}\n", &[]);
        let v = run(&MISSING_README, without.path());
        assert_eq!(v.len(), 1);
        assert_eq!(
            v[0].message,
            "Missing ./README.rst file. Template here: https://github.com/OCA/maintainer-tools/blob/master/template/module/README.rst"
        );
        let with = module("{'name': 'x'}\n", &["README.md"]);
        assert!(run(&MISSING_README, with.path()).is_empty());
    }

    #[test]
    fn app_files() {
        let app = module("{'price': 10}\n", &[]);
        let v = run(&MISSING_ODOO_FILE_APP, app.path());
        assert_eq!(v.len(), 1);
        assert_eq!(
            v[0].message,
            "Missing acme_sale/static/description/index.html file for modules with price"
        );
        assert!(run(&MISSING_ODOO_FILE, app.path()).is_empty());
        let free = module("{'name': 'x'}\n", &[]);
        assert!(run(&MISSING_ODOO_FILE_APP, free.path()).is_empty());
    }

    #[test]
    fn behind_migrations() {
        let root = module(
            "{'version': '17.0.1.0.0'}\n",
            &["migrations/17.0.1.1.0/post-migration.py"],
        );
        fs::create_dir_all(root.path().join("acme_sale/migrations/16.0.2.0.0")).unwrap();
        fs::create_dir_all(root.path().join("acme_sale/migrations/not-a-version")).unwrap();
        let v = run(&MANIFEST_BEHIND_MIGRATIONS, root.path());
        assert_eq!(v.len(), 1);
        assert_eq!(
            v[0].message,
            "Manifest version (17.0.1.0.0) is lower than migration scripts (17.0.1.1.0)"
        );
        let current = module(
            "{'version': '17.0.1.1.0'}\n",
            &["migrations/17.0.1.1.0/post-migration.py"],
        );
        assert!(run(&MANIFEST_BEHIND_MIGRATIONS, current.path()).is_empty());
    }

    #[test]
    fn data_files() {
        let manifest =
            "{\n 'data': [\n  'views/a.xml',\n  'views/missing.xml',\n  'views/a.xml',\n  'views/a.xml',\n ],\n}\n";
        let root = module(manifest, &["views/a.xml"]);
        let missing = run(&RESOURCE_NOT_EXIST, root.path());
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].line, 4);
        assert_eq!(missing[0].message, "File \"data\": \"views/missing.xml\" not found.");
        let duplicated = run(&MANIFEST_DATA_DUPLICATED, root.path());
        assert_eq!(duplicated.len(), 1);
        assert_eq!(duplicated[0].line, 3);
        assert_eq!(
            duplicated[0].message,
            "The file \"views/a.xml\" is duplicated in lines 5, 6 from manifest key \"data\""
        );
    }
}
