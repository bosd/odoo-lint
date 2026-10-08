//! Rule registry.
//!
//! Rules ported from pylint-odoo keep its message id as `code` and its
//! symbolic name as `name` (e.g. `C8101` / `manifest-required-author`), so
//! existing `# pylint: disable=` comments and configs keep working. Rules of
//! odoo-lint's own use `ODOO###` codes.

use crate::checker::{ManifestContext, PythonContext, Reporter};
use crate::odoo_version::OdooVersion;

pub mod e0001_syntax_error;
pub mod manifest;
pub mod odoo001_missing_depends;
pub mod python;

use manifest::{author, files, keys, values};
use python::{calls, fields, imports, methods, misc, models};

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
    author::MANIFEST_REQUIRED_AUTHOR,
    keys::MANIFEST_REQUIRED_KEY,
    keys::MANIFEST_DEPRECATED_KEY,
    values::LICENSE_ALLOWED,
    values::MANIFEST_VERSION_FORMAT,
    fields::METHOD_COMPUTE,
    fields::METHOD_SEARCH,
    fields::METHOD_INVERSE,
    values::DEVELOPMENT_STATUS_ALLOWED,
    files::MISSING_README,
    models::NO_WIZARD_IN_MODELS,
    values::CATEGORY_ALLOWED,
    files::MISSING_ODOO_FILE,
    keys::MANIFEST_SUPERFLUOUS_KEY,
    values::CATEGORY_ALLOWED_APP,
    files::MISSING_ODOO_FILE_APP,
    keys::MANIFEST_REQUIRED_KEY_APP,
    values::MANIFEST_SUMMARY_MULTILINE,
    e0001_syntax_error::RULE,
    author::MANIFEST_AUTHOR_STRING,
    calls::INVALID_COMMIT,
    values::MANIFEST_MAINTAINERS_LIST,
    calls::EXTERNAL_REQUEST_TIMEOUT,
    imports::TEST_FOLDER_IMPORTED,
    models::NO_WRITE_IN_COMPUTE,
    methods::NO_RAISE_UNLINK,
    files::MANIFEST_BEHIND_MIGRATIONS,
    methods::DEPRECATED_NAME_GET,
    fields::INHERITABLE_METHOD_STRING,
    fields::INHERITABLE_METHOD_LAMBDA,
    misc::DEPRECATED_INSELECT_OPERATOR,
    files::RESOURCE_NOT_EXIST,
    odoo001_missing_depends::RULE,
    imports::ODOO_EXCEPTION_WARNING,
    values::INVALID_EMAIL,
    fields::TRANSLATION_FIELD,
    fields::ATTRIBUTE_DEPRECATED,
    methods::METHOD_REQUIRED_SUPER,
    methods::PROHIBITED_METHOD_OVERRIDE,
    methods::MISSING_RETURN,
    fields::RENAMED_FIELD_PARAMETER,
    fields::ATTRIBUTE_STRING_REDUNDANT,
    values::WEBSITE_MANIFEST_KEY_NOT_VALID_URI,
    calls::PRINT_USED,
    calls::CONTEXT_OVERRIDDEN,
    files::MANIFEST_DATA_DUPLICATED,
    misc::EXCEPT_PASS,
    imports::ODOO_ADDONS_RELATIVE_IMPORT,
    calls::BAD_BUILTIN_GROUPBY,
    methods::DEPRECATED_ODOO_MODEL_METHOD,
    values::MANIFEST_EXTERNAL_ASSETS,
    calls::NO_SEARCH_ALL,
    methods::SUPER_METHOD_MISMATCH,
    models::DEPRECATED_SELF_CR,
    misc::USE_VIM_COMMENT,
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
