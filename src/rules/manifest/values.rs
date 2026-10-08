//! Manifest values: license, version format, development status, category,
//! summary, maintainers, support email, website and external assets.

use super::{key_or_dict, string_elements};
use crate::checker::{ManifestContext, Reporter};
use crate::config::list_or;
use crate::odoo_version::OdooVersion;
use crate::pyliteral::{is_truthy, repr_str, str_of};
use crate::rules::{Check, Rule};
use regex::Regex;
use ruff_python_ast::Expr;
use ruff_text_size::Ranged;
use std::sync::LazyLock;

const DEFAULT_LICENSES: &[&str] = &[
    "AGPL-3",
    "GPL-2 or any later version",
    "GPL-2",
    "GPL-3 or any later version",
    "GPL-3",
    "LGPL-3",
    "OEEL-1",
    "Other OSI approved licence",
    "Other proprietary",
];
const DEFAULT_DEVELOPMENT_STATUS: &[&str] = &["Alpha", "Beta", "Mature", "Production/Stable"];
const DEFAULT_CATEGORIES_APP: &[&str] = &[
    "Accounting",
    "Discuss",
    "Document Management",
    "eCommerce",
    "Extra Tools",
    "Human Resources",
    "Industries",
    "Localization",
    "Manufacturing",
    "Marketing",
    "Point of Sale",
    "Productivity",
    "Project",
    "Purchases",
    "Sales",
    "Tutorial",
    "Warehouse",
    "Website",
];
/// Every Odoo series, as in pylint-odoo's `valid-odoo-versions` default.
const DEFAULT_VALID_ODOO_VERSIONS: &[&str] = &[
    "4.2", "5.0", "6.0", "6.1", "7.0", "8.0", "9.0", "10.0", "11.0", "12.0", "13.0", "14.0", "15.0", "16.0", "17.0",
    "18.0", "19.0", "20.0",
];
const DEFAULT_VERSION_FORMAT: &str = r"({valid_odoo_versions})\.\d+\.\d+\.\d+$";

static EMAIL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_.+-]+@[a-zA-Z0-9-]+\.[a-zA-Z0-9-.]+\n?\z").unwrap());
static DOMAIN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z0-9][a-z0-9-_]{0,61}[a-z]$").unwrap()
});

pub const LICENSE_ALLOWED: Rule = Rule {
    code: "C8105",
    name: "license-allowed",
    summary: "The manifest license is not in the allowed list.",
    doc: r#"
## What it does

Checks the manifest `license` against a list of allowed licenses. The default
list holds the licenses Odoo itself recognises.

## Why is this bad?

Odoo and the Odoo Apps store only understand specific license strings; a
typo such as `AGPL-3.0` shows up as an unknown license.

## Configuration

```toml
[tool.odoo-lint.rules.license-allowed]
allowed = ["AGPL-3", "LGPL-3"]
```
"#,
    check: Check::Manifest(check_license),
    min_odoo: None,
    max_odoo: None,
};

pub const MANIFEST_VERSION_FORMAT: Rule = Rule {
    code: "C8106",
    name: "manifest-version-format",
    summary: "The manifest version does not follow `<odoo series>.x.y.z`.",
    doc: r#"
## What it does

Checks that the manifest `version` is the Odoo series followed by three
numbers, such as `17.0.1.0.0`.

## Why is this bad?

Odoo and the OCA tooling derive the module's Odoo series and its migration
scripts from this version. A version such as `1.0` makes upgrades unreliable.

## Configuration

```toml
[tool.odoo-lint.rules.manifest-version-format]
# Series accepted as the first part; default: every Odoo series
valid-odoo-versions = ["17.0"]
# Regex the version must match from the start
format = '({valid_odoo_versions})\.\d+\.\d+\.\d+$'
```

Set `valid-odoo-versions` to the series of your branch to also catch modules
that were not migrated.
"#,
    check: Check::Manifest(check_version_format),
    min_odoo: None,
    max_odoo: None,
};

pub const DEVELOPMENT_STATUS_ALLOWED: Rule = Rule {
    code: "C8111",
    name: "development-status-allowed",
    summary: "The manifest `development_status` is not an allowed value.",
    doc: r#"
## What it does

Checks `development_status` against the values the OCA uses: `Alpha`, `Beta`,
`Mature` and `Production/Stable`.

## Why is this bad?

The OCA tooling generates badges and the Apps store listing from it; other
values are shown as unknown.

## Configuration

```toml
[tool.odoo-lint.rules.development-status-allowed]
allowed = ["Alpha", "Beta", "Mature", "Production/Stable"]
```
"#,
    check: Check::Manifest(check_development_status),
    min_odoo: None,
    max_odoo: None,
};

