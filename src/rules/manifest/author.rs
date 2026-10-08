//! Manifest `author`: C8101 manifest-required-author (extended with
//! per-module-prefix authors) and E8101 manifest-author-string.

use super::key_or_dict;
use crate::checker::{ManifestContext, Reporter};
use crate::rules::{Check, Rule};
use ruff_text_size::Ranged;

pub const MANIFEST_REQUIRED_AUTHOR: Rule = Rule {
    code: "C8101",
    name: "manifest-required-author",
    summary: "None of the required authors is in the manifest `author`.",
    doc: r#"
## What it does

Checks that the comma-separated `author` of every `__manifest__.py` contains
at least one of the required authors. By default that is the Odoo Community
Association (OCA), as in pylint-odoo.

odoo-lint can require a different author per group of modules, chosen by the
name of the module folder, so one repository can enforce its own authors for
its own modules.

## Why is this bad?

The manifest author is shown in the Apps menu and on the Odoo Apps store, and
OCA tooling relies on it. Inconsistent authors make it unclear who maintains a
module and break automated checks that filter on author.

## Configuration

```toml
[tool.odoo-lint.rules.manifest-required-author]
# Used when no mapping matches; a string or a list
authors = ["Odoo Community Association (OCA)"]

[tool.odoo-lint.rules.manifest-required-author.mapping]
"acme_*" = "Acme Corp"
"acme_hr_*" = ["Acme HR", "Acme Corp"]
"acme_special" = "Someone Else"
```

An exact module name wins; otherwise the longest matching `prefix*` pattern
wins, then `authors`. With the configuration above:

| Module folder    | One of these authors is required |
| ---------------- | -------------------------------- |
| `acme_sale`      | Acme Corp                        |
| `acme_hr_leave`  | Acme HR, Acme Corp               |
| `acme_special`   | Someone Else                     |
| `sale_stock_ext` | Odoo Community Association (OCA) |

## Example

For a module folder `acme_sale`:

```python
{
    "name": "Acme Sale",
    "author": "Odoo Community Association (OCA)",
}
```

Use instead:

```python
{
    "name": "Acme Sale",
    "author": "Acme Corp, Odoo Community Association (OCA)",
}
```
"#,
    check: Check::Manifest(check_required_author),
    min_odoo: None,
    max_odoo: None,
};

pub const MANIFEST_AUTHOR_STRING: Rule = Rule {
    code: "E8101",
    name: "manifest-author-string",
    summary: "The manifest `author` is not a string.",
    doc: r#"
## What it does

Checks that `author` in the manifest is a single string. Several authors are
separated by commas inside that string.

## Why is this bad?

Odoo and the Odoo Apps store expect a string. A list breaks tools that read
the manifest, and [C8101](C8101.md) cannot check the authors.

## Example

```python
{
    "author": ["Acme Corp", "Odoo Community Association (OCA)"],
}
```

Use instead:

```python
{
    "author": "Acme Corp, Odoo Community Association (OCA)",
}
```
"#,
    check: Check::Manifest(check_author_string),
    min_odoo: None,
    max_odoo: None,
};

fn check_author_string(ctx: &ManifestContext, reporter: &mut Reporter) {
    if ctx
        .manifest
        .get("author")
        .is_some_and(|value| !value.is_string_literal_expr())
    {
        reporter.report(
            &MANIFEST_AUTHOR_STRING,
            key_or_dict(ctx, "author"),
            "The author key in the manifest file must be a string (with comma separated values)",
        );
    }
}

fn check_required_author(ctx: &ManifestContext, reporter: &mut Reporter) {
    let required = ctx.settings.config.required_authors(&ctx.module.name);
    let location = match ctx.manifest.entry("author") {
        Some((key, value)) => {
            // A non-string author is reported by manifest-author-string (E8101).
            let Some(author) = value.as_string_literal_expr() else {
                return;
            };
            let authors: Vec<&str> = author.value.to_str().split(',').map(str::trim).collect();
            if required.iter().any(|r| authors.contains(&r.as_str())) {
                return;
            }
            key.start()
        }
        None => ctx.manifest.dict().start(),
    };
    let quoted: Vec<String> = required.iter().map(|a| format!("'{a}'")).collect();
    reporter.report(
        &MANIFEST_REQUIRED_AUTHOR,
        location,
        format!(
            "One of the following authors must be present in manifest: {}",
            quoted.join(", ")
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::run_manifest_rule;
    use crate::config::OdooLintConfig;
    use crate::settings::{CliOverrides, Settings};

    fn settings(toml: &str) -> Settings {
        let config = OdooLintConfig::from_odoo_lint_toml(toml).unwrap();
        Settings::new(config, None, CliOverrides::default()).unwrap().0
    }

    #[test]
    fn oca_default() {
        let s = Settings::default();
        let ok = "{\n    'author': 'Acme Corp, Odoo Community Association (OCA)',\n}\n";
        assert!(run_manifest_rule(&MANIFEST_REQUIRED_AUTHOR, ok, "sale_x", &s).is_empty());

        let bad = "{\n    'name': 'x',\n    'author': 'Acme Corp',\n}\n";
        let v = run_manifest_rule(&MANIFEST_REQUIRED_AUTHOR, bad, "sale_x", &s);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 3);
        assert_eq!(
            v[0].message,
            "One of the following authors must be present in manifest: 'Odoo Community Association (OCA)'"
        );
    }

    #[test]
    fn missing_author_reported_at_dict() {
        let v = run_manifest_rule(
            &MANIFEST_REQUIRED_AUTHOR,
            "{'name': 'x'}\n",
            "sale_x",
            &Settings::default(),
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 1);
    }

    #[test]
    fn non_string_author_is_left_to_e8101() {
        let v = run_manifest_rule(
            &MANIFEST_REQUIRED_AUTHOR,
            "{'author': ['A', 'B']}\n",
            "sale_x",
            &Settings::default(),
        );
        assert!(v.is_empty());
    }

    #[test]
    fn author_string() {
        let s = Settings::default();
        let v = run_manifest_rule(&MANIFEST_AUTHOR_STRING, "{\n  'author': ['A'],\n}\n", "m", &s);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 2);
        assert!(run_manifest_rule(&MANIFEST_AUTHOR_STRING, "{'author': 'A'}\n", "m", &s).is_empty());
        assert!(run_manifest_rule(&MANIFEST_AUTHOR_STRING, "{'name': 'x'}\n", "m", &s).is_empty());
    }

    #[test]
    fn mapping_by_prefix() {
        let s = settings(
            r#"
[rules.manifest-required-author.mapping]
"acme_*" = "Acme Corp"
"acme_hr_*" = ["Acme HR", "Acme Corp"]
"#,
        );
        let oca = "{'author': 'Odoo Community Association (OCA)'}\n";
        let v = run_manifest_rule(&MANIFEST_REQUIRED_AUTHOR, oca, "acme_sale", &s);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.ends_with("'Acme Corp'"));
        assert!(run_manifest_rule(
            &MANIFEST_REQUIRED_AUTHOR,
            "{'author': 'Acme HR'}\n",
            "acme_hr_leave",
            &s
        )
        .is_empty());
        assert!(run_manifest_rule(&MANIFEST_REQUIRED_AUTHOR, oca, "sale_stock", &s).is_empty());
    }
}
