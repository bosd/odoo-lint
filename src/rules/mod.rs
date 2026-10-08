//! Rule registry.
//!
//! Rules ported from pylint-odoo keep its message id as `code` and its
//! symbolic name as `name` (e.g. `C8101` / `manifest-required-author`), so
//! existing `# pylint: disable=` comments and configs keep working. Rules of
//! odoo-lint's own use `ODOO###` codes.

use crate::checker::{ManifestContext, PythonContext, Reporter};
use crate::odoo_version::OdooVersion;

pub mod c8101_manifest_required_author;
pub mod e0001_syntax_error;
pub mod odoo001_missing_depends;

/// How a rule is run.
#[derive(Debug, Clone, Copy)]
pub enum Check {
    /// Runs on the syntax tree of every Python file.
    Python(fn(&PythonContext, &mut Reporter)),
    /// Runs once per module, on its parsed manifest.
    Manifest(fn(&ManifestContext, &mut Reporter)),
    /// Emitted by the linter itself (e.g. syntax errors).
    Builtin,
}

/// Metadata, documentation and implementation of a rule.
///
/// `doc` is Markdown; it is shown by `odl rule <CODE>` and rendered into
/// `docs/rules/<CODE>.md` (see `tests/generated_docs.rs`).
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub code: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub doc: &'static str,
    pub check: Check,
    /// First Odoo version the rule applies to.
    pub min_odoo: Option<OdooVersion>,
    /// Last Odoo version the rule applies to.
    pub max_odoo: Option<OdooVersion>,
}

/// All rules, ordered by code.
pub const ALL: &[Rule] = &[
    c8101_manifest_required_author::RULE,
    e0001_syntax_error::RULE,
    odoo001_missing_depends::RULE,
];

pub fn find(code_or_name: &str) -> Option<&'static Rule> {
    ALL.iter()
        .find(|rule| rule.code.eq_ignore_ascii_case(code_or_name) || rule.name.eq_ignore_ascii_case(code_or_name))
}

impl Rule {
    pub fn applies_to(&self, version: OdooVersion) -> bool {
        self.min_odoo.is_none_or(|min| version >= min) && self.max_odoo.is_none_or(|max| version <= max)
    }

    /// Human-readable Odoo version range, if the rule is version-specific.
    pub fn version_range(&self) -> Option<String> {
        match (self.min_odoo, self.max_odoo) {
            (None, None) => None,
            (Some(min), None) => Some(format!("Odoo {min} and later")),
            (None, Some(max)) => Some(format!("Odoo {max} and earlier")),
            (Some(min), Some(max)) => Some(format!("Odoo {min} to {max}")),
        }
    }

    /// Full Markdown page for this rule.
    pub fn to_markdown(&self) -> String {
        let mut page = format!("# {} ({})\n\n{}\n", self.name, self.code, self.summary);
        if let Some(range) = self.version_range() {
            page.push_str(&format!("\nApplies to {range}.\n"));
        }
        page.push_str(&format!("\n{}\n", self.doc.trim()));
        page
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_sorted_and_unique() {
        assert!(ALL.windows(2).all(|w| w[0].code < w[1].code));
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<_> = ALL.iter().map(|r| r.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ALL.len());
    }

    #[test]
    fn find_by_code_or_name() {
        assert_eq!(find("odoo001").map(|r| r.code), Some("ODOO001"));
        assert_eq!(find("manifest-required-author").map(|r| r.code), Some("C8101"));
        assert!(find("ODOO999").is_none());
    }

    #[test]
    fn version_gating() {
        let rule = Rule {
            min_odoo: Some(OdooVersion::new(17, 0)),
            ..odoo001_missing_depends::RULE
        };
        assert!(!rule.applies_to(OdooVersion::new(16, 0)));
        assert!(rule.applies_to(OdooVersion::new(17, 0)));
        assert_eq!(rule.version_range().as_deref(), Some("Odoo 17.0 and later"));
    }
}