pub const CATEGORY_ALLOWED: Rule = Rule {
    code: "C8114",
    name: "category-allowed",
    summary: "The manifest category is not in the allowed list.",
    doc: r#"
## What it does

Checks the `category` of modules without a `price` against a list of allowed
categories. The list is empty by default, which allows every category.

## Configuration

```toml
[tool.odoo-lint.rules.category-allowed]
allowed = ["Accounting", "Sales", "Hidden"]
```
"#,
    check: Check::Manifest(check_category),
    min_odoo: None,
    max_odoo: None,
};

pub const CATEGORY_ALLOWED_APP: Rule = Rule {
    code: "C8117",
    name: "category-allowed-app",
    summary: "The category of a paid app is not an Odoo Apps store category.",
    doc: r#"
## What it does

For manifests with a `price`, checks that `category` is one of the categories
of the Odoo Apps store, such as `Accounting`, `Sales` or `Website`.

## Why is this bad?

The Apps store files the app under an unknown category, where customers do
not find it.

## Configuration

```toml
[tool.odoo-lint.rules.category-allowed-app]
allowed = ["Accounting", "Sales"]
```
"#,
    check: Check::Manifest(check_category_app),
    min_odoo: None,
    max_odoo: None,
};

pub const MANIFEST_SUMMARY_MULTILINE: Rule = Rule {
    code: "C8120",
    name: "manifest-summary-multiline",
    summary: "The manifest summary spans several lines.",
    doc: r#"
## What it does

Checks that `summary` is a single line.

## Why is this bad?

From Odoo 20.0 the summary is shown as a one-line description; a newline
breaks the layout. Longer texts belong in the README.
"#,
    check: Check::Manifest(check_summary_multiline),
    min_odoo: Some(OdooVersion::new(20, 0)),
    max_odoo: None,
};

pub const MANIFEST_MAINTAINERS_LIST: Rule = Rule {
    code: "E8104",
    name: "manifest-maintainers-list",
    summary: "The manifest `maintainers` is not a list of strings.",
    doc: r#"
## What it does

Checks that `maintainers` is a list of GitHub user names (strings).

## Why is this bad?

The OCA tooling reads it to assign reviewers and to render the maintainers
section of the README; other shapes break it.

## Example

```python
{
    "maintainers": "johndoe",
}
```

Use instead:

```python
{
    "maintainers": ["johndoe"],
}
```
"#,
    check: Check::Manifest(check_maintainers),
    min_odoo: None,
    max_odoo: None,
};

pub const INVALID_EMAIL: Rule = Rule {
    code: "R8181",
    name: "invalid-email",
    summary: "The manifest `support` is not a valid email address.",
    doc: r#"
## What it does

Checks that `support` in the manifest is an email address.

## Why is this bad?

The Odoo Apps store shows it as the contact for customers of the module.
"#,
    check: Check::Manifest(check_support_email),
    min_odoo: None,
    max_odoo: None,
};

pub const WEBSITE_MANIFEST_KEY_NOT_VALID_URI: Rule = Rule {
    code: "W8114",
    name: "website-manifest-key-not-valid-uri",
    summary: "The manifest `website` is not a valid http(s) URL.",
    doc: r#"
## What it does

Checks that `website` is an `http://` or `https://` URL with a valid domain
and without spaces. The message says what is wrong with it.

## Why is this bad?

Odoo links to the website from the Apps menu; an invalid URL is a dead link.
"#,
    check: Check::Manifest(check_website),
    min_odoo: None,
    max_odoo: None,
};

