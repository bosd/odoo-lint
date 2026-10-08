//! Effective settings: the configuration file merged with CLI options.

use crate::config::OdooLintConfig;
use crate::odoo_version::{OdooVersion, DEFAULT_ODOO_VERSION};
use crate::rules::{Rule, ALL};
use globset::{Glob, GlobMatcher};
use std::path::{Path, PathBuf};

/// Directory names never descended into.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    ".bzr",
    ".direnv",
    ".eggs",
    ".git",
    ".hg",
    ".mypy_cache",
    ".nox",
    ".pytest_cache",
    ".ruff_cache",
    ".svn",
    ".tox",
    ".venv",
    "__pycache__",
    "build",
    "dist",
    "node_modules",
    "site-packages",
    "target",
    "venv",
];

/// Options given on the command line; they take precedence over the config.
#[derive(Debug, Default)]
pub struct CliOverrides {
    pub target_version: Option<String>,
    /// Replaces `select` from the config.
    pub select: Option<Vec<String>>,
    /// Added to `ignore` from the config.
    pub ignore: Vec<String>,
}

/// A path pattern from `exclude` or `per-file-ignores`.
#[derive(Debug, Clone)]
struct PathPattern {
    matcher: GlobMatcher,
    /// Patterns without `/` also match the bare file or directory name.
    match_name: bool,
}

impl PathPattern {
    fn new(pattern: &str) -> Result<Self, String> {
        let glob = Glob::new(pattern).map_err(|e| format!("invalid glob '{pattern}': {e}"))?;
        Ok(Self {
            matcher: glob.compile_matcher(),
            match_name: !pattern.contains('/'),
        })
    }

    fn matches(&self, relative: &Path) -> bool {
        self.matcher.is_match(relative)
            || (self.match_name && relative.file_name().is_some_and(|n| self.matcher.is_match(n)))
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub config: OdooLintConfig,
    pub target_version: OdooVersion,
    pub select: Vec<String>,
    pub ignore: Vec<String>,
    /// Paths in `exclude` and `per-file-ignores` are relative to this.
    pub project_root: PathBuf,
    exclude: Vec<PathPattern>,
    per_file_ignores: Vec<(PathPattern, Vec<String>)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            config: OdooLintConfig::default(),
            target_version: DEFAULT_ODOO_VERSION,
            select: vec!["ALL".to_string()],
            ignore: Vec::new(),
            project_root: PathBuf::from("."),
            exclude: Vec::new(),
            per_file_ignores: Vec::new(),
        }
    }
}

/// Whether a `select`/`ignore` entry matches `rule`: `ALL`, an exact code or
/// name, or a code prefix such as `C81` or `ODOO`.
pub fn selector_matches(selector: &str, rule: &Rule) -> bool {
    selector.eq_ignore_ascii_case("all")
        || selector.eq_ignore_ascii_case(rule.code)
        || selector.eq_ignore_ascii_case(rule.name)
        || (!selector.is_empty()
            && rule.code.len() > selector.len()
            && rule.code[..selector.len()].eq_ignore_ascii_case(selector))
}

/// Settings as `odl check` builds them: the config file given, or the one
/// found from `start`, with the CLI options on top.
pub struct Loaded {
    pub settings: Settings,
    pub config_path: Option<PathBuf>,
    /// Selectors that match no rule.
    pub warnings: Vec<String>,
}

impl Settings {
    pub fn load(start: &Path, config: Option<&Path>, cli: CliOverrides) -> Result<Loaded, String> {
        let (config, config_path) = match config {
            Some(file) => OdooLintConfig::from_file(file).map(|c| (c, Some(file.to_path_buf()))),
            None => OdooLintConfig::discover(start),
        }
        .map_err(|e| e.to_string())?;
        let (settings, warnings) = Self::new(config, config_path.as_deref(), cli)?;
        Ok(Loaded {
            settings,
            config_path,
            warnings,
        })
    }

    /// Builds settings from a config (found at `config_path`, if any) and CLI
    /// options. Returns warnings for selectors that match no rule; those are
    /// usually pylint-odoo checks odoo-lint does not implement yet.
    pub fn new(
        config: OdooLintConfig,
        config_path: Option<&Path>,
        cli: CliOverrides,
    ) -> Result<(Self, Vec<String>), String> {
        let version_str = cli.target_version.clone().or_else(|| config.target_version.clone());
        let target_version = match version_str {
            Some(v) => v.parse()?,
            None => DEFAULT_ODOO_VERSION,
        };
        let select = cli
            .select
            .or_else(|| config.select.clone())
            .unwrap_or_else(|| vec!["ALL".to_string()]);
        let mut ignore = config.ignore.clone().unwrap_or_default();
        ignore.extend(cli.ignore);

        let exclude = config
            .exclude
            .iter()
            .flatten()
            .map(|p| PathPattern::new(p))
            .collect::<Result<Vec<_>, _>>()?;
        let mut per_file_ignores = Vec::new();
        for (pattern, rules) in config.per_file_ignores.iter().flatten() {
            per_file_ignores.push((PathPattern::new(pattern)?, rules.clone()));
        }

        let mut warnings = Vec::new();
        let per_file_selectors = per_file_ignores.iter().flat_map(|(_, rules)| rules.iter());
        for selector in select.iter().chain(&ignore).chain(per_file_selectors) {
            if !ALL.iter().any(|rule| selector_matches(selector, rule)) {
                warnings.push(format!("'{selector}' does not match any rule odoo-lint implements"));
            }
        }

        let project_root = config_path
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let settings = Self {
            config,
            target_version,
            select,
            ignore,
            project_root,
            exclude,
            per_file_ignores,
        };
        Ok((settings, warnings))
    }

