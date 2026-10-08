//! E0001: file cannot be parsed (pylint's `syntax-error`).

use crate::rules::{Check, Rule};

pub const RULE: Rule = Rule {
    code: "E0001",
    name: "syntax-error",
    summary: "A Python file cannot be parsed.",
    doc: r#"
## What it does

Reports Python files, including manifests, that are not valid Python. No other
rule can check such a file, so its other problems stay hidden until the syntax
error is fixed.

The code and name are pylint's, so `# pylint: disable=syntax-error` and
`ignore = ["E0001"]` work as you would expect.
"#,
    check: Check::Builtin,
    min_odoo: None,
    max_odoo: None,
};
