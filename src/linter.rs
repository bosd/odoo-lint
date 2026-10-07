use crate::config::OdooLintConfig;
use crate::diagnostics::Violation;
use crate::rules::{odoo001_missing_depends, odoo010_manifest_author};
use rayon::prelude::*;
use std::path::Path;
use walkdir::WalkDir;

pub fn lint_directory(root: &Path, _version: &str, config: &OdooLintConfig) -> Vec<Violation> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .collect::<Vec<_>>()
        .par_iter()
        .flat_map(|entry| {
            let mut violations = Vec::new();
            let path = entry.path();
            if path.file_name() == Some(std::ffi::OsStr::new("__manifest__.py")) {
                let content = std::fs::read_to_string(path).unwrap_or_default();
                let module_name = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                violations.extend(odoo010_manifest_author::check_manifest(
                    path.to_str().unwrap_or(""),
                    &content,
                    module_name,
                    config,
                ));
            } else if path.extension() == Some(std::ffi::OsStr::new("py")) {
                let content = std::fs::read_to_string(path).unwrap_or_default();
                violations.extend(odoo001_missing_depends::check_python_file(
                    path.to_str().unwrap_or(""),
                    &content,
                ));
            }
            violations
        })
        .collect()
}