    /// Rules that are selected, not ignored and apply to the target version.
    pub fn enabled_rules(&self) -> Vec<&'static Rule> {
        ALL.iter()
            .filter(|rule| self.select.iter().any(|s| selector_matches(s, rule)))
            .filter(|rule| !self.ignore.iter().any(|s| selector_matches(s, rule)))
            .filter(|rule| rule.applies_to(self.target_version))
            .collect()
    }

    fn relative<'a>(&self, path: &'a Path) -> &'a Path {
        path.strip_prefix(&self.project_root)
            .or_else(|_| path.strip_prefix("./"))
            .unwrap_or(path)
    }

    /// Whether `path` (a file or directory) is excluded by `exclude`.
    pub fn is_excluded(&self, path: &Path) -> bool {
        let relative = self.relative(path);
        self.exclude.iter().any(|p| p.matches(relative))
    }

    /// Whether `per-file-ignores` turns `rule` off for `path`.
    pub fn is_ignored_in_file(&self, path: &Path, rule: &Rule) -> bool {
        let relative = self.relative(path);
        self.per_file_ignores
            .iter()
            .any(|(pattern, rules)| pattern.matches(relative) && rules.iter().any(|s| selector_matches(s, rule)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::find;

    fn rule(code: &str) -> &'static Rule {
        find(code).unwrap()
    }

    #[test]
    fn selectors() {
        let c8101 = rule("C8101");
        assert!(selector_matches("ALL", c8101));
        assert!(selector_matches("c8101", c8101));
        assert!(selector_matches("manifest-required-author", c8101));
        assert!(selector_matches("C81", c8101));
        assert!(selector_matches("C", c8101));
        assert!(!selector_matches("C82", c8101));
        assert!(!selector_matches("ODOO", c8101));
        assert!(selector_matches("ODOO", rule("ODOO001")));
    }

    #[test]
    fn select_and_ignore() {
        let config = OdooLintConfig {
            select: Some(vec!["C".into(), "ODOO".into()]),
            ignore: Some(vec!["missing-depends".into()]),
            ..Default::default()
        };
        let (settings, warnings) = Settings::new(config, None, CliOverrides::default()).unwrap();
        let codes: Vec<_> = settings.enabled_rules().iter().map(|r| r.code).collect();
        assert!(codes.contains(&"C8101"));
        assert!(codes.iter().all(|c| c.starts_with('C')), "{codes:?}");
        // ODOO001 is ignored by name; C8120 only applies from Odoo 20.0.
        assert!(!codes.contains(&"ODOO001"));
        assert!(!codes.contains(&"C8120"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn cli_overrides_config() {
        let config = OdooLintConfig {
            target_version: Some("16.0".into()),
            select: Some(vec!["C".into()]),
            ..Default::default()
        };
        let cli = CliOverrides {
            target_version: Some("18.0".into()),
            select: Some(vec!["ALL".into()]),
            ignore: vec!["E0001".into()],
        };
        let (settings, _) = Settings::new(config, None, cli).unwrap();
        assert_eq!(settings.target_version, OdooVersion::new(18, 0));
        assert!(settings.enabled_rules().iter().any(|r| r.code == "ODOO001"));
        assert!(!settings.enabled_rules().iter().any(|r| r.code == "E0001"));
    }

    #[test]
    fn unknown_selectors_warn() {
        let config = OdooLintConfig {
            ignore: Some(vec!["C0103".into()]),
            ..Default::default()
        };
        let (_, warnings) = Settings::new(config, None, CliOverrides::default()).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("C0103"));
    }

    #[test]
    fn invalid_version_is_an_error() {
        let cli = CliOverrides {
            target_version: Some("latest".into()),
            ..Default::default()
        };
        assert!(Settings::new(OdooLintConfig::default(), None, cli).is_err());
    }

    #[test]
    fn per_file_ignores_and_exclude() {
        let config = OdooLintConfig::from_odoo_lint_toml(
            r#"
exclude = ["setup", "addons/legacy_*"]
[per-file-ignores]
"*/tests/*" = ["ODOO001"]
"#,
        )
        .unwrap();
        let (settings, _) =
            Settings::new(config, Some(Path::new("/repo/odoo-lint.toml")), CliOverrides::default()).unwrap();
        let odoo001 = rule("ODOO001");
        assert!(settings.is_ignored_in_file(Path::new("/repo/addons/x/tests/test_a.py"), odoo001));
        assert!(!settings.is_ignored_in_file(Path::new("/repo/addons/x/models/a.py"), odoo001));
        assert!(settings.is_excluded(Path::new("/repo/setup")));
        assert!(settings.is_excluded(Path::new("/repo/addons/legacy_sale")));
        assert!(!settings.is_excluded(Path::new("/repo/addons/sale")));
    }
}
