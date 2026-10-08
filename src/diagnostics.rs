use serde::Serialize;
use std::fmt;

/// A single rule violation. `line` and `column` are 1-based.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Violation {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    /// Rule code, e.g. `C8101` or `ODOO001`.
    pub code: String,
    /// Symbolic rule name, e.g. `manifest-required-author`.
    pub name: String,
    pub message: String,
}

impl Violation {
    /// Severity derived from the pylint message-id prefix: `E` and `F` are
    /// errors, everything else (including `ODOO###`) is a warning.
    pub fn is_error(&self) -> bool {
        matches!(self.code.as_bytes().first(), Some(b'E' | b'F')) && self.code[1..].bytes().all(|b| b.is_ascii_digit())
    }

    fn sort_key(&self) -> (&str, usize, usize, &str) {
        (&self.file_path, self.line, self.column, &self.code)
    }
}

impl PartialOrd for Violation {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Violation {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.sort_key()
            .cmp(&other.sort_key())
            .then_with(|| self.message.cmp(&other.message))
    }
}

/// pylint's default message template, so existing tooling and muscle memory
/// keep working: `path:line:column: CODE: message (name)` with a 0-based
/// column, as pylint prints it.
impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}: {} ({})",
            self.file_path,
            self.line,
            self.column.saturating_sub(1),
            self.code,
            self.message,
            self.name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(code: &str) -> Violation {
        Violation {
            file_path: "a.py".into(),
            line: 3,
            column: 5,
            code: code.into(),
            name: "x".into(),
            message: "m".into(),
        }
    }

    #[test]
    fn pylint_format() {
        assert_eq!(v("C8101").to_string(), "a.py:3:4: C8101: m (x)");
    }

    #[test]
    fn severity() {
        assert!(v("E8103").is_error());
        assert!(v("F8101").is_error());
        assert!(!v("W8116").is_error());
        assert!(!v("ODOO001").is_error());
        assert!(!v("EXAMPLE1").is_error());
    }
}
