//! Runs the enabled rules over files and modules.

use crate::checker::{ManifestContext, ModuleInfo, PythonContext, Reporter};
use crate::diagnostics::Violation;
use crate::manifest::{is_manifest_file_name, Manifest, MANIFEST_FILE_NAMES};
use crate::po::{PoError, PoFile};
use crate::rules::po::PoContext;
use crate::rules::python::inherit::{self, InheritFact, CONSIDER_MERGING_CLASSES_INHERITED};
use crate::rules::{e0001_syntax_error, Check, Rule};
use crate::semantic::Semantic;
use crate::settings::{Settings, DEFAULT_EXCLUDES};
use crate::sources::{normalize_newlines, Sources};
use crate::suppression::Suppressions;
use rayon::prelude::*;
use ruff_python_parser::parse_module;
use ruff_source_file::LineIndex;
use ruff_text_size::TextSize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

/// Lints `paths` (files or directories) and returns sorted violations.
pub fn lint_paths(paths: &[PathBuf], settings: &Settings) -> Vec<Violation> {
    lint_paths_with(paths, settings, &Sources::default())
}

/// Like [`lint_paths`], reading files through `sources` (for fixes in memory).
pub fn lint_paths_with(paths: &[PathBuf], settings: &Settings, sources: &Sources) -> Vec<Violation> {
    lint_files_with(&collect_files(paths, settings), settings, sources)
}

/// Lints the given files (as collected from the paths, excludes applied).
pub fn lint_files_with(files: &[PathBuf], settings: &Settings, sources: &Sources) -> Vec<Violation> {
    let modules = resolve_modules(files, sources);
    let rules = settings.enabled_rules();

    let results: Vec<(Vec<Violation>, Vec<InheritFact>)> = files
        .par_iter()
        .map(|file| {
            let module = modules.get(file).and_then(Option::as_deref);
            lint_file(file, module, &rules, settings, sources)
        })
        .collect();
    let mut violations = Vec::new();
    let mut inherit_facts = Vec::new();
    for (file_violations, facts) in results {
        violations.extend(file_violations);
        inherit_facts.extend(facts);
    }
    // Rules across the files of a module.
    violations.extend(inherit::violations(inherit_facts));
    violations.sort();
    violations
}

/// The files to lint under `paths`, without excluded ones.
pub fn collect_files(paths: &[PathBuf], settings: &Settings) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in paths {
        if root.is_file() {
            // Patterns usually name folders: check the file's folders too.
            let excluded = settings.force_exclude && root.ancestors().any(|p| settings.is_excluded(p));
            if !excluded {
                files.push(root.clone());
            }
            continue;
        }
        let walker = WalkDir::new(root).into_iter().filter_entry(|entry| {
            // Never filter out the root the user asked for.
            if entry.depth() == 0 {
                return true;
            }
            let excluded_name = entry.file_type().is_dir()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| DEFAULT_EXCLUDES.contains(&name));
            !excluded_name && !settings.is_excluded(entry.path())
        });
        files.extend(
            walker
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(is_linted_extension))
                .map(|e| e.into_path()),
        );
    }
    files.sort();
    files.dedup();
    files
}

fn is_linted_extension(extension: &std::ffi::OsStr) -> bool {
    extension == "py" || is_po_extension(extension)
}

fn is_po_extension(extension: &std::ffi::OsStr) -> bool {
    extension == "po" || extension == "pot"
}

/// Maps every file to the module (closest ancestor with a manifest) it is in.
/// The unit every file is linted with: its module's folder, or the file itself
/// outside a module. Rules only look across files within one unit.
pub fn lint_units(files: &[PathBuf], sources: &Sources) -> HashMap<PathBuf, PathBuf> {
    resolve_modules(files, sources)
        .into_iter()
        .map(|(file, module)| {
            let unit = module.map_or_else(|| file.clone(), |m| m.path.clone());
            (file, unit)
        })
        .collect()
}

fn resolve_modules(files: &[PathBuf], sources: &Sources) -> HashMap<PathBuf, Option<Arc<ModuleInfo>>> {
    let mut by_dir: HashMap<PathBuf, Option<Arc<ModuleInfo>>> = HashMap::new();
    let mut result = HashMap::new();
    for file in files {
        let mut found = None;
        for dir in file.ancestors().skip(1) {
            if let Some(cached) = by_dir.get(dir) {
                if cached.is_some() {
                    found = cached.clone();
                    break;
                }
                continue;
            }
            let module = load_module(dir, sources);
            by_dir.insert(dir.to_path_buf(), module.clone());
            if module.is_some() {
                found = module;
                break;
            }
        }
        result.insert(file.clone(), found);
    }
    result
}

fn load_module(dir: &Path, sources: &Sources) -> Option<Arc<ModuleInfo>> {
    let manifest_path = MANIFEST_FILE_NAMES.iter().map(|n| dir.join(n)).find(|p| p.is_file())?;
    let source = sources.read_to_string(&manifest_path).ok()?;
    let name = (if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    })
    .canonicalize()
    .ok()
    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
    .unwrap_or_default();
    Some(Arc::new(ModuleInfo {
        name,
        path: dir.to_path_buf(),
        manifest_path,
        manifest: Manifest::parse(&source).map(Arc::new),
    }))
}

