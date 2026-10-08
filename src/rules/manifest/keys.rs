//! Manifest keys: required (C8102, C8119 for apps), deprecated (C8103) and
//! superfluous (C8116).

use super::{add_default, key_or_dict};
use crate::checker::{ManifestContext, Reporter};
use crate::config::list_or;
use crate::fix::{Edit, Fix};
use crate::pyliteral::{is_truthy, str_of};
use crate::rules::{Check, Rule};
use ruff_python_ast::Expr;
use ruff_text_size::Ranged;

const DEFAULT_REQUIRED_KEYS: &[&str] = &["license"];
const DEFAULT_REQUIRED_KEYS_APP: &[&str] = &["currency", "images", "license", "support"];
const DEFAULT_DEPRECATED_KEYS: &[&str] = &["description"];
const DEFAULT_KEYS_VALUES_TRUE: &[&str] = &["active", "installable"];

pub const MANIFEST_REQUIRED_KEY: Rule = Rule {
    code: "C8102",
    name: "manifest-required-key",
    summary: "A required key is missing from the manifest.",
    doc: r#"
## What it does

Checks that the manifest has every required key. By default that is only
`license`.

## Why is this bad?

Without a `license` Odoo assumes LGPL-3, which may not be what the author
intended, and the Odoo Apps store rejects the module.

## Configuration

```toml
[tool.odoo-lint.rules.manifest-required-key]
keys = ["license", "website"]
```

## Fix safety

Safe, when [`manifest-defaults`](../configuration.md#manifest-defaults) has a
value for the key: it is added as the last entry. Otherwise there is no fix.
"#,
    check: Check::Manifest(check_required_keys),
    min_odoo: None,
    max_odoo: None,
};

pub const MANIFEST_DEPRECATED_KEY: Rule = Rule {
    code: "C8103",
    name: "manifest-deprecated-key",
    summary: "The manifest uses a deprecated key.",
    doc: r#"
## What it does

Reports deprecated manifest keys. By default that is `description`.

## Why is this bad?

Odoo shows `README.rst` or `static/description/index.html` instead of the
`description` key, so the text in the manifest is stale documentation nobody
sees.

## Configuration

```toml
[tool.odoo-lint.rules.manifest-deprecated-key]
keys = ["description", "active"]
```
"#,
    check: Check::Manifest(check_deprecated_keys),
    min_odoo: None,
    max_odoo: None,
};

pub const MANIFEST_SUPERFLUOUS_KEY: Rule = Rule {
    code: "C8116",
    name: "manifest-superfluous-key",
    summary: "A manifest key is set to its default value.",
    doc: r#"
## What it does

Reports manifest keys whose value is the default anyway: a falsy value such as
`False`, `""` or `[]`, or a truthy value for keys that default to `True`
(`active` and `installable`).

## Why is this bad?

Default values add noise and make real differences between modules harder to
spot.

## Example

```python
{
    "installable": True,
    "application": False,
}
```

Use instead: remove both keys.

## Configuration

```toml
[tool.odoo-lint.rules.manifest-superfluous-key]
# Keys whose default value is True
keys-values-true = ["active", "installable"]
```

## Fix safety

Safe: the key is removed when its entry has its lines to itself.
"#,
    check: Check::Manifest(check_superfluous_keys),
    min_odoo: None,
    max_odoo: None,
};

pub const MANIFEST_REQUIRED_KEY_APP: Rule = Rule {
    code: "C8119",
    name: "manifest-required-key-app",
    summary: "A key required for paid apps is missing from the manifest.",
    doc: r#"
## What it does

For manifests with a `price` (apps sold on the Odoo Apps store), checks that
the keys the store needs are present. By default those are `currency`,
`images`, `license` and `support`; keys already required by
[C8102](C8102.md) are not reported twice.

## Why is this bad?

The Odoo Apps store rejects or misrepresents paid modules without them.

## Configuration

```toml
[tool.odoo-lint.rules.manifest-required-key-app]
keys = ["currency", "images", "license", "support"]
```

## Fix safety

Safe, when [`manifest-defaults`](../configuration.md#manifest-defaults) has a
value for the key: it is added as the last entry. Otherwise there is no fix.
"#,
    check: Check::Manifest(check_required_keys_app),
    min_odoo: None,
    max_odoo: None,
};

fn required_keys(ctx: &ManifestContext) -> Vec<String> {
    list_or(
        ctx.settings.config.rules().manifest_required_key.as_ref(),
        |c| c.keys.as_ref(),
        DEFAULT_REQUIRED_KEYS,
    )
}

fn check_required_keys(ctx: &ManifestContext, reporter: &mut Reporter) {
    for key in required_keys(ctx) {
        if ctx.manifest.get(&key).is_none() {
            reporter
                .report(
                    &MANIFEST_REQUIRED_KEY,
                    ctx.manifest.dict().start(),
                    format!("Missing required key \"{key}\" in manifest file"),
                )
                .fix = add_default(ctx, &key);
        }
    }
}

fn check_deprecated_keys(ctx: &ManifestContext, reporter: &mut Reporter) {
    let deprecated = list_or(
        ctx.settings.config.rules().manifest_deprecated_key.as_ref(),
        |c| c.keys.as_ref(),
        DEFAULT_DEPRECATED_KEYS,
    );
    for key in deprecated {
        if ctx.manifest.get(&key).is_some() {
            reporter.report(
                &MANIFEST_DEPRECATED_KEY,
                key_or_dict(ctx, &key),
                format!("Deprecated key \"{key}\" in manifest file"),
            );
        }
    }
}

fn check_superfluous_keys(ctx: &ManifestContext, reporter: &mut Reporter) {
    let values_true = list_or(
        ctx.settings.config.rules().manifest_superfluous_key.as_ref(),
        |c| c.keys_values_true.as_ref(),
        DEFAULT_KEYS_VALUES_TRUE,
    );
    // Like a Python dict: one entry per key, the last value wins.
    let mut seen: Vec<&str> = Vec::new();
    for (key, _, _) in ctx.manifest.entries() {
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let (key_expr, value): (&Expr, &Expr) = ctx.manifest.entry(key).expect("key comes from entries()");
        let Some(truthy) = is_truthy(value) else { continue };
        let default_true = values_true.iter().any(|k| k == key);
        if truthy == default_true {
            let shown = str_of(value).unwrap_or_default();
            let violation = reporter.report(
                &MANIFEST_SUPERFLUOUS_KEY,
                key_expr.start(),
                format!("Manifest superfluous key \"{key}\". It is the same as the default value: {shown}. Better remove it"),
            );
            let unique = ctx.manifest.entries().filter(|(k, _, _)| *k == key).count() == 1;
            if unique {
                violation.fix = remove_entry(ctx.source, key_expr, value)
                    .map(|edit| Fix::safe(format!("Remove `{key}`"), vec![edit]));
            }
        }
    }
}

/// Deletes a `key: value,` entry that has its lines to itself, with the
/// line break after it.
fn remove_entry(source: &str, key: &Expr, value: &Expr) -> Option<Edit> {
    let start = key.start().to_usize();
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    if !source[line_start..start].trim().is_empty() {
        return None;
    }
    let after = &source[value.end().to_usize()..];
    let rest = after.trim_start_matches([' ', '\t']);
    let rest = rest.strip_prefix(',').unwrap_or(rest).trim_start_matches([' ', '\t']);
    let rest = rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n'))?;
    let end = source.len() - rest.len();
    Some(Edit::delete(line_start, end))
}

fn check_required_keys_app(ctx: &ManifestContext, reporter: &mut Reporter) {
    if ctx.manifest.get("price").is_none() {
        return;
    }
    let already_required = required_keys(ctx);
    let app_keys = list_or(
        ctx.settings.config.rules().manifest_required_key_app.as_ref(),
        |c| c.keys.as_ref(),
        DEFAULT_REQUIRED_KEYS_APP,
    );
    for key in app_keys {
        if !already_required.contains(&key) && ctx.manifest.get(&key).is_none() {
            reporter
                .report(
                    &MANIFEST_REQUIRED_KEY_APP,
                    ctx.manifest.dict().start(),
                    format!("Missing required key \"{key}\" in manifest file for modules with price."),
                )
                .fix = add_default(ctx, &key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::run_manifest_rule;
    use crate::diagnostics::Violation;
    use crate::settings::Settings;

    fn run(rule: &Rule, src: &str) -> Vec<Violation> {
        run_manifest_rule(rule, src, "m", &Settings::default())
    }

    #[test]
    fn required_key() {
        let v = run(&MANIFEST_REQUIRED_KEY, "{'name': 'x'}\n");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].message, "Missing required key \"license\" in manifest file");
        assert!(run(&MANIFEST_REQUIRED_KEY, "{'license': 'AGPL-3'}\n").is_empty());
    }

    #[test]
    fn deprecated_key() {
        let v = run(
            &MANIFEST_DEPRECATED_KEY,
            "{\n  'name': 'x',\n  'description': 'y',\n}\n",
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 3);
        assert_eq!(v[0].message, "Deprecated key \"description\" in manifest file");
    }

    #[test]
    fn superfluous_keys() {
        let src =
            "{\n 'installable': True,\n 'application': False,\n 'demo': [],\n 'active': False,\n 'name': 'x',\n}\n";
        let v = run(&MANIFEST_SUPERFLUOUS_KEY, src);
        let messages: Vec<_> = v.iter().map(|v| (v.line, v.message.as_str())).collect();
        assert_eq!(
            messages,
            vec![
                (2, "Manifest superfluous key \"installable\". It is the same as the default value: True. Better remove it"),
                (3, "Manifest superfluous key \"application\". It is the same as the default value: False. Better remove it"),
                (4, "Manifest superfluous key \"demo\". It is the same as the default value: []. Better remove it"),
            ]
        );
    }

    #[test]
    fn duplicate_keys_count_once() {
        let v = run(&MANIFEST_SUPERFLUOUS_KEY, "{\n 'demo': [],\n 'demo': [],\n}\n");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 3);
    }

    #[test]
    fn required_key_app() {
        let v = run(
            &MANIFEST_REQUIRED_KEY_APP,
            "{'price': 10, 'images': [], 'license': 'OPL-1'}\n",
        );
        let keys: Vec<_> = v.iter().map(|v| v.message.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "Missing required key \"currency\" in manifest file for modules with price.",
                "Missing required key \"support\" in manifest file for modules with price.",
            ]
        );
        assert!(run(&MANIFEST_REQUIRED_KEY_APP, "{'name': 'free'}\n").is_empty());
    }
}
