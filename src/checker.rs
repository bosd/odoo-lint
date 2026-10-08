//! What a rule sees while it runs, and how it reports violations.

use crate::diagnostics::Violation;
use crate::manifest::Manifest;
use crate::rules::Rule;
use crate::semantic::Semantic;
use crate::settings::Settings;
use ruff_python_ast::ModModule;
use ruff_python_parser::Parsed;
use ruff_source_file::LineIndex;
use ruff_text_size::TextSize;
use std::path::PathBuf;
use std::sync::Arc;

/// The Odoo module (addon) a file belongs to.
#[derive(Debug)]
pub struct ModuleInfo {
    /// Folder name, e.g. `sale_stock`.
    pub name: String,
    pub path: PathBuf,
    /// The manifest file Odoo loads (`__manifest__.py` before `__openerp__.py`).
    pub manifest_path: PathBuf,
    /// `None` when the manifest could not be parsed.
    pub manifest: Option<Arc<Manifest>>,
}

/// Context for rules that inspect a Python file.
pub struct PythonContext<'a> {
    pub file_path: &'a str,
    pub source: &'a str,
    pub parsed: &'a Parsed<ModModule>,
    pub semantic: &'a Semantic,
    pub module: Option<&'a ModuleInfo>,
    pub settings: &'a Settings,
}

/// Context for rules that inspect a module manifest.
pub struct ManifestContext<'a> {
    pub file_path: &'a str,
    pub source: &'a str,
    pub manifest: &'a Manifest,
    pub module: &'a ModuleInfo,
    pub settings: &'a Settings,
}

/// Collects violations for one file, translating offsets to line/column.
pub struct Reporter<'a> {
    file_path: &'a str,
    source: &'a str,
    line_index: &'a LineIndex,
    pub violations: Vec<Violation>,
}

impl<'a> Reporter<'a> {
    pub fn new(file_path: &'a str, source: &'a str, line_index: &'a LineIndex) -> Self {
        Self {
            file_path,
            source,
            line_index,
            violations: Vec::new(),
        }
    }

    /// 1-based line number of `offset`, for messages that mention lines.
    pub fn line_of(&self, offset: TextSize) -> usize {
        self.line_index.line_index(offset).get()
    }

    /// Reports at the start of a 1-based line, for files without an AST.
    pub fn report_line(&mut self, rule: &Rule, line: usize, message: impl Into<String>) {
        self.violations.push(Violation {
            file_path: self.file_path.to_string(),
            line,
            column: 1,
            code: rule.code.to_string(),
            name: rule.name.to_string(),
            message: message.into(),
        });
    }

    pub fn report(&mut self, rule: &Rule, offset: TextSize, message: impl Into<String>) {
        let location = self.line_index.line_column(offset, self.source);
        self.violations.push(Violation {
            file_path: self.file_path.to_string(),
            line: location.line.get(),
            column: location.column.get(),
            code: rule.code.to_string(),
            name: rule.name.to_string(),
            message: message.into(),
        });
    }
}

/// Test helper: run a single Python rule on `source` with default settings.
#[doc(hidden)]
pub fn run_python_rule(rule: &Rule, source: &str) -> Vec<Violation> {
    run_python_rule_with(rule, source, "test.py", None, &Settings::default())
}

/// Test helper: run a single Python rule on `source` as file `file_path` of
/// `module`, with `settings`.
#[doc(hidden)]
pub fn run_python_rule_with(
    rule: &Rule,
    source: &str,
    file_path: &str,
    module: Option<&ModuleInfo>,
    settings: &Settings,
) -> Vec<Violation> {
    let Ok(parsed) = ruff_python_parser::parse_module(source) else {
        return Vec::new();
    };
    let semantic = Semantic::new(parsed.suite());
    let line_index = LineIndex::from_source_text(source);
    let mut reporter = Reporter::new(file_path, source, &line_index);
    if let crate::rules::Check::Python(check) = rule.check {
        let ctx = PythonContext {
            file_path,
            source,
            parsed: &parsed,
            semantic: &semantic,
            module,
            settings,
        };
        check(&ctx, &mut reporter);
    }
    reporter.violations
}

/// Test helper: run a manifest rule on the `__manifest__.py` in `dir`.
#[doc(hidden)]
pub fn run_manifest_rule_in_dir(rule: &Rule, dir: &std::path::Path, settings: &Settings) -> Vec<Violation> {
    let path = dir.join("__manifest__.py");
    let source = std::fs::read_to_string(&path).expect("test module has a manifest");
    let Some(manifest) = Manifest::parse(&source) else {
        return Vec::new();
    };
    let manifest = Arc::new(manifest);
    let module = ModuleInfo {
        name: dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: dir.to_path_buf(),
        manifest_path: path.clone(),
        manifest: Some(manifest.clone()),
    };
    let file_path = path.to_string_lossy().into_owned();
    let line_index = LineIndex::from_source_text(&source);
    let mut reporter = Reporter::new(&file_path, &source, &line_index);
    if let crate::rules::Check::Manifest(check) = rule.check {
        let ctx = ManifestContext {
            file_path: &file_path,
            source: &source,
            manifest: &manifest,
            module: &module,
            settings,
        };
        check(&ctx, &mut reporter);
    }
    reporter.violations
}

/// Test helper: run a single manifest rule on `source` for module `module_name`.
#[doc(hidden)]
pub fn run_manifest_rule(rule: &Rule, source: &str, module_name: &str, settings: &Settings) -> Vec<Violation> {
    let Some(manifest) = Manifest::parse(source) else {
        return Vec::new();
    };
    let manifest = Arc::new(manifest);
    let module = ModuleInfo {
        name: module_name.to_string(),
        path: PathBuf::from(module_name),
        manifest_path: PathBuf::from(module_name).join("__manifest__.py"),
        manifest: Some(manifest.clone()),
    };
    let line_index = LineIndex::from_source_text(source);
    let mut reporter = Reporter::new("__manifest__.py", source, &line_index);
    if let crate::rules::Check::Manifest(check) = rule.check {
        let ctx = ManifestContext {
            file_path: "__manifest__.py",
            source,
            manifest: &manifest,
            module: &module,
            settings,
        };
        check(&ctx, &mut reporter);
    }
    reporter.violations
}
