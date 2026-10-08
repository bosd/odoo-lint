//! R8180 consider-merging-classes-inherited: several classes of one module
//! extend the same model. Needs all files of the module, so the linter
//! collects [`InheritFact`]s per file and reports after the parallel pass.

use super::classes;
use crate::checker::PythonContext;
use crate::rules::{Check, Rule};
use ruff_python_ast::{Expr, Stmt, StmtClassDef};
use ruff_text_size::{Ranged, TextSize};
use std::path::PathBuf;

pub const CONSIDER_MERGING_CLASSES_INHERITED: Rule = Rule {
    code: "R8180",
    name: "consider-merging-classes-inherited",
    summary: "Several classes in one module extend the same model.",
    doc: r#"
## What it does

Reports when two or more classes in the same module extend a model with
`_inherit = "model"` (without their own `_name`). The message is shown on one
of them and lists the others.

## Why is this bad?

Extensions of one model spread over several classes are hard to find and
review, and their relative order of method overrides depends on the import
order of the files.
"#,
    check: Check::Builtin,
    min_odoo: None,
    max_odoo: None,
};

/// A class extending a model, collected per file.
#[derive(Debug, Clone)]
pub struct InheritFact {
    pub module_path: PathBuf,
    pub model: String,
    pub file_path: String,
    /// The `_inherit = ...` assignment.
    pub offset: TextSize,
    pub line: usize,
    /// 0-based, as pylint prints positions in its message.
    pub column: usize,
}

fn string_assignment<'a>(class: &'a StmtClassDef, name: &str) -> Vec<(&'a str, TextSize)> {
    class
        .body
        .iter()
        .filter_map(Stmt::as_assign_stmt)
        .filter(|a| matches!(a.targets.first(), Some(Expr::Name(n)) if n.id.as_str() == name))
        .filter_map(|a| a.value.as_string_literal_expr().map(|s| (s.value.to_str(), a.start())))
        .collect()
}

/// `_inherit = "model"` assignments of classes without a `_name` of their own.
pub fn collect(
    ctx: &PythonContext,
    line_column: impl Fn(TextSize) -> (usize, usize),
    is_suppressed: impl Fn(usize) -> bool,
) -> Vec<InheritFact> {
    let Some(module) = ctx.module else { return Vec::new() };
    let mut facts = Vec::new();
    for class in classes(ctx.parsed.suite()) {
        if !string_assignment(class, "_name").is_empty() {
            continue;
        }
        for (model, offset) in string_assignment(class, "_inherit") {
            let (line, column) = line_column(offset);
            // A disabled line does not take part at all, as in pylint-odoo.
            if is_suppressed(line) {
                continue;
            }
            facts.push(InheritFact {
                module_path: module.path.clone(),
                model: model.to_string(),
                file_path: ctx.file_path.to_string(),
                offset,
                line,
                column,
            });
        }
    }
    facts
}

/// Path relative to the working directory, as pylint-odoo prints it.
fn relative_to_cwd(path: &str) -> String {
    let path = std::path::Path::new(path);
    let relative = std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| path.to_path_buf());
    let text = relative.to_string_lossy().into_owned();
    text.strip_prefix("./").map(str::to_string).unwrap_or(text)
}

/// Combines the facts of all files: one violation per model extended by more
/// than one class in a module, on the last of them as pylint-odoo does.
pub fn violations(mut facts: Vec<InheritFact>) -> Vec<crate::diagnostics::Violation> {
    facts.sort_by(|a, b| {
        (&a.module_path, &a.model, &a.file_path, a.line).cmp(&(&b.module_path, &b.model, &b.file_path, b.line))
    });
    let mut result = Vec::new();
    for group in facts.chunk_by(|a, b| a.module_path == b.module_path && a.model == b.model) {
        let Some((reported, others)) = group.split_last() else {
            continue;
        };
        if others.is_empty() {
            continue;
        }
        let locations: Vec<String> = others
            .iter()
            .map(|f| format!("{}:{}:{}", relative_to_cwd(&f.file_path), f.line, f.column))
            .collect();
        result.push(crate::diagnostics::Violation {
            file_path: reported.file_path.clone(),
            line: reported.line,
            column: reported.column + 1,
            code: CONSIDER_MERGING_CLASSES_INHERITED.code.to_string(),
            name: CONSIDER_MERGING_CLASSES_INHERITED.name.to_string(),
            message: format!(
                "Consider merging classes inherited to \"{}\" from {}.",
                reported.model,
                locations.join(", ")
            ),
            fix: None,
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(file: &str, model: &str, line: usize) -> InheritFact {
        InheritFact {
            module_path: PathBuf::from("m"),
            model: model.into(),
            file_path: file.into(),
            offset: TextSize::new(0),
            line,
            column: 4,
        }
    }

    #[test]
    fn groups_per_model() {
        let v = violations(vec![
            fact("m/models/b.py", "res.partner", 3),
            fact("m/models/a.py", "res.partner", 7),
            fact("m/models/a.py", "sale.order", 9),
        ]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].file_path, "m/models/b.py");
        assert_eq!(
            v[0].message,
            "Consider merging classes inherited to \"res.partner\" from m/models/a.py:7:4."
        );
    }
}
