//! Applies fixes: lint, apply every fix that does not overlap another one,
//! lint again, until nothing is left to fix. Everything happens in memory;
//! the caller writes the changed files or shows a diff.

use crate::diagnostics::Violation;
use crate::fix::{apply_edits, select_non_overlapping, Edit, FixMode};
use crate::linter::{collect_files, lint_files_with, lint_units};
use crate::settings::Settings;
use crate::sources::{normalize_newlines, Sources};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Fixes can enable other fixes; this bounds runaway loops.
const MAX_PASSES: usize = 20;

#[derive(Debug)]
pub struct FixResult {
    /// Violations left after fixing.
    pub remaining: Vec<Violation>,
    /// Number of fixes applied.
    pub fixed: usize,
    /// Changed files: path, contents before fixing, new contents.
    pub changed: Vec<(PathBuf, String, String)>,
    /// Files to copy, `(from, to)`: binary files the fixes replace.
    pub copies: Vec<(PathBuf, PathBuf)>,
}

impl FixResult {
    /// Writes the changed files and copies the copied ones.
    pub fn write(&self) -> Result<(), String> {
        for (path, _, new) in &self.changed {
            std::fs::write(path, new).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
        for (from, to) in &self.copies {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
            std::fs::copy(from, to).map_err(|e| format!("cannot copy {} to {}: {e}", from.display(), to.display()))?;
        }
        Ok(())
    }

    /// What `write` would do, as a diff and lines for the copied files.
    pub fn diff(&self) -> String {
        let mut text: String = self
            .changed
            .iter()
            .map(|(path, old, new)| unified_diff(path, old, new))
            .collect();
        for (from, to) in &self.copies {
            text.push_str(&format!("Copy {} to {}\n", from.display(), to.display()));
        }
        text
    }

    /// The number of files `write` changes or copies.
    pub fn files(&self) -> usize {
        self.changed.len() + self.copies.len()
    }
}

fn is_po(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "po" || e == "pot")
}

/// The text the rules computed offsets on: PO files with universal newlines.
fn current_text(sources: &Sources, path: &Path) -> Option<String> {
    let text = sources.read_to_string(path).ok()?;
    Some(if is_po(path) { normalize_newlines(&text) } else { text })
}

pub fn fix_paths(paths: &[PathBuf], settings: &Settings, mode: FixMode) -> FixResult {
    fix_paths_with(paths, settings, mode, &Sources::default())
}

/// Fixes `paths` as `sources` has them, e.g. with an editor's unsaved
/// documents in it. `sources` holds the fixed contents afterwards.
pub fn fix_paths_with(paths: &[PathBuf], settings: &Settings, mode: FixMode, sources: &Sources) -> FixResult {
    let mut originals: HashMap<PathBuf, String> = HashMap::new();
    let mut copies: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut fixed = 0;
    let files = collect_files(paths, settings);
    let units = lint_units(&files, sources);
    let mut violations = lint_files_with(&files, settings, sources);
    for _ in 0..MAX_PASSES {
        let candidates = violations.iter().filter_map(|v| {
            let fix = v.fix.as_ref().filter(|f| mode.allows(f.applicability))?;
            // A copy is done in the end; the file keeps reporting until then.
            if fix
                .copy
                .as_ref()
                .is_some_and(|(_, to)| copies.iter().any(|(_, t)| t == to))
            {
                return None;
            }
            Some((PathBuf::from(&v.file_path), fix))
        });
        let chosen = select_non_overlapping(candidates);
        if chosen.is_empty() {
            break;
        }
        let mut edits_by_file: HashMap<PathBuf, Vec<&Edit>> = HashMap::new();
        for (_, fix) in &chosen {
            if let Some(copy) = &fix.copy {
                copies.push(copy.clone());
            }
        }
        for (file, fix) in &chosen {
            for edit in fix.all_edits() {
                let target = edit.path.clone().unwrap_or_else(|| file.clone());
                edits_by_file.entry(target).or_default().push(edit);
            }
        }
        let mut changed_units = HashSet::new();
        let mut changed_paths: HashSet<PathBuf> = HashSet::new();
        for (path, edits) in edits_by_file {
            let Some(text) = current_text(sources, &path) else {
                continue;
            };
            let new_text = apply_edits(&text, &edits);
            if new_text != text {
                if !originals.contains_key(&path) {
                    let before = sources.read_to_string(&path).unwrap_or_default();
                    originals.insert(path.clone(), before);
                }
                changed_units.insert(units.get(&path).cloned().unwrap_or_else(|| path.clone()));
                changed_paths.insert(path.clone());
                sources.set(path, new_text);
            }
        }
        // A fix whose edits changed nothing (already applied) is not counted.
        fixed += chosen
            .iter()
            .filter(|(file, fix)| {
                fix.copy.is_some()
                    || fix
                        .edits
                        .iter()
                        .any(|e| changed_paths.contains(e.path.as_ref().unwrap_or(file)))
            })
            .count();
        if changed_units.is_empty() {
            break;
        }
        // Only the modules that changed can have new results.
        let relint: Vec<PathBuf> = files
            .iter()
            .filter(|f| units.get(*f).is_some_and(|u| changed_units.contains(u)))
            .cloned()
            .collect();
        let relinted: HashSet<String> = relint.iter().map(|f| f.to_string_lossy().into_owned()).collect();
        // Module rules report on files that are not linted themselves (a
        // manifest, a README): drop everything of a changed unit.
        violations.retain(|v| {
            !relinted.contains(&v.file_path) && !changed_units.iter().any(|u| Path::new(&v.file_path).starts_with(u))
        });
        violations.extend(lint_files_with(&relint, settings, sources));
        violations.sort();
        violations.dedup();
    }
    let mut changed: Vec<(PathBuf, String, String)> = originals
        .into_iter()
        .filter_map(|(path, old)| {
            let mut new = sources.read_to_string(&path).ok()?;
            // PO files are fixed with universal newlines: keep a CRLF file CRLF.
            if is_po(&path) && old.contains("\r\n") && !new.contains("\r\n") {
                new = new.replace('\n', "\r\n");
            }
            (new != old).then_some((path, old, new))
        })
        .collect();
    changed.sort();
    violations.retain(|v| {
        !v.fix
            .as_ref()
            .and_then(|f| f.copy.as_ref())
            .is_some_and(|copy| copies.contains(copy))
    });
    copies.sort();
    FixResult {
        remaining: violations,
        fixed,
        changed,
        copies,
    }
}

/// A unified diff of a changed file, as `git diff` shows it.
pub fn unified_diff(path: &Path, old: &str, new: &str) -> String {
    let name = path.to_string_lossy();
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{name}"), &format!("b/{name}"))
        .to_string()
}
