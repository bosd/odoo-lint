//! Output formats for violations.

use crate::diagnostics::Violation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// pylint-style `path:line:col: CODE: message (name)`
    #[default]
    Text,
    /// JSON array, 1-based line and column
    Json,
    /// GitHub Actions workflow commands, shown as annotations on pull requests
    Github,
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
    }
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
    fn github() {
        assert_eq!(
            render(OutputFormat::Github, &[violation()]),
            "::warning file=addons/a%2Cb/__manifest__.py,line=4,col=5,title=C8101 (manifest-required-author)::One of the following authors must be present in manifest: 'X'\n"
        );
    }
}
