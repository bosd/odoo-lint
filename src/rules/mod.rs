pub mod odoo001_missing_depends;
pub mod odoo010_manifest_author;

/// Metadata and user-facing documentation of a rule.
///
/// `doc` is Markdown; it is shown by `odl rule <CODE>` and rendered into
/// `docs/rules/<CODE>.md` (see `tests/generated_docs.rs`).
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub code: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub doc: &'static str,
}

/// All rules, ordered by code.
pub const ALL: &[Rule] = &[odoo001_missing_depends::RULE, odoo010_manifest_author::RULE];

pub fn find(code: &str) -> Option<&'static Rule> {
    ALL.iter().find(|rule| rule.code.eq_ignore_ascii_case(code))
}

impl Rule {
    /// Full Markdown page for this rule.
    pub fn to_markdown(&self) -> String {
        format!(
            "# {} ({})\n\n{}\n\n{}\n",
            self.name,
            self.code,
            self.summary,
            self.doc.trim()
        )
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
    fn find_is_case_insensitive() {
        assert_eq!(find("odoo001").map(|r| r.code), Some("ODOO001"));
        assert!(find("ODOO999").is_none());
    }
}
