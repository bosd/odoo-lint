//! Rule registry.
//!
//! Rules ported from pylint-odoo keep its message id as `code` and its
//! symbolic name as `name` (e.g. `C8101` / `manifest-required-author`), so
//! existing `# pylint: disable=` comments and configs keep working. Rules of
//! odoo-lint's own use `ODOO###` codes.

use crate::checker::{ManifestContext, PythonContext, Reporter};
use crate::odoo_version::OdooVersion;
use po::PoContext;

pub mod e0001_syntax_error;
pub mod manifest;
pub mod module;
pub mod odoo001_missing_depends;
pub mod po;
pub mod po_fixes;
pub mod po_odoo;
pub mod python;
pub mod python_fields;
pub mod references;
pub mod upgrade;
pub mod views;
pub mod xml;

use manifest::{author, files, keys, values};
use python::{calls, fields, imports, inherit, methods, misc, models, sql, translations};

/// How a rule is run.
#[derive(Debug, Clone, Copy)]
pub enum Check {
    /// Runs on the syntax tree of every Python file.
    Python(fn(&PythonContext, &mut Reporter)),
    /// Runs once per module, on its parsed manifest.
    Manifest(fn(&ManifestContext, &mut Reporter)),
    /// Runs on every `.po` and `.pot` file.
    Po(fn(&PoContext, &mut Reporter)),
    /// Runs once per module, on the XML files its manifest loads. Gated on
    /// the module's own Odoo version.
    Xml(fn(&xml::XmlContext, &mut xml::XmlReporter)),
    /// Runs once per module, on its folder and manifest.
    Module(fn(&module::ModuleContext, &mut module::ModuleReporter)),
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
    translations::TRANSLATION_REQUIRED,
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
    sql::SQL_INJECTION,
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
    translations::TRANSLATION_INJECTION,
    translations::TRANSLATION_UNSUPPORTED_FORMAT,
    translations::TRANSLATION_FORMAT_TRUNCATED,
    translations::TRANSLATION_TOO_MANY_ARGS,
    translations::TRANSLATION_TOO_FEW_ARGS,
    files::RESOURCE_NOT_EXIST,
    module::MANIFEST_SYNTAX_ERROR,
    module::FILE_NOT_USED,
    module::CSV_SYNTAX_ERROR,
    module::CSV_DUPLICATE_RECORD_ID,
    module::PREFER_README_RST,
    module::WEBLATE_COMPONENT_TOO_LONG,
    module::UNUSED_LOGGER,
    module::USE_HEADER_COMMENTS,
    module::FIELD_STRING_REDUNDANT,
    odoo001_missing_depends::RULE,
    module::UNWANTED_FILE,
    module::LARGE_FILE,
    views::VIEW_FIELD_NOT_FOUND,
    references::XML_ID_NOT_FOUND,
    references::MODEL_NOT_FOUND,
    python_fields::PYTHON_FIELD_NOT_FOUND,
    po::PO_SYNTAX_ERROR,
    po::PO_REQUIRES_MODULE,
    po::PO_PYTHON_PARSE_PRINTF,
    po::PO_PYTHON_PARSE_FORMAT,
    po::PO_DUPLICATE_MESSAGE_DEFINITION,
    po::PO_DUPLICATE_MODEL_DEFINITION,
    po::PO_PRETTY_FORMAT,
    po_odoo::PO_NOT_IN_POT,
    po_odoo::PO_UNKNOWN_OCCURRENCE,
    po_odoo::PO_FILE_NAME,
    po_odoo::PO_FUZZY,
    imports::ODOO_EXCEPTION_WARNING,
    inherit::CONSIDER_MERGING_CLASSES_INHERITED,
    values::INVALID_EMAIL,
    upgrade::v16::XML_EXTENSION_GROUPS,
    upgrade::v16::XML_HTML_FIELD_TYPE,
    upgrade::v16::ASSETS_QWEB,
    upgrade::v16::REMOVED_BUNDLES_RULE,
    upgrade::v16::MANIFEST_QWEB,
    upgrade::v16::IR_TRANSLATION,
    upgrade::v16::REQUEST_API,
    upgrade::v16::BINARY_CONTENT,
    upgrade::v16::SEARCH_ARGS,
    upgrade::v16::OSV_QUERY,
    upgrade::v16::FIELDS_VIEW_GET,
    upgrade::v17::XML_ATTRS,
    upgrade::v17::XML_LIST_INVISIBLE,
    upgrade::v17::XML_SHORTCUT_TAGS,
    upgrade::v17::XML_QUICK_ADD,
    upgrade::v17::XML_ACTIVE_ID,
    upgrade::v17::XML_SERVER_ACTION_LINES,
    upgrade::v17::XML_FIELD_PARENT,
    upgrade::v17::XML_VIEW_DIRECTIVES,
    upgrade::v17::NAME_GET,
    upgrade::v17::NAME_SEARCH_SIGNATURE,
    upgrade::v17::SEARCH_COUNT,
    upgrade::v17::REMOVED_RECORDSET_METHODS,
    upgrade::v17::FIELD_STATES,
    upgrade::v17::SAVEPOINT_CASE,
    upgrade::v17::OLD_EXCEPTIONS,
    upgrade::v17::ONCHANGE_DOMAIN,
    upgrade::v17::NORECOMPUTE,
    upgrade::v17::IR_DEFAULT_GET,
    upgrade::v17::READ_GROUP_SIGNATURE,
    upgrade::v17::OPENERP_MANIFEST,
    upgrade::v17::REMOVED_BUNDLES_RULE,
    upgrade::v18::XML_TREE_VIEW,
    upgrade::v18::XML_VIEW_MODE_TREE,
    upgrade::v18::XML_TREE_REFERENCE,
    upgrade::v18::XML_CRON_NUMBERCALL,
    upgrade::v18::XML_DEFAULT_PERIOD,
    upgrade::v18::XML_KANBAN_BOX,
    upgrade::v18::GROUP_OPERATOR,
    upgrade::v18::USER_HAS_GROUPS,
    upgrade::v18::PYTHON_TREE_VIEW,
    upgrade::v18::NAME_SEARCH_OVERRIDE,
    upgrade::v19::ENV_SHORTCUTS,
    upgrade::v19::SQL_CONSTRAINTS,
    upgrade::v19::API_MODEL_CREATE,
    upgrade::v19::API_RETURNS,
    upgrade::v19::READ_GROUP_OVERRIDE,
    upgrade::v19::ROUTE_JSON,
    upgrade::v19::PYTHON_GROUPS_ID,
    upgrade::v19::OSV_EXPRESSION,
    upgrade::v19::AUTO_JOIN,
    upgrade::v19::NAME_SEARCH_ARGS,
    upgrade::v19::CLEAR_CACHES,
    upgrade::v19::REMOVED_HELPERS,
    upgrade::v19::SEQUENCE_GET,
    upgrade::v19::DOMAIN_OPERATORS,
    upgrade::v19::MODELS_NEWID,
    upgrade::v19::XML_GROUPS_ID,
    upgrade::v19::XML_SEARCH_GROUP,
    upgrade::v19::XML_GROUPS_USERS,
    upgrade::v19::MANIFEST_OLD_DATA_KEYS,
    upgrade::v19::XML_T_CALL_ELEMENT,
    upgrade::v19::XML_PARTNER_MOBILE,
    upgrade::v20::MANIFEST_ACCESS_CSV,
    upgrade::v20::XML_ACCESS_RECORDS,
    upgrade::v20::XML_T_ESC,
    upgrade::v20::XML_T_CALL_BODY,
    upgrade::v20::XML_T_CALL_OPTIONS,
    upgrade::v20::XML_BASE64_FILE,
    upgrade::v20::XML_ATTACHMENT_DATAS,
    upgrade::v20::XML_REPORT_FILE,
    upgrade::v20::XML_FONT_AWESOME,
    upgrade::v20::XML_FILTER_DATE_ATTRIBUTES,
    upgrade::v20::XML_CALENDAR_DATE_DELAY,
    upgrade::v20::XML_BANK_FIELDS,
    upgrade::v20::XML_PARTNER_FIELDS,
    upgrade::v20::XML_WIDGET_RENAMES,
    upgrade::v20::MANIFEST_JQUERY,
    upgrade::v20::MANIFEST_INIT_XML,
    upgrade::v20::CONFIG_PARAMETER,
    upgrade::v20::ATTACHMENT_DATAS,
    upgrade::v20::ACCESS_MODELS,
    upgrade::v20::REMOVED_ACCESS_METHODS,
    upgrade::v20::READ_GROUP_SIGNATURE,
    upgrade::v20::ORMCACHE_INVALIDATION,
    upgrade::v20::ORMCACHE_IMPORT,
    upgrade::v20::HTTP_IMPORTS,
    upgrade::v20::TOOLS_IMPORTS,
    upgrade::v20::BANK_FIELDS,
    upgrade::v20::PARTNER_FIELDS,
    upgrade::v20::SELF_FIELDS,
    upgrade::v20::INHERIT_READ,
    upgrade::v20::TEST_CLASSES,
    fields::TRANSLATION_FIELD,
    fields::ATTRIBUTE_DEPRECATED,
    methods::METHOD_REQUIRED_SUPER,
    methods::PROHIBITED_METHOD_OVERRIDE,
    methods::MISSING_RETURN,
    fields::RENAMED_FIELD_PARAMETER,
    fields::ATTRIBUTE_STRING_REDUNDANT,
    values::WEBSITE_MANIFEST_KEY_NOT_VALID_URI,
    translations::TRANSLATION_CONTAINS_VARIABLE,
    calls::PRINT_USED,
    translations::TRANSLATION_POSITIONAL_USED,
    calls::CONTEXT_OVERRIDDEN,
    files::MANIFEST_DATA_DUPLICATED,
    misc::EXCEPT_PASS,
    imports::ODOO_ADDONS_RELATIVE_IMPORT,
    calls::BAD_BUILTIN_GROUPBY,
    methods::DEPRECATED_ODOO_MODEL_METHOD,
    translations::PREFER_ENV_TRANSLATION,
    values::MANIFEST_EXTERNAL_ASSETS,
    calls::NO_SEARCH_ALL,
    methods::SUPER_METHOD_MISMATCH,
    models::DEPRECATED_SELF_CR,
    misc::USE_VIM_COMMENT,
    translations::TRANSLATION_NOT_LAZY,
    translations::TRANSLATION_FORMAT_INTERPOLATION,
    translations::TRANSLATION_FSTRING_INTERPOLATION,
    xml::XML_SYNTAX_ERROR,
    xml::XML_HEADER_MISSING,
    xml::XML_HEADER_WRONG,
    xml::XML_RECORD_MISSING_ID,
    xml::XML_DUPLICATE_RECORD_ID,
    xml::XML_DUPLICATE_FIELDS,
    xml::XML_DUPLICATE_TEMPLATE_ID,
    xml::XML_REDUNDANT_MODULE_NAME,
    xml::XML_TAG_POSITION,
    xml::XML_DEPRECATED_DATA_NODE,
    xml::XML_DEPRECATED_OPENERP_NODE,
    xml::XML_DEPRECATED_QWEB_DIRECTIVE,
    xml::XML_DEPRECATED_QWEB_DIRECTIVE_15,
    xml::XML_DEPRECATED_TREE_ATTRIBUTE,
    xml::XML_DEPRECATED_OE_CHATTER,
    xml::XML_DEPRECATED_RES_GROUPS_CATEGORY_ID,
    xml::XML_VIEW_DANGEROUS_REPLACE_LOW_PRIORITY,
    xml::XML_DANGEROUS_QWEB_REPLACE_LOW_PRIORITY,
    xml::XML_CREATE_USER_WO_RESET_PASSWORD,
    xml::XML_NOT_VALID_CHAR_LINK,
    xml::XML_XPATH_TRANSLATABLE_ITEM,
    xml::XML_OE_STRUCTURE_MISSING_ID,
    xml::XML_FIELD_BOOL_WITHOUT_EVAL,
    xml::XML_FIELD_NUMERIC_WITHOUT_EVAL,
    xml::XML_BOOTSTRAP4_CLASS,
    xml::XML_BOOTSTRAP4_REMOVED_CLASS,
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

    /// The rule as `odl rule --output-format json` shows it.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "code": self.code,
            "name": self.name,
            "summary": self.summary,
            "min_odoo_version": self.min_odoo.map(|v| v.to_string()),
            "max_odoo_version": self.max_odoo.map(|v| v.to_string()),
            "doc": self.doc.trim(),
        })
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
