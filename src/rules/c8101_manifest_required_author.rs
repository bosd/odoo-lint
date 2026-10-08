//! C8101: required author missing from the manifest (pylint-odoo's
//! `manifest-required-author`), extended with per-module-prefix authors.

use crate::checker::{ManifestContext, Reporter};
use crate::rules::{Check, Rule};
use ruff_text_size::Ranged;

pub const RULE: Rule = Rule {
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
    check: Check::Manifest(check),
    min_odoo: None,
    max_odoo: None,
};

fn check(ctx: &ManifestContext, reporter: &mut Reporter) {
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
        &RULE,
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
        assert!(run_manifest_rule(&RULE, ok, "sale_x", &s).is_empty());

        let bad = "{\n    'name': 'x',\n    'author': 'Acme Corp',\n}\n";
        let v = run_manifest_rule(&RULE, bad, "sale_x", &s);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 3);
        assert_eq!(
            v[0].message,
            "One of the following authors must be present in manifest: 'Odoo Community Association (OCA)'"
        );
    }

    #[test]
    fn missing_author_reported_at_dict() {
        let v = run_manifest_rule(&RULE, "{'name': 'x'}\n", "sale_x", &Settings::default());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 1);
    }

    #[test]
    fn non_string_author_is_left_to_e8101() {
        let v = run_manifest_rule(&RULE, "{'author': ['A', 'B']}\n", "sale_x", &Settings::default());
        assert!(v.is_empty());
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
        let v = run_manifest_rule(&RULE, oca, "acme_sale", &s);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.ends_with("'Acme Corp'"));
        assert!(run_manifest_rule(&RULE, "{'author': 'Acme HR'}\n", "acme_hr_leave", &s).is_empty());
        assert!(run_manifest_rule(&RULE, oca, "sale_stock", &s).is_empty());
    }
}