/// Violations of one file, and what it contributes to cross-file rules.
fn lint_file(
    path: &Path,
    module: Option<&ModuleInfo>,
    rules: &[&'static Rule],
    settings: &Settings,
    sources: &Sources,
) -> (Vec<Violation>, Vec<InheritFact>) {
    if path.extension().is_some_and(is_po_extension) {
        return (lint_po_file(path, rules, settings, sources), Vec::new());
    }
    let Ok(source) = sources.read_to_string(path) else {
        return (Vec::new(), Vec::new());
    };
    let file_path = path.to_string_lossy();
    let rules: Vec<&Rule> = rules
        .iter()
        .copied()
        .filter(|rule| !settings.is_ignored_in_file(path, rule))
        .collect();
    let line_index = LineIndex::from_source_text(&source);
    let mut reporter = Reporter::new(&file_path, &source, &line_index);

    let parsed = match parse_module(&source) {
        Ok(parsed) => parsed,
        Err(error) => {
            if rules.iter().any(|r| r.code == e0001_syntax_error::RULE.code) {
                let offset = error.location.start().min(TextSize::of(source.as_str()));
                reporter.report(
                    &e0001_syntax_error::RULE,
                    offset,
                    format!("Parsing failed: '{}'", error.error),
                );
            }
            return (reporter.violations, Vec::new());
        }
    };

    let semantic = Semantic::new(parsed.suite());
    let python_ctx = PythonContext {
        file_path: &file_path,
        source: &source,
        parsed: &parsed,
        semantic: &semantic,
        module,
        settings,
    };
    for rule in &rules {
        if let Check::Python(check) = rule.check {
            check(&python_ctx, &mut reporter);
        }
    }

    let is_manifest = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(is_manifest_file_name);
    if let (true, Some(module)) = (is_manifest, module) {
        // Every manifest file in the module folder is checked, like pylint-odoo
        // does, including e.g. a stale __openerp__.py next to __manifest__.py.
        let in_module_folder = path.parent() == Some(module.path.as_path())
            || (module.path.as_os_str().is_empty() && path.parent() == Some(Path::new("")));
        if let (true, Some(manifest)) = (in_module_folder, Manifest::parse(&source)) {
            let manifest_ctx = ManifestContext {
                file_path: &file_path,
                source: &source,
                manifest: &manifest,
                module,
                settings,
            };
            for rule in &rules {
                if let Check::Manifest(check) = rule.check {
                    check(&manifest_ctx, &mut reporter);
                }
            }
        }
    }

    let suppressions = Suppressions::from_tokens(&source, parsed.tokens(), &line_index);
    let inherit_facts = if rules.iter().any(|r| r.code == CONSIDER_MERGING_CLASSES_INHERITED.code) {
        let rule = &CONSIDER_MERGING_CLASSES_INHERITED;
        inherit::collect(
            &python_ctx,
            |offset| {
                let location = line_index.line_column(offset, &source);
                (location.line.get(), location.column.get() - 1)
            },
            |line| suppressions.is_suppressed(line, rule.code, rule.name),
        )
    } else {
        Vec::new()
    };
    let violations = reporter
        .violations
        .into_iter()
        .filter(|v| !suppressions.is_suppressed(v.line, &v.code, &v.name))
        .collect();
    (violations, inherit_facts)
}

/// Lints a `.po`/`.pot` file. Files are read as UTF-8 with universal
/// newlines, like OCA's `oca-checks-po` reads them.
fn lint_po_file(path: &Path, rules: &[&'static Rule], settings: &Settings, sources: &Sources) -> Vec<Violation> {
    let rules: Vec<&Rule> = rules
        .iter()
        .copied()
        .filter(|rule| !settings.is_ignored_in_file(path, rule))
        .collect();
    let file_path = path.to_string_lossy();
    let Ok(bytes) = sources.read(path) else {
        return Vec::new();
    };
    match String::from_utf8(bytes) {
        Ok(source) => lint_po_source_with(&file_path, &source, &rules, settings, sources),
        Err(error) => {
            let position = error.utf8_error().valid_up_to();
            let byte = error.as_bytes()[position];
            let decode_error = PoError {
                line: 1,
                message: format!(
                    "'utf-8' codec can't decode byte {byte:#04x} in position {position}: invalid start byte"
                ),
            };
            run_po_rules(&file_path, "", Err(&decode_error), path, &rules, settings, sources)
        }
    }
}

/// Lints PO file contents; `file_path` decides the data section (`i18n`,
/// `i18n_extra`).
pub fn lint_po_source(file_path: &str, source: &str, rules: &[&Rule], settings: &Settings) -> Vec<Violation> {
    lint_po_source_with(file_path, source, rules, settings, &Sources::default())
}

fn lint_po_source_with(
    file_path: &str,
    source: &str,
    rules: &[&Rule],
    settings: &Settings,
    sources: &Sources,
) -> Vec<Violation> {
    let normalized = normalize_newlines(source);
    let parsed = PoFile::parse(&normalized);
    run_po_rules(
        file_path,
        &normalized,
        parsed.as_ref(),
        Path::new(file_path),
        rules,
        settings,
        sources,
    )
}

fn run_po_rules(
    file_path: &str,
    source: &str,
    po: Result<&PoFile, &PoError>,
    path: &Path,
    rules: &[&Rule],
    settings: &Settings,
    sources: &Sources,
) -> Vec<Violation> {
    let data_section = path
        .parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let line_index = LineIndex::from_source_text(source);
    let mut reporter = Reporter::new(file_path, source, &line_index);
    let ctx = PoContext {
        file_path,
        source,
        po,
        data_section: &data_section,
        settings,
        sources,
    };
    for rule in rules {
        if let Check::Po(check) = rule.check {
            check(&ctx, &mut reporter);
        }
    }
    reporter.violations
}
