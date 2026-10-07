use std::fmt;

#[derive(Debug, Clone)]
pub struct Violation {
    pub file_path: String,
    pub line: usize,
    pub rule_code: &'static str,
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: [{}] {}",
            self.file_path, self.line, self.rule_code, self.message
        )
    }
}
