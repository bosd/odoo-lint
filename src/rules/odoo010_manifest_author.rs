use crate::config::OdooLintConfig;
use crate::diagnostics::Violation;

pub fn check_manifest(file_path: &str, content: &str, module_name: &str, config: &OdooLintConfig) -> Vec<Violation> {
    let expected = config.get_expected_author(module_name);
    if !content.contains(&format!("'author': '{}'", expected))
        && !content.contains(&format!("\"author\": \"{}\"", expected))
    {
        return vec![Violation {
            file_path: file_path.to_string(),
            line: 1,
            rule_code: "ODOO010",
            message: format!(
                "Manifest author does not match expected '{}' for module '{}'",
                expected, module_name
            ),
        }];
    }
    vec![]
}
