//! `odl hook`: lints the file an AI coding agent just edited, for a
//! post-edit hook. The agent harness passes a JSON event on stdin; findings go
//! back as context for the model, not as an error.

use crate::diagnostics::Violation;
use crate::fix::Applicability;
use crate::linter;
use crate::settings::{CliOverrides, Settings};
use crate::sources::Sources;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The edited file in a hook event: `tool_input.file_path` (Claude Code),
/// or the other spellings harnesses use. Relative paths are relative to the
/// event's `cwd`.
pub fn edited_file(event: &Value) -> Option<PathBuf> {
    let input = event.get("tool_input").or_else(|| event.get("input")).unwrap_or(event);
    let path = ["file_path", "filePath", "path"]
        .iter()
        .find_map(|key| input.get(key).and_then(Value::as_str))?;
    let path = PathBuf::from(path);
    Some(match event.get("cwd").and_then(Value::as_str) {
        Some(cwd) if path.is_relative() => Path::new(cwd).join(path),
        _ => path,
    })
}

fn is_linted(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e == "py" || e == "po" || e == "pot" || e == "xml")
}

/// The violations in `path`, if it is a file of an Odoo module.
pub fn lint_edited(path: &Path) -> Option<Vec<Violation>> {
    if !is_linted(path) || !path.is_file() {
        return None;
    }
    let file = path.to_path_buf();
    // Outside an addon the Odoo rules make no sense.
    let units = linter::lint_units(std::slice::from_ref(&file), &Sources::default());
    if units.get(&file).is_none_or(|unit| *unit == file) {
        return None;
    }
    let settings = Settings::load(path, None, CliOverrides::default()).ok()?.settings;
    Some(linter::lint_paths(&[file], &settings))
}

/// What to tell the model about the violations in `path`.
pub fn report(path: &Path, violations: &[Violation]) -> String {
    let mut text = format!("odoo-lint found {} issue(s) in {}:\n", violations.len(), path.display());
    for v in violations {
        text.push_str(&format!("  line {}: {} {} ({})", v.line, v.code, v.message, v.name));
        match v.fix.as_ref().map(|f| f.applicability) {
            Some(Applicability::Safe) => text.push_str(" [fixable]"),
            Some(Applicability::Unsafe) => text.push_str(" [unsafe fix]"),
            None => {}
        }
        text.push('\n');
    }
    let has = |a: Applicability| {
        violations
            .iter()
            .any(|v| v.fix.as_ref().is_some_and(|f| f.applicability == a))
    };
    if has(Applicability::Safe) {
        text.push_str(&format!("Apply the safe fixes: `odl check --fix {}`\n", path.display()));
    }
    if has(Applicability::Unsafe) {
        text.push_str(&format!(
            "Unsafe fixes can change behaviour; review with `odl check --diff --unsafe-fixes {}`\n",
            path.display()
        ));
    }
    text.push_str("Explain a code with `odl rule <code>`.");
    text
}

/// The hook's stdout for an event, in Claude Code's format: `None` when there
/// is nothing to say.
pub fn run(event: &str) -> Option<String> {
    let event: Value = serde_json::from_str(event).ok()?;
    let path = edited_file(&event)?;
    let violations = lint_edited(&path)?;
    if violations.is_empty() {
        return None;
    }
    let event_name = event
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or("PostToolUse");
    Some(
        json!({
            "hookSpecificOutput": {
                "hookEventName": event_name,
                "additionalContext": report(&path, &violations),
            }
        })
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn edited_file_spellings() {
        let claude = json!({"cwd": "/work", "tool_input": {"file_path": "a/b.py"}});
        assert_eq!(edited_file(&claude), Some(PathBuf::from("/work/a/b.py")));
        let other = json!({"input": {"filePath": "/abs/c.po"}});
        assert_eq!(edited_file(&other), Some(PathBuf::from("/abs/c.po")));
        assert_eq!(edited_file(&json!({"tool_input": {"command": "ls"}})), None);
    }

    #[test]
    fn lints_addon_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let module = dir.path().join("acme_hook");
        fs::create_dir_all(module.join("models")).unwrap();
        fs::write(
            module.join("__manifest__.py"),
            "{\n    'name': 'Hook',\n    'license': 'AGPL-3',\n    'installable': True,\n}\n",
        )
        .unwrap();
        let loose = dir.path().join("script.py");
        fs::write(&loose, "import os\n").unwrap();
        let event =
            |path: &Path| json!({"hook_event_name": "PostToolUse", "tool_input": {"file_path": path}}).to_string();

        assert_eq!(run(&event(&loose)), None, "not in an addon");
        let output: Value = serde_json::from_str(&run(&event(&module.join("__manifest__.py"))).unwrap()).unwrap();
        let context = output["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
        assert!(context.contains("C8116"), "{context}");
        assert!(context.contains("[fixable]"), "{context}");
        assert_eq!(output["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    }
}
