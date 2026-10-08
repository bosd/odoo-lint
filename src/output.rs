//! Output formats for violations.

use crate::diagnostics::Violation;
use crate::rules::{self, Rule};
use serde_json::json;
use std::collections::HashMap;

/// Base URL of the rule pages in the documentation.
const RULE_DOCS_URL: &str = "https://odoo-lint.readthedocs.io/en/latest/rules/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// pylint-style `path:line:col: CODE: message (name)`
    #[default]
    Text,
    /// JSON array, 1-based line and column
    Json,
    /// GitHub Actions workflow commands, shown as annotations on pull requests
    Github,
    /// SARIF 2.1.0, for GitHub code scanning, reviewdog (Forgejo, Gitea) and IDEs
    Sarif,
    /// GitLab Code Quality report, shown in merge requests
    Gitlab,
}

pub fn render(format: OutputFormat, violations: &[Violation]) -> String {
    match format {
        OutputFormat::Text => violations.iter().map(|v| format!("{v}\n")).collect(),
        OutputFormat::Json => {
            let mut json = serde_json::to_string_pretty(violations).expect("violations serialize");
            json.push('\n');
            json
        }
        OutputFormat::Github => violations.iter().map(github_annotation).collect(),
        OutputFormat::Sarif => pretty(&sarif(violations)),
        OutputFormat::Gitlab => pretty(&gitlab(violations)),
    }
}

fn pretty(value: &serde_json::Value) -> String {
    let mut out = serde_json::to_string_pretty(value).expect("JSON serializes");
    out.push('\n');
    out
}

/// Repository-relative path with `/` separators, as SARIF and GitLab expect.
fn report_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    path.strip_prefix("./").unwrap_or(&path).to_string()
}

fn rule_url(code: &str) -> String {
    format!("{RULE_DOCS_URL}{code}.html")
}

/// https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html
fn sarif(violations: &[Violation]) -> serde_json::Value {
    let rule_index: HashMap<&str, usize> = rules::ALL.iter().enumerate().map(|(i, r)| (r.code, i)).collect();
    let level = |error: bool| if error { "error" } else { "warning" };
    let sarif_rules: Vec<_> = rules::ALL
        .iter()
        .map(|rule: &Rule| {
            json!({
                "id": rule.code,
                "name": rule.name,
                "shortDescription": { "text": rule.summary },
                "fullDescription": { "text": rule.summary },
                "help": { "text": rule.summary, "markdown": rule.to_markdown() },
                "helpUri": rule_url(rule.code),
                "defaultConfiguration": { "level": level(matches!(rule.code.as_bytes()[0], b'E' | b'F')) },
            })
        })
        .collect();
    let results: Vec<_> = violations
        .iter()
        .map(|v| {
            let mut result = json!({
                "ruleId": v.code,
                "level": level(v.is_error()),
                "message": { "text": v.message },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": report_path(&v.file_path) },
                        "region": { "startLine": v.line, "startColumn": v.column },
                    }
                }],
            });
            if let Some(index) = rule_index.get(v.code.as_str()) {
                result["ruleIndex"] = json!(index);
            }
            result
        })
        .collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "odoo-lint",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://odoo-lint.readthedocs.io/",
                    "rules": sarif_rules,
                }
            },
            "results": results,
        }],
    })
}

/// https://docs.gitlab.com/ci/testing/code_quality/#code-quality-report-format
fn gitlab(violations: &[Violation]) -> serde_json::Value {
    // Fingerprints must be unique per report; number repeats of the same issue.
    let mut seen: HashMap<String, usize> = HashMap::new();
    let issues: Vec<_> = violations
        .iter()
        .map(|v| {
            let path = report_path(&v.file_path);
            let key = format!("{}\0{}\0{}", v.code, path, v.message);
            let occurrence = seen.entry(key.clone()).or_default();
            *occurrence += 1;
            let fingerprint = format!("{:016x}", fnv1a(format!("{key}\0{occurrence}").as_bytes()));
            json!({
                "description": format!("{} ({})", v.message, v.name),
                "check_name": v.code,
                "fingerprint": fingerprint,
                "severity": gitlab_severity(&v.code),
                "location": { "path": path, "lines": { "begin": v.line } },
            })
        })
        .collect();
    json!(issues)
}

