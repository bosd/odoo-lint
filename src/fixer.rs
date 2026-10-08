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
    /// Changed files: path, contents on disk, new contents.
    pub changed: Vec<(PathBuf, String, String)>,
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
    let sources = Sources::default();
    let mut originals: HashMap<PathBuf, String> = HashMap::new();
    let mut fixed = 0;
    let files = collect_files(paths, settings);
    let units = lint_units(&files, &sources);
    let mut violations = lint_files_with(&files, settings, &sources);
    for _ in 0..MAX_PASSES {
        let candidates = violations.iter().filter_map(|v| {
            let fix = v.fix.as_ref().filter(|f| mode.allows(f.applicability))?;
            Some((PathBuf::from(&v.file_path), fix))
        });
        let chosen = select_non_overlapping(candidates);
        if chosen.is_empty() {
            break;
        }
        let mut edits_by_file: HashMap<PathBuf, Vec<&Edit>> = HashMap::new();
        for (file, fix) in &chosen {
            for edit in &fix.edits {
                let target = edit.path.clone().unwrap_or_else(|| file.clone());
                edits_by_file.entry(target).or_default().push(edit);
            }
        }
        let mut changed_units = HashSet::new();
        for (path, edits) in edits_by_file {
            let Some(text) = current_text(&sources, &path) else {
                continue;
            };
            let new_text = apply_edits(&text, &edits);
            if new_text != text {
                if !originals.contains_key(&path) {
                    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
                    originals.insert(path.clone(), on_disk);
                }
                changed_units.insert(units.get(&path).cloned().unwrap_or_else(|| path.clone()));
                sources.set(path, new_text);
            }
        }
        fixed += chosen.len();
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
        violations.retain(|v| !relinted.contains(&v.file_path));
        violations.extend(lint_files_with(&relint, settings, &sources));
        violations.sort();
    }
    let changed = sources
        .changed()
        .into_iter()
        .map(|(path, new)| {
            let old = originals.remove(&path).unwrap_or_default();
            (path, old, new)
        })
        .collect();
    FixResult {
        remaining: violations,
        fixed,
        changed,
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
