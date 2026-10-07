//! Loading of `[tool.odoo-lint]` settings.
//!
//! Lookup starts at the linted path and walks up the directory tree. In each
//! directory `odoo-lint.toml` (settings at the top level) wins over
//! `pyproject.toml` (settings under `[tool.odoo-lint]`). A `pyproject.toml`
//! without a `[tool.odoo-lint]` table is skipped, like ruff does.

use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub const DEFAULT_AUTHOR: &str = "Odoo Community Association (OCA)";

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct OdooLintConfig {
    pub target_version: Option<String>,
    pub rules: Option<RulesConfig>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RulesConfig {
    pub manifest_author: Option<ManifestAuthorConfig>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ManifestAuthorConfig {
    pub default: Option<String>,
    /// Module name (exact) or prefix pattern ending in `*` -> expected author.
    pub mapping: Option<HashMap<String, String>>,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(PathBuf, std::io::Error),
    Parse(PathBuf, toml::de::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(path, err) => write!(f, "cannot read {}: {}", path.display(), err),
            ConfigError::Parse(path, err) => write!(f, "invalid config in {}: {}", path.display(), err),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Deserialize)]
struct PyProject {
    tool: Option<PyProjectTool>,
}

#[derive(Deserialize)]
struct PyProjectTool {
    #[serde(rename = "odoo-lint")]
    odoo_lint: Option<OdooLintConfig>,
}

impl OdooLintConfig {
    /// Parses the contents of a standalone `odoo-lint.toml`.
    pub fn from_odoo_lint_toml(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }

    /// Parses the `[tool.odoo-lint]` table of a `pyproject.toml`.
    /// Returns `Ok(None)` when the table is absent.
    pub fn from_pyproject_toml(content: &str) -> Result<Option<Self>, toml::de::Error> {
        let pyproject: PyProject = toml::from_str(content)?;
        Ok(pyproject.tool.and_then(|tool| tool.odoo_lint))
    }

    /// Loads a config file explicitly, choosing the format by file name.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io(path.to_path_buf(), e))?;
        let parsed = if path.file_name().is_some_and(|n| n == "pyproject.toml") {
            Self::from_pyproject_toml(&content).map(Option::unwrap_or_default)
        } else {
            Self::from_odoo_lint_toml(&content)
        };
        parsed.map_err(|e| ConfigError::Parse(path.to_path_buf(), e))
    }

    /// Finds the nearest config for `start` (a file or directory).
    /// Returns the config and the file it came from, or the default config.
    pub fn discover(start: &Path) -> Result<(Self, Option<PathBuf>), ConfigError> {
        let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
        let first_dir = if start.is_dir() {
            Some(start.as_path())
        } else {
            start.parent()
        };

        for dir in first_dir.into_iter().flat_map(Path::ancestors) {
            let standalone = dir.join("odoo-lint.toml");
            if standalone.is_file() {
                return Ok((Self::from_file(&standalone)?, Some(standalone)));
            }
            let pyproject = dir.join("pyproject.toml");
            if pyproject.is_file() {
                let content = std::fs::read_to_string(&pyproject).map_err(|e| ConfigError::Io(pyproject.clone(), e))?;
                match Self::from_pyproject_toml(&content) {
                    Ok(Some(config)) => return Ok((config, Some(pyproject))),
                    Ok(None) => {}
                    Err(e) => return Err(ConfigError::Parse(pyproject, e)),
                }
            }
        }
        Ok((Self::default(), None))
    }

    /// Expected manifest author for a module folder name.
    ///
    /// An exact module name wins; otherwise the longest matching `prefix*`
    /// pattern wins, so the result does not depend on table order.
    pub fn get_expected_author(&self, module_name: &str) -> String {
        let Some(author_cfg) = self.rules.as_ref().and_then(|r| r.manifest_author.as_ref()) else {
            return DEFAULT_AUTHOR.to_string();
        };
        if let Some(mapping) = &author_cfg.mapping {
            if let Some(expected) = mapping.get(module_name).filter(|_| !module_name.ends_with('*')) {
                return expected.clone();
            }
            let best = mapping
                .iter()
                .filter_map(|(pattern, expected)| {
                    let prefix = pattern.strip_suffix('*')?;
                    module_name.starts_with(prefix).then_some((prefix.len(), expected))
                })
                .max_by_key(|(len, _)| *len);
            if let Some((_, expected)) = best {
                return expected.clone();
            }
        }
        author_cfg.default.clone().unwrap_or_else(|| DEFAULT_AUTHOR.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PYPROJECT: &str = r#"
[project]
name = "my-addons"

[tool.odoo-lint]
target-version = "16.0"

[tool.odoo-lint.rules.manifest-author]
default = "Odoo Community Association (OCA)"

[tool.odoo-lint.rules.manifest-author.mapping]
"mijnbedrijf_*" = "MijnBedrijf B.V."
"acme_*" = "Acme Corp"
"acme_hr_*" = "Acme HR"
"acme_special" = "Someone Else"
"#;

    fn config() -> OdooLintConfig {
        OdooLintConfig::from_pyproject_toml(PYPROJECT).unwrap().unwrap()
    }

    #[test]
    fn parses_pyproject_section() {
        assert_eq!(config().target_version.as_deref(), Some("16.0"));
    }

    #[test]
    fn author_resolution() {
        let c = config();
        assert_eq!(c.get_expected_author("mijnbedrijf_maatwerkmodule"), "MijnBedrijf B.V.");
        assert_eq!(c.get_expected_author("acme_sale"), "Acme Corp");
        assert_eq!(c.get_expected_author("acme_hr_payroll"), "Acme HR");
        assert_eq!(c.get_expected_author("acme_special"), "Someone Else");
        assert_eq!(c.get_expected_author("sale_stock"), DEFAULT_AUTHOR);
    }

    #[test]
    fn default_without_config() {
        assert_eq!(OdooLintConfig::default().get_expected_author("x"), DEFAULT_AUTHOR);
    }

    #[test]
    fn pyproject_without_section_is_none() {
        assert!(OdooLintConfig::from_pyproject_toml("[project]\nname = 'x'\n")
            .unwrap()
            .is_none());
    }

    #[test]
    fn standalone_toml_and_unknown_keys() {
        let c = OdooLintConfig::from_odoo_lint_toml("[rules.manifest-author]\ndefault = \"Acme\"\n").unwrap();
        assert_eq!(c.get_expected_author("anything"), "Acme");
        // snake_case typo is rejected instead of silently ignored
        assert!(OdooLintConfig::from_odoo_lint_toml("[rules.manifest_author]\ndefault = \"Acme\"\n").is_err());
    }
}