/// pylint categories: F(atal) and E(rror) are serious, W(arning) and
/// odoo-lint's own ODOO rules are minor, C(onvention) and R(efactor) are info.
fn gitlab_severity(code: &str) -> &'static str {
    match code.as_bytes().first() {
        Some(b'F') => "critical",
        Some(b'E') => "major",
        Some(b'W') => "minor",
        _ if code.starts_with("ODOO") => "minor",
        _ => "info",
    }
}

/// 64-bit FNV-1a: stable across Rust versions, unlike `DefaultHasher`.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// https://docs.github.com/en/actions/reference/workflow-commands-for-github-actions
fn github_annotation(v: &Violation) -> String {
    let level = if v.is_error() { "error" } else { "warning" };
    format!(
        "::{level} file={},line={},col={},title={}::{}\n",
        escape_property(&v.file_path),
        v.line,
        v.column,
        escape_property(&format!("{} ({})", v.code, v.name)),
        escape_data(&v.message)
    )
}

fn escape_data(s: &str) -> String {
    s.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A")
}

fn escape_property(s: &str) -> String {
    escape_data(s).replace(':', "%3A").replace(',', "%2C")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn violation() -> Violation {
        Violation {
            file_path: "addons/a,b/__manifest__.py".into(),
            line: 4,
            column: 5,
            code: "C8101".into(),
            name: "manifest-required-author".into(),
            message: "One of the following authors must be present in manifest: 'X'".into(),
            fix: None,
        }
    }

    #[test]
    fn text() {
        assert_eq!(
            render(OutputFormat::Text, &[violation()]),
            "addons/a,b/__manifest__.py:4:4: C8101: One of the following authors must be present in manifest: 'X' (manifest-required-author)\n"
        );
    }

    #[test]
    fn json() {
        let out = render(OutputFormat::Json, &[violation()]);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed[0]["code"], "C8101");
        assert_eq!(parsed[0]["column"], 5);
    }

    #[test]
    fn sarif_report() {
        let mut v = violation();
        v.file_path = "./addons/x/__manifest__.py".into();
        let out: serde_json::Value = serde_json::from_str(&render(OutputFormat::Sarif, &[v])).unwrap();
        assert_eq!(out["version"], "2.1.0");
        let run = &out["runs"][0];
        let result = &run["results"][0];
        assert_eq!(result["ruleId"], "C8101");
        assert_eq!(result["level"], "warning");
        let location = &result["locations"][0]["physicalLocation"];
        assert_eq!(location["artifactLocation"]["uri"], "addons/x/__manifest__.py");
        assert_eq!(location["region"]["startLine"], 4);
        let index = result["ruleIndex"].as_u64().unwrap() as usize;
        let rule = &run["tool"]["driver"]["rules"][index];
        assert_eq!(rule["id"], "C8101");
        assert_eq!(
            rule["helpUri"],
            "https://odoo-lint.readthedocs.io/en/latest/rules/C8101.html"
        );
    }

    #[test]
    fn gitlab_report() {
        let a = violation();
        let mut b = violation();
        b.line = 9;
        let out: serde_json::Value = serde_json::from_str(&render(OutputFormat::Gitlab, &[a, b])).unwrap();
        assert_eq!(out[0]["check_name"], "C8101");
        assert_eq!(out[0]["severity"], "info");
        assert_eq!(out[0]["location"]["path"], "addons/a,b/__manifest__.py");
        assert_eq!(out[1]["location"]["lines"]["begin"], 9);
        // Same rule, file and message twice: fingerprints must still differ.
        assert_ne!(out[0]["fingerprint"], out[1]["fingerprint"]);
        assert_eq!(out[0]["fingerprint"].as_str().unwrap().len(), 16);
    }

    #[test]
    fn stable_fingerprint() {
        // FNV-1a test vector: must never change between releases.
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(gitlab_severity("E8103"), "major");
        assert_eq!(gitlab_severity("ODOO001"), "minor");
        assert_eq!(gitlab_severity("C8101"), "info");
    }

    #[test]
    fn github() {
        assert_eq!(
            render(OutputFormat::Github, &[violation()]),
            "::warning file=addons/a%2Cb/__manifest__.py,line=4,col=5,title=C8101 (manifest-required-author)::One of the following authors must be present in manifest: 'X'\n"
        );
    }
}
