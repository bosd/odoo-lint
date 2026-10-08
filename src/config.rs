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

/// pylint-odoo's default for `manifest-required-authors`.
pub const DEFAULT_AUTHOR: &str = "Odoo Community Association (OCA)";

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct OdooLintConfig {
    pub target_version: Option<String>,
    /// Rules to enable: codes, names or code prefixes; default `["ALL"]`.
    pub select: Option<Vec<String>>,
    /// Rules to disable, same syntax as `select`.
    pub ignore: Option<Vec<String>>,
    /// Extra glob patterns (relative to the project root) to skip.
    pub exclude: Option<Vec<String>>,
    /// Glob pattern -> rules to ignore in matching files.
    pub per_file_ignores: Option<HashMap<String, Vec<String>>>,
    pub rules: Option<RulesConfig>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RulesConfig {
    /// Settings for C8101; `manifest-author` is the pre-0.1 name.
    #[serde(alias = "manifest-author")]
    pub manifest_required_author: Option<ManifestAuthorConfig>,
}

/// A single string or a list of strings.
#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            OneOrMany::One(s) => vec![s.clone()],
            OneOrMany::Many(v) => v.clone(),
        }
    }
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ManifestAuthorConfig {
    /// Authors of which at least one must be present (pylint-odoo's
    /// `manifest-required-authors`). `default` is the pre-0.1 name.
    #[serde(alias = "default")]
    pub authors: Option<OneOrMany>,
    /// Module name (exact) or prefix pattern ending in `*` -> required author(s).
    pub mapping: Option<HashMap<String, OneOrMany>>,
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

    /// Authors of which one must be in the manifest of `module_name` (C8101).
    ///
    /// An exact module name wins; otherwise the longest matching `prefix*`
    /// pattern wins, so the result does not depend on table order; otherwise
    /// `authors`, which defaults to the OCA.
    pub fn required_authors(&self, module_name: &str) -> Vec<String> {
        let Some(author_cfg) = self.rules.as_ref().and_then(|r| r.manifest_required_author.as_ref()) else {
            return vec![DEFAULT_AUTHOR.to_string()];
        };
        if let Some(mapping) = &author_cfg.mapping {
            if let Some(expected) = mapping.get(module_name).filter(|_| !module_name.ends_with('*')) {
                return expected.to_vec();
            }
            let best = mapping
                .iter()
                .filter_map(|(pattern, expected)| {
                    let prefix = pattern.strip_suffix('*')?;
                    module_name.starts_with(prefix).then_some((prefix.len(), expected))
                })
                .max_by_key(|(len, _)| *len);
            if let Some((_, expected)) = best {
                return expected.to_vec();
            }
        }
        author_cfg
            .authors
            .as_ref()
            .map(OneOrMany::to_vec)
            .unwrap_or_else(|| vec![DEFAULT_AUTHOR.to_string()])
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
select = ["ALL"]
ignore = ["W8116"]
exclude = ["setup/*"]

[tool.odoo-lint.per-file-ignores]
"*/tests/*" = ["print-used"]

[tool.odoo-lint.rules.manifest-required-author]
authors = "Odoo Community Association (OCA)"

[tool.odoo-lint.rules.manifest-required-author.mapping]
"mijnbedrijf_*" = "MijnBedrijf B.V."
"acme_*" = "Acme Corp"
"acme_hr_*" = ["Acme HR", "Acme Corp"]
"acme_special" = "Someone Else"
"#;

    fn config() -> OdooLintConfig {
        OdooLintConfig::from_pyproject_toml(PYPROJECT).unwrap().unwrap()
    }

    #[test]
    fn parses_pyproject_section() {
        let c = config();
        assert_eq!(c.target_version.as_deref(), Some("16.0"));
        assert_eq!(c.ignore, Some(vec!["W8116".to_string()]));
        assert_eq!(c.per_file_ignores.unwrap()["*/tests/*"], vec!["print-used"]);
    }

    #[test]
    fn author_resolution() {
        let c = config();
        assert_eq!(
            c.required_authors("mijnbedrijf_maatwerkmodule"),
            vec!["MijnBedrijf B.V."]
        );
        assert_eq!(c.required_authors("acme_sale"), vec!["Acme Corp"]);
        assert_eq!(c.required_authors("acme_hr_payroll"), vec!["Acme HR", "Acme Corp"]);
        assert_eq!(c.required_authors("acme_special"), vec!["Someone Else"]);
        assert_eq!(c.required_authors("sale_stock"), vec![DEFAULT_AUTHOR]);
    }

    #[test]
    fn default_without_config() {
        assert_eq!(OdooLintConfig::default().required_authors("x"), vec![DEFAULT_AUTHOR]);
    }

    #[test]
    fn legacy_manifest_author_keys() {
        let c = OdooLintConfig::from_odoo_lint_toml("[rules.manifest-author]\ndefault = \"Acme\"\n").unwrap();
        assert_eq!(c.required_authors("anything"), vec!["Acme"]);
    }

    #[test]
    fn pyproject_without_section_is_none() {
        assert!(OdooLintConfig::from_pyproject_toml("[project]\nname = 'x'\n")
            .unwrap()
            .is_none());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        // snake_case typo is rejected instead of silently ignored
        assert!(OdooLintConfig::from_odoo_lint_toml("[rules.manifest_author]\ndefault = \"Acme\"\n").is_err());
        assert!(OdooLintConfig::from_odoo_lint_toml("selekt = [\"ALL\"]\n").is_err());
    }
}