pub const MANIFEST_EXTERNAL_ASSETS: Rule = Rule {
    code: "W8162",
    name: "manifest-external-assets",
    summary: "An asset in the manifest is loaded from an external URL.",
    doc: r#"
## What it does

Reports entries in the manifest `assets` that point to an external URL
instead of a file in the module.

## Why is this bad?

Assets from a CDN break offline and air-gapped installations, change without
notice and give a third party a way to inject code into every Odoo page. See
[the risks of public CDNs](https://httptoolkit.com/blog/public-cdn-risks/).

## Example

```python
{
    "assets": {
        "web.assets_backend": ["https://cdn.example.com/chart.js"],
    },
}
```

Use instead: ship the file in the module, for example
`acme_chart/static/lib/chart.js`.
"#,
    check: Check::Manifest(check_external_assets),
    min_odoo: None,
    max_odoo: None,
};

fn check_license(ctx: &ManifestContext, reporter: &mut Reporter) {
    let Some(value) = ctx.manifest.get("license") else {
        return;
    };
    if is_truthy(value) != Some(true) {
        return;
    }
    let allowed = list_or(
        ctx.settings.config.rules().license_allowed.as_ref(),
        |c| c.allowed.as_ref(),
        DEFAULT_LICENSES,
    );
    let license = str_of(value).unwrap_or_default();
    if !(value.is_string_literal_expr() && allowed.contains(&license)) {
        reporter.report(
            &LICENSE_ALLOWED,
            key_or_dict(ctx, "license"),
            format!("License \"{license}\" not allowed in manifest file."),
        );
    }
}

/// The version regex as pylint-odoo prints it, and as a compiled regex.
fn version_format(ctx: &ManifestContext) -> (String, Option<Regex>) {
    let config = ctx.settings.config.rules().manifest_version_format.as_ref();
    let versions = list_or(config, |c| c.valid_odoo_versions.as_ref(), DEFAULT_VALID_ODOO_VERSIONS);
    let alternatives: Vec<String> = versions.iter().map(|v| regex::escape(v)).collect();
    let template = config
        .and_then(|c| c.format.clone())
        .unwrap_or_else(|| DEFAULT_VERSION_FORMAT.to_string());
    let parsed = template.replace("{valid_odoo_versions}", &alternatives.join("|"));
    // Python's re.match anchors at the start, and `$` also matches before a
    // final newline.
    let rust = match parsed.strip_suffix('$') {
        Some(head) if !head.ends_with('\\') => format!(r"^(?:{head}\n?\z)"),
        _ => format!("^(?:{parsed})"),
    };
    (parsed, Regex::new(&rust).ok())
}

fn check_version_format(ctx: &ManifestContext, reporter: &mut Reporter) {
    let Some(version) = ctx.manifest.get_str("version") else {
        return;
    };
    if version.is_empty() {
        return;
    }
    let (parsed, regex) = version_format(ctx);
    if regex.is_some_and(|re| !re.is_match(version)) {
        reporter.report(
            &MANIFEST_VERSION_FORMAT,
            key_or_dict(ctx, "version"),
            format!("Wrong Version Format \"{version}\" in manifest file. Regex to match: \"{parsed}\""),
        );
    }
}

fn check_development_status(ctx: &ManifestContext, reporter: &mut Reporter) {
    let Some(value) = ctx.manifest.get("development_status") else {
        return;
    };
    if is_truthy(value) != Some(true) {
        return;
    }
    let allowed = list_or(
        ctx.settings.config.rules().development_status_allowed.as_ref(),
        |c| c.allowed.as_ref(),
        DEFAULT_DEVELOPMENT_STATUS,
    );
    let status = str_of(value).unwrap_or_default();
    if !(value.is_string_literal_expr() && allowed.contains(&status)) {
        reporter.report(
            &DEVELOPMENT_STATUS_ALLOWED,
            key_or_dict(ctx, "development_status"),
            format!(
                "Manifest key development_status \"{status}\" not allowed. Use one of: {}.",
                allowed.join(", ")
            ),
        );
    }
}

/// The category as a string, when it is truthy.
fn category(ctx: &ManifestContext) -> Option<(String, bool)> {
    let value = ctx.manifest.get("category")?;
    (is_truthy(value) == Some(true)).then(|| (str_of(value).unwrap_or_default(), value.is_string_literal_expr()))
}

fn check_category(ctx: &ManifestContext, reporter: &mut Reporter) {
    if ctx.manifest.get("price").is_some() {
        return;
    }
    let Some((category, is_str)) = category(ctx) else {
        return;
    };
    let allowed = list_or(
        ctx.settings.config.rules().category_allowed.as_ref(),
        |c| c.allowed.as_ref(),
        &[],
    );
    if !allowed.is_empty() && !(is_str && allowed.contains(&category)) {
        reporter.report(
            &CATEGORY_ALLOWED,
            key_or_dict(ctx, "category"),
            format!("Category \"{category}\" not allowed in manifest file."),
        );
    }
}

fn check_category_app(ctx: &ManifestContext, reporter: &mut Reporter) {
    if ctx.manifest.get("price").is_none() {
        return;
    }
    let Some((category, is_str)) = category(ctx) else {
        return;
    };
    let allowed = list_or(
        ctx.settings.config.rules().category_allowed_app.as_ref(),
        |c| c.allowed.as_ref(),
        DEFAULT_CATEGORIES_APP,
    );
    if !allowed.is_empty() && !(is_str && allowed.contains(&category)) {
        reporter.report(
            &CATEGORY_ALLOWED_APP,
            key_or_dict(ctx, "category"),
            format!("Category \"{category}\" not allowed in manifest file for modules with price."),
        );
    }
}

fn check_summary_multiline(ctx: &ManifestContext, reporter: &mut Reporter) {
    if ctx.manifest.get_str("summary").is_some_and(|s| s.contains('\n')) {
        reporter.report(
            &MANIFEST_SUMMARY_MULTILINE,
            key_or_dict(ctx, "summary"),
            "Summary in manifest file should be a one-line short description, found newline character",
        );
    }
}

fn check_maintainers(ctx: &ManifestContext, reporter: &mut Reporter) {
    let Some(value) = ctx.manifest.get("maintainers") else {
        return;
    };
    if is_truthy(value) != Some(true) {
        return;
    }
    let valid = match value {
        Expr::List(list) => list.elts.iter().all(Expr::is_string_literal_expr),
        _ => false,
    };
    if !valid {
        reporter.report(
            &MANIFEST_MAINTAINERS_LIST,
            key_or_dict(ctx, "maintainers"),
            "The maintainers key in the manifest file must be a list of strings",
        );
    }
}

fn check_support_email(ctx: &ManifestContext, reporter: &mut Reporter) {
    let Some(support) = ctx.manifest.get_str("support") else {
        return;
    };
    if !support.is_empty() && !EMAIL_RE.is_match(support) {
        reporter.report(
            &INVALID_EMAIL,
            key_or_dict(ctx, "support"),
            format!("Invalid email \"{support}\""),
        );
    }
}

/// Scheme and network location as Python's `urllib.parse.urlsplit` finds them.
fn url_scheme_and_netloc(url: &str) -> (String, String) {
    let url: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let mut rest = url.as_str();
    let mut scheme = String::new();
    if let Some(i) = url.find(':') {
        let candidate = &url[..i];
        let mut chars = candidate.chars();
        let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
        if valid {
            scheme = candidate.to_ascii_lowercase();
            rest = &url[i + 1..];
        }
    }
    let netloc = rest
        .strip_prefix("//")
        .map(|r| r.split(['/', '?', '#']).next().unwrap_or("").to_string())
        .unwrap_or_default();
    (scheme, netloc)
}

/// Why `url` is not a valid website, following pylint-odoo's checks in order.
fn invalid_url_reason(url: &str) -> Option<String> {
    if url.chars().any(char::is_whitespace) {
        return Some("URL must not contain white spaces, they must be encoded".into());
    }
    let (scheme, netloc) = url_scheme_and_netloc(url);
    if scheme != "https" && scheme != "http" {
        return Some("URL needs to start with 'http[s]://'".into());
    }
    if netloc.is_empty() {
        return Some("Invalid URL domain not identified".into());
    }
    if netloc.contains("__") {
        return Some(format!(
            "Domain section must not contain double underscore '__' because of security issues {netloc}"
        ));
    }
    // IDNA: empty or over-long labels cannot be encoded (a final dot is fine);
    // non-ASCII labels become `xn--` labels, which the domain regex accepts.
    let labels: Vec<&str> = netloc.split('.').collect();
    let last = labels.len() - 1;
    let encodable = labels
        .iter()
        .enumerate()
        .all(|(i, label)| (!label.is_empty() || (i == last && i > 0)) && label.len() < 64);
    if !encodable {
        return Some(format!("Unable to encode/decode domain section {netloc}"));
    }
    let ascii: Vec<String> = labels
        .iter()
        .map(|l| {
            if l.is_ascii() {
                l.to_string()
            } else {
                "xn--a".to_string()
            }
        })
        .collect();
    if !DOMAIN_RE.is_match(&ascii.join(".")) {
        return Some(format!("Domain {} contains invalid characters", repr_str(&netloc)));
    }
    None
}

fn check_website(ctx: &ManifestContext, reporter: &mut Reporter) {
    let Some(website) = ctx.manifest.get_str("website") else {
        return;
    };
    if website.is_empty() {
        return;
    }
    if let Some(reason) = invalid_url_reason(website) {
        reporter.report(
            &WEBSITE_MANIFEST_KEY_NOT_VALID_URI,
            key_or_dict(ctx, "website"),
            format!("Website \"{website}\" in manifest key is not a valid URI. {reason}"),
        );
    }
}

fn is_external_url(url: &str) -> bool {
    !url_scheme_and_netloc(url).0.is_empty()
}

fn check_external_assets(ctx: &ManifestContext, reporter: &mut Reporter) {
    let Some(Expr::Dict(assets)) = ctx.manifest.get("assets") else {
        return;
    };
    for item in &assets.items {
        let elements = match &item.value {
            Expr::List(list) => &list.elts[..],
            Expr::Tuple(tuple) => &tuple.elts[..],
            _ => continue,
        };
        for element in elements {
            let urls: Vec<&str> = match element {
                Expr::StringLiteral(s) => vec![s.value.to_str()],
                // ('include', 'other.bundle') style directives
                Expr::List(_) | Expr::Tuple(_) => string_elements(element).into_iter().map(|(s, _)| s).collect(),
                _ => continue,
            };
            for url in urls.into_iter().filter(|u| is_external_url(u)) {
                reporter.report(
                    &MANIFEST_EXTERNAL_ASSETS,
                    element.start(),
                    format!(
                        "Asset {url} should be distributed with module's source code. More info at https://httptoolkit.com/blog/public-cdn-risks/"
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::run_manifest_rule;
    use crate::config::OdooLintConfig;
    use crate::diagnostics::Violation;
    use crate::settings::{CliOverrides, Settings};

    fn run(rule: &Rule, src: &str) -> Vec<Violation> {
        run_manifest_rule(rule, src, "m", &Settings::default())
    }

    fn messages(rule: &Rule, src: &str) -> Vec<String> {
        run(rule, src).into_iter().map(|v| v.message).collect()
    }

    #[test]
    fn license() {
        assert!(run(&LICENSE_ALLOWED, "{'license': 'AGPL-3'}\n").is_empty());
        assert!(run(&LICENSE_ALLOWED, "{'license': ''}\n").is_empty());
        assert_eq!(
            messages(&LICENSE_ALLOWED, "{'license': 'AGPL-3.0'}\n"),
            vec!["License \"AGPL-3.0\" not allowed in manifest file."]
        );
    }

    #[test]
    fn version_format_default() {
        assert!(run(&MANIFEST_VERSION_FORMAT, "{'version': '17.0.1.0.0'}\n").is_empty());
        assert!(run(&MANIFEST_VERSION_FORMAT, "{'version': '6.1.1.0.0'}\n").is_empty());
        let m = messages(&MANIFEST_VERSION_FORMAT, "{'version': '1.0'}\n");
        assert_eq!(m.len(), 1);
        assert!(m[0]
            .starts_with("Wrong Version Format \"1.0\" in manifest file. Regex to match: \"(4\\.2|5\\.0|6\\.0|6\\.1|"));
        assert!(m[0].ends_with("|20\\.0)\\.\\d+\\.\\d+\\.\\d+$\""));
        assert_eq!(run(&MANIFEST_VERSION_FORMAT, "{'version': '17.0.1.0.0.1'}\n").len(), 1);
    }

    #[test]
    fn version_format_configured() {
        let config =
            OdooLintConfig::from_odoo_lint_toml("[rules.manifest-version-format]\nvalid-odoo-versions = [\"17.0\"]\n")
                .unwrap();
        let settings = Settings::new(config, None, CliOverrides::default()).unwrap().0;
        let v = run_manifest_rule(&MANIFEST_VERSION_FORMAT, "{'version': '16.0.1.0.0'}\n", "m", &settings);
        assert_eq!(v.len(), 1);
        assert!(v[0]
            .message
            .ends_with("Regex to match: \"(17\\.0)\\.\\d+\\.\\d+\\.\\d+$\""));
    }

    #[test]
    fn development_status() {
        assert!(run(&DEVELOPMENT_STATUS_ALLOWED, "{'development_status': 'Beta'}\n").is_empty());
        assert_eq!(
            messages(&DEVELOPMENT_STATUS_ALLOWED, "{'development_status': 'beta'}\n"),
            vec!["Manifest key development_status \"beta\" not allowed. Use one of: Alpha, Beta, Mature, Production/Stable."]
        );
    }

    #[test]
    fn categories() {
        // Without configuration every category is allowed for free modules.
        assert!(run(&CATEGORY_ALLOWED, "{'category': 'Anything'}\n").is_empty());
        assert!(run(&CATEGORY_ALLOWED_APP, "{'price': 1, 'category': 'Sales'}\n").is_empty());
        assert_eq!(
            messages(&CATEGORY_ALLOWED_APP, "{'price': 1, 'category': 'Stuff'}\n"),
            vec!["Category \"Stuff\" not allowed in manifest file for modules with price."]
        );
        assert!(run(&CATEGORY_ALLOWED_APP, "{'category': 'Stuff'}\n").is_empty());
    }

    #[test]
    fn summary_multiline() {
        assert_eq!(run(&MANIFEST_SUMMARY_MULTILINE, "{'summary': 'a\\nb'}\n").len(), 1);
        assert!(run(&MANIFEST_SUMMARY_MULTILINE, "{'summary': 'a b'}\n").is_empty());
        assert_eq!(MANIFEST_SUMMARY_MULTILINE.min_odoo, Some(OdooVersion::new(20, 0)));
    }

    #[test]
    fn maintainers() {
        assert!(run(&MANIFEST_MAINTAINERS_LIST, "{'maintainers': ['a', 'b']}\n").is_empty());
        assert!(run(&MANIFEST_MAINTAINERS_LIST, "{'maintainers': []}\n").is_empty());
        assert_eq!(run(&MANIFEST_MAINTAINERS_LIST, "{'maintainers': 'a'}\n").len(), 1);
        assert_eq!(run(&MANIFEST_MAINTAINERS_LIST, "{'maintainers': ('a',)}\n").len(), 1);
        assert_eq!(run(&MANIFEST_MAINTAINERS_LIST, "{'maintainers': ['a', 1]}\n").len(), 1);
    }

    #[test]
    fn support_email() {
        assert!(run(&INVALID_EMAIL, "{'support': 'help@example.com'}\n").is_empty());
        assert_eq!(
            messages(&INVALID_EMAIL, "{'support': 'help.example.com'}\n"),
            vec!["Invalid email \"help.example.com\""]
        );
    }

    #[test]
    fn website_reasons() {
        assert_eq!(invalid_url_reason("https://www.odoo-community.org"), None);
        assert_eq!(invalid_url_reason("http://example.com/path?q=1#x"), None);
        assert_eq!(
            invalid_url_reason("https://exa mple.com").as_deref(),
            Some("URL must not contain white spaces, they must be encoded")
        );
        assert_eq!(
            invalid_url_reason("www.example.com").as_deref(),
            Some("URL needs to start with 'http[s]://'")
        );
        assert_eq!(
            invalid_url_reason("https://").as_deref(),
            Some("Invalid URL domain not identified")
        );
        assert_eq!(
            invalid_url_reason("https://a__b.com").as_deref(),
            Some("Domain section must not contain double underscore '__' because of security issues a__b.com")
        );
        assert_eq!(
            invalid_url_reason("https://example.com:8080").as_deref(),
            Some("Domain 'example.com:8080' contains invalid characters")
        );
        assert_eq!(
            invalid_url_reason("https://a..com").as_deref(),
            Some("Unable to encode/decode domain section a..com")
        );
    }

    #[test]
    fn website_message() {
        assert_eq!(
            messages(&WEBSITE_MANIFEST_KEY_NOT_VALID_URI, "{'website': 'www.example.com'}\n"),
            vec![
                "Website \"www.example.com\" in manifest key is not a valid URI. URL needs to start with 'http[s]://'"
            ]
        );
        assert!(run(&WEBSITE_MANIFEST_KEY_NOT_VALID_URI, "{'website': ''}\n").is_empty());
    }

    #[test]
    fn external_assets() {
        let src = "{\n 'assets': {\n  'web.assets_backend': [\n   'acme/static/src/x.js',\n   'https://cdn.example.com/a.js',\n   ('include', 'web._assets_helpers'),\n   ('after', 'x.js', '//cdn.example.com/b.js'),\n  ],\n }\n}\n";
        let v = run(&MANIFEST_EXTERNAL_ASSETS, src);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 5);
        assert_eq!(
            v[0].message,
            "Asset https://cdn.example.com/a.js should be distributed with module's source code. More info at https://httptoolkit.com/blog/public-cdn-risks/"
        );
    }
}
