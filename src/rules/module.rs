//! Module checks: the non-XML checks of OCA's `oca-checks-odoo-module`
//! (MOD001-MOD009, same names, same results on its test repository) and
//! odoo-lint's own checks of the files in a module (ODOO002, ODOO003).
//!
//! The module-wide checks run once per module, on its folder and manifest;
//! the Python ones on the Python files of installable modules.

use crate::checker::{ModuleInfo, PythonContext, Reporter};
use crate::diagnostics::Violation;
use crate::fix::{Edit, Fix};
use crate::rules::python::{classes, source_of};
use crate::rules::{Check, Rule};
use crate::settings::Settings;
use crate::sources::Sources;
use crate::visit::{walk, Node};
use crate::xml::{is_installable, py_glob};
use ruff_python_ast::{Expr, Stmt};
use ruff_text_size::{Ranged, TextSize};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

pub struct ModuleContext<'a> {
    pub module: &'a ModuleInfo,
    pub settings: &'a Settings,
    pub sources: &'a Sources,
}

#[derive(Default)]
pub struct ModuleReporter {
    pub violations: Vec<Violation>,
}

impl ModuleReporter {
    pub fn report(&mut self, rule: &Rule, path: &Path, line: usize, message: impl Into<String>) {
        self.violations.push(Violation {
            file_path: path.to_string_lossy().into_owned(),
            line,
            column: 1,
            code: rule.code.to_string(),
            name: rule.name.to_string(),
            message: message.into(),
            fix: None,
        });
    }
}

/// Runs the module rules on `module`.
pub fn lint_module(module: &ModuleInfo, rules: &[&Rule], settings: &Settings, sources: &Sources) -> Vec<Violation> {
    let ctx = ModuleContext {
        module,
        settings,
        sources,
    };
    let mut reporter = ModuleReporter::default();
    // The CSV rules share a check function, which reports for both.
    let mut done: Vec<usize> = Vec::new();
    for rule in rules {
        if let Check::Module(check) = rule.check {
            if !done.contains(&(check as usize)) {
                done.push(check as usize);
                check(&ctx, &mut reporter);
            }
        }
    }
    reporter.violations
}

// --- Manifest files ---------------------------------------------------------

/// Manifest keys whose files Odoo loads; `qweb` and `assets` are globs.
const DATA_KEYS: &[&str] = &[
    "data",
    "demo",
    "demo_xml",
    "init_xml",
    "qweb",
    "test",
    "update_xml",
    "assets",
];

/// The string items of a list or tuple literal.
fn strings(expr: &Expr) -> Vec<&str> {
    let items: &[Expr] = match expr {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tuple) => &tuple.elts,
        _ => return Vec::new(),
    };
    items
        .iter()
        .filter_map(|i| i.as_string_literal_expr().map(|s| s.value.to_str()))
        .collect()
}

/// `relative` without `.` and `..` parts, `/`-separated.
fn normalize(relative: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str().unwrap_or_default()),
            Component::ParentDir => {
                parts.pop();
            }
            _ => {}
        }
    }
    parts.join("/")
}

/// The files the manifest lists, per key, relative to the module and
/// normalised, each once per key.
fn manifest_files(module: &ModuleInfo) -> Vec<(String, &'static str)> {
    let Some(manifest) = &module.manifest else {
        return Vec::new();
    };
    let mut files: Vec<(String, &'static str)> = Vec::new();
    for &section in DATA_KEYS {
        let Some(value) = manifest.get(section) else { continue };
        let names: Vec<String> = if section == "qweb" || section == "assets" {
            let lists: Vec<&Expr> = match value {
                Expr::Dict(dict) => dict.items.iter().map(|item| &item.value).collect(),
                other => vec![other],
            };
            let Some(addons) = module.path.parent() else { continue };
            let prefix = format!("{}/", module.name);
            lists
                .into_iter()
                .flat_map(strings)
                .flat_map(|pattern| {
                    let pattern = if section == "qweb" {
                        format!("{prefix}{pattern}")
                    } else {
                        pattern.to_string()
                    };
                    py_glob(addons, &pattern)
                })
                .filter_map(|path| path.strip_prefix(&prefix).map(str::to_string))
                .collect()
        } else {
            strings(value).into_iter().map(str::to_string).collect()
        };
        for name in names {
            let entry = (normalize(&name), section);
            if !files.contains(&entry) {
                files.push(entry);
            }
        }
    }
    files
}

fn manifest_line_one(module: &ModuleInfo) -> &Path {
    &module.manifest_path
}

// --- MOD001 manifest-syntax-error ---------------------------------------------

pub const MANIFEST_SYNTAX_ERROR: Rule = Rule {
    code: "MOD001",
    name: "manifest-syntax-error",
    summary: "The manifest could not be loaded.",
    doc: r#"
## What it does

Reports a manifest that is not a dictionary of literals, an empty one, and
a module without an `__init__.py`.

## Why is this bad?

Odoo reads the manifest with `ast.literal_eval`: variables, calls or
expressions in it make the module fail to load, and so does a missing
`__init__.py`.
"#,
    check: Check::Module(check_manifest_syntax_error),
    min_odoo: None,
    max_odoo: None,
};

fn check_manifest_syntax_error(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let module = ctx.module;
    let loaded = module.path.join("__init__.py").is_file()
        && module.manifest.as_ref().is_some_and(|m| !m.dict().items.is_empty());
    if !loaded {
        reporter.report(
            &MANIFEST_SYNTAX_ERROR,
            manifest_line_one(module),
            1,
            "Manifest could not be loaded",
        );
    }
}

// --- MOD002 file-not-used ---------------------------------------------------

/// Folders of data files, as OCA lists them.
const DATA_DIRS: &[&str] = &[
    "data",
    "datas",
    "demo",
    "demos",
    "report",
    "reports",
    "security",
    "template",
    "templates",
    "view",
    "views",
    "wizard",
    "wizards",
];

/// Manifest key for data files the module loads itself (e.g. from a hook).
const DATA_MANUAL_KEY: &str = "oca_data_manual";

pub const FILE_NOT_USED: Rule = Rule {
    code: "MOD002",
    name: "file-not-used",
    summary: "A data file is not referenced in the manifest.",
    doc: r#"
## What it does

Reports `.xml` and `.csv` files directly in a data folder of the module
(`data`, `demo`, `report`, `security`, `views`, `wizard`… and their plural
or singular forms) that the manifest does not list.

## Why is this bad?

Odoo only loads the files the manifest lists: a view, rule or record in an
unlisted file silently does nothing. Add it to the manifest, or remove it.

A file the module loads itself (from a `post_init_hook`, say) can be listed
under the manifest key `"oca_data_manual"`.
"#,
    check: Check::Module(check_file_not_used),
    min_odoo: None,
    max_odoo: None,
};

fn check_file_not_used(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let module = ctx.module;
    if !is_installable(module) {
        return;
    }
    let referenced: BTreeSet<String> = manifest_files(module).into_iter().map(|(file, _)| file).collect();
    let manual: Vec<String> = module
        .manifest
        .as_ref()
        .and_then(|m| m.get(DATA_MANUAL_KEY))
        .map(|value| strings(value).into_iter().map(normalize).collect())
        .unwrap_or_default();
    let Ok(dirs) = std::fs::read_dir(&module.path) else {
        return;
    };
    let mut unused = BTreeSet::new();
    for dir in dirs.flatten() {
        let dir_name = dir.file_name().to_string_lossy().into_owned();
        if !DATA_DIRS.contains(&dir_name.to_lowercase().as_str()) || !dir.path().is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(dir.path()) else {
            continue;
        };
        for file in files.flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            let extension = Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase());
            if !file.path().is_file() || !matches!(extension.as_deref(), Some("xml" | "csv")) {
                continue;
            }
            let relative = format!("{dir_name}/{name}");
            if !referenced.contains(&relative) && !manual.contains(&relative) {
                unused.insert(relative);
            }
        }
    }
    for relative in unused {
        reporter.report(
            &FILE_NOT_USED,
            manifest_line_one(module),
            1,
            format!("File \"{}/{relative}\" is not referenced in the manifest.", module.name),
        );
    }
}

// --- MOD003/MOD004 CSV ------------------------------------------------------

/// Where a CSV record is: file, line and its `id`.
type CsvPlace = (PathBuf, usize, String);

/// Python's `csv.field_size_limit()` default.
const CSV_FIELD_LIMIT: usize = 131_072;

/// A record of a CSV file and the physical line it ends on (Python's
/// `reader.line_num`).
struct CsvRecord {
    fields: Vec<String>,
    line: usize,
}

/// Reads CSV text as Python's `csv.reader` does with the default `excel`
/// dialect on a file opened in text mode: universal newlines, quotes only
/// special at the start of a field, empty lines skipped.
fn read_csv(text: &str) -> Result<Vec<CsvRecord>, String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut records = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut line = 0;
    let mut in_quotes = false;
    let mut quoted = false; // the field started with a quote
    let mut started = false; // the record has content
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if field.chars().count() > CSV_FIELD_LIMIT {
            return Err(format!("field larger than field limit ({CSV_FIELD_LIMIT})"));
        }
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => in_quotes = false,
                '\n' => {
                    line += 1;
                    field.push('\n');
                }
                other => field.push(other),
            }
            continue;
        }
        match c {
            ',' => {
                fields.push(std::mem::take(&mut field));
                quoted = false;
                started = true;
            }
            '"' if field.is_empty() && !quoted => {
                in_quotes = true;
                quoted = true;
                started = true;
            }
            '\n' => {
                line += 1;
                if started || !field.is_empty() {
                    fields.push(std::mem::take(&mut field));
                    records.push(CsvRecord {
                        fields: std::mem::take(&mut fields),
                        line,
                    });
                }
                quoted = false;
                started = false;
            }
            other => {
                field.push(other);
                started = true;
            }
        }
    }
    if started || !field.is_empty() || in_quotes {
        line += 1;
        fields.push(field);
        records.push(CsvRecord { fields, line });
    }
    Ok(records)
}

/// Python's message for a UTF-8 decoding error at `bytes[position..]`.
fn utf8_error_message(bytes: &[u8], error: &std::str::Utf8Error) -> String {
    let position = error.valid_up_to();
    let byte = bytes[position];
    let reason = match error.error_len() {
        None => "unexpected end of data",
        Some(_) if (0x80..0xc2).contains(&byte) || byte > 0xf4 => "invalid start byte",
        Some(_) => "invalid continuation byte",
    };
    format!("'utf-8' codec can't decode byte 0x{byte:02x} in position {position}: {reason}")
}

pub const CSV_SYNTAX_ERROR: Rule = Rule {
    code: "MOD003",
    name: "csv-syntax-error",
    summary: "A CSV file the manifest lists cannot be read.",
    doc: r#"
## What it does

Reports CSV files listed in the manifest that are missing, not valid UTF-8,
or have a field longer than Python's `csv` limit (131072 characters).

## Why is this bad?

Odoo fails to install or update the module.
"#,
    check: Check::Module(check_csv),
    min_odoo: None,
    max_odoo: None,
};

pub const CSV_DUPLICATE_RECORD_ID: Rule = Rule {
    code: "MOD004",
    name: "csv-duplicate-record-id",
    summary: "Two CSV records of a manifest key have the same `id`.",
    doc: r#"
## What it does

Reports CSV records whose `id` (XML id) another record of a CSV file under
the same manifest key already uses.

## Why is this bad?

The second record silently overwrites the first: an access right or record
is lost.
"#,
    check: Check::Module(check_csv),
    min_odoo: None,
    max_odoo: None,
};

fn check_csv(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let module = ctx.module;
    if !is_installable(module) {
        return;
    }
    let enabled = |rule: &Rule| ctx.settings.enabled_rules().iter().any(|r| r.code == rule.code);
    let (syntax, duplicates) = (enabled(&CSV_SYNTAX_ERROR), enabled(&CSV_DUPLICATE_RECORD_ID));
    let mut ids: Vec<(String, Vec<CsvPlace>)> = Vec::new();
    for (file, section) in manifest_files(module) {
        if !file.to_lowercase().ends_with(".csv") {
            continue;
        }
        let path = module.path.join(&file);
        let records = match ctx.sources.read(&path) {
            Err(_) => Err(format!("[Errno 2] No such file or directory: '{}'", path.display())),
            Ok(bytes) => match std::str::from_utf8(&bytes) {
                Err(error) => Err(utf8_error_message(&bytes, &error)),
                Ok(text) => read_csv(text),
            },
        };
        let records = match records {
            Ok(records) => records,
            Err(message) => {
                if syntax {
                    reporter.report(&CSV_SYNTAX_ERROR, &path, 1, message);
                }
                continue;
            }
        };
        let mut rows = records.into_iter();
        let Some(header) = rows.next() else { continue };
        let Some(column) = header.fields.iter().position(|f| f == "id") else {
            continue;
        };
        for row in rows {
            let id = row.fields.get(column).cloned().unwrap_or_else(|| "None".into());
            let key = format!("{section}/{id}");
            match ids.iter_mut().find(|(k, _)| *k == key) {
                Some((_, places)) => places.push((path.clone(), row.line, id)),
                None => ids.push((key, vec![(path.clone(), row.line, id)])),
            }
        }
    }
    if !duplicates {
        return;
    }
    for (_, places) in ids {
        if places.len() < 2 {
            continue;
        }
        let (path, line, id) = &places[0];
        let others: Vec<String> = places[1..]
            .iter()
            .map(|(p, l, _)| format!("{}:{l}", p.display()))
            .collect();
        reporter.report(
            &CSV_DUPLICATE_RECORD_ID,
            path,
            *line,
            format!("Duplicate CSV record `{id}` (also in {})", others.join(", ")),
        );
    }
}

// --- MOD005 prefer-readme-rst -----------------------------------------------

pub const PREFER_README_RST: Rule = Rule {
    code: "MOD005",
    name: "prefer-readme-rst",
    summary: "The module has a `README.md` instead of a `README.rst`.",
    doc: r#"
## What it does

Reports a `README.md` in an installable module.

## Why is this bad?

OCA's README generator, the Odoo Apps store and the module description in
Odoo read `README.rst` (or the `readme/` fragments it is built from).
"#,
    check: Check::Module(check_readme),
    min_odoo: None,
    max_odoo: None,
};

fn check_readme(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let readme = ctx.module.path.join("README.md");
    if is_installable(ctx.module) && readme.is_file() {
        reporter.report(&PREFER_README_RST, &readme, 1, "Prefer README.rst instead of README.md");
    }
}

// --- MOD006 weblate-component-too-long ----------------------------------------

/// Weblate's limit for a component name.
const MAX_WEBLATE_NAME_LENGTH: usize = 90;

pub const WEBLATE_COMPONENT_TOO_LONG: Rule = Rule {
    code: "MOD006",
    name: "weblate-component-too-long",
    summary: "Repository name, Odoo version and module name are too long for a Weblate component.",
    doc: r#"
## What it does

Reports modules for which `<repository>-<version>-<module>` (the Weblate
component OCA creates, such as `account-financial-reporting-18.0-account_tax_balance`)
is longer than 90 characters. The repository name comes from the first git
remote.

## Why is this bad?

Weblate cannot create the component, so the module cannot be translated.
"#,
    check: Check::Module(check_weblate_component),
    min_odoo: None,
    max_odoo: None,
};

/// The folder holding `.git` above `path`, and the git config to read.
fn git_config(path: &Path) -> Option<PathBuf> {
    let path = path.canonicalize().ok()?;
    for dir in path.ancestors() {
        let git = dir.join(".git");
        if git.is_dir() {
            return Some(git.join("config"));
        }
        if git.is_file() {
            // A worktree: `gitdir: <path>`, whose `commondir` holds the config.
            let pointer = std::fs::read_to_string(&git).ok()?;
            let gitdir = dir.join(pointer.trim().strip_prefix("gitdir:")?.trim());
            let common = std::fs::read_to_string(gitdir.join("commondir")).ok();
            let common = common.map_or(gitdir.clone(), |c| gitdir.join(c.trim()));
            return Some(common.join("config"));
        }
    }
    None
}

/// The repository name of the first remote (`git remote` sorts them), from
/// its URL; empty without a remote.
fn repo_name(path: &Path) -> String {
    let Some(config) = git_config(path).and_then(|p| std::fs::read_to_string(p).ok()) else {
        return String::new();
    };
    let mut remotes: Vec<(String, String)> = Vec::new();
    let mut current: Option<String> = None;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            current = line
                .strip_prefix("[remote \"")
                .and_then(|rest| rest.strip_suffix("\"]"))
                .map(str::to_string);
        } else if let (Some(name), Some(url)) = (&current, line.strip_prefix("url")) {
            if let Some(url) = url.trim_start().strip_prefix('=') {
                remotes.push((name.clone(), url.trim().to_string()));
            }
        }
    }
    remotes.sort();
    let Some((_, url)) = remotes.first() else {
        return String::new();
    };
    let base = url.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or_default();
    base.strip_suffix(".git").unwrap_or(base).to_string()
}

fn check_weblate_component(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let module = ctx.module;
    if !is_installable(module) {
        return;
    }
    let component = format!("{}-00.0-{}", repo_name(&module.path), module.name);
    let length = component.chars().count();
    if length > MAX_WEBLATE_NAME_LENGTH {
        reporter.report(
            &WEBLATE_COMPONENT_TOO_LONG,
            manifest_line_one(module),
            1,
            format!(
                "Repo Name + Odoo version + Module name is too long for weblate component: '{component}' size {length} is longer than {MAX_WEBLATE_NAME_LENGTH} characters"
            ),
        );
    }
}

// --- ODOO002/ODOO003 files in a module ----------------------------------------

/// Extensions of files that do not belong in a module: packages, archives,
/// executables, database dumps and videos.
const UNWANTED_EXTENSIONS: &[&str] = &[
    "rpm", "deb", "apk", "msi", "exe", "dll", "dmg", "iso", "jar", "zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "rar",
    "dump", "sql", "backup", "mp4", "mov", "avi", "mkv", "webm",
];

/// Default size limit of a file in a module, in KiB.
const DEFAULT_MAX_KIB: u64 = 1024;

/// Folders not to look into.
const SKIPPED_DIRS: &[&str] = &[".git", "__pycache__", "node_modules", ".venv", "venv"];

/// The files of a module, recursively, without skipped and excluded ones.
fn module_files(ctx: &ModuleContext) -> Vec<PathBuf> {
    walkdir::WalkDir::new(&ctx.module.path)
        .into_iter()
        .filter_entry(|e| !(e.file_type().is_dir() && SKIPPED_DIRS.contains(&e.file_name().to_string_lossy().as_ref())))
        .flatten()
        .filter(|e| e.file_type().is_file() && !ctx.settings.is_excluded(e.path()))
        .map(|e| e.path().to_path_buf())
        .collect()
}

pub const UNWANTED_FILE: Rule = Rule {
    code: "ODOO002",
    name: "module-unwanted-file",
    summary: "A package, archive, executable, database dump or video inside a module.",
    doc: r#"
## What it does

Reports files with an extension that does not belong in an Odoo module:
packages (`.rpm`, `.deb`), archives (`.zip`, `.tar.gz`, `.7z`…),
executables (`.exe`, `.dll`, `.jar`), database dumps (`.dump`, `.sql`,
`.backup`), disk images and videos.

## Why is this bad?

Such files end up in the repository and in every installation of the module.
They are usually committed by accident, and some (dumps) hold customer data.

## Options

```toml
[tool.odoo-lint.rules.module-unwanted-file]
extensions = ["rpm", "deb", "zip", "sql"]  # replaces the default list
```
"#,
    check: Check::Module(check_unwanted_file),
    min_odoo: None,
    max_odoo: None,
};

fn check_unwanted_file(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let configured = ctx
        .settings
        .config
        .rules()
        .module_unwanted_file
        .as_ref()
        .and_then(|c| c.extensions.clone());
    let extensions: Vec<String> = configured
        .unwrap_or_else(|| UNWANTED_EXTENSIONS.iter().map(|e| e.to_string()).collect())
        .into_iter()
        .map(|e| e.trim_start_matches('.').to_lowercase())
        .collect();
    for path in module_files(ctx) {
        let Some(extension) = path.extension().map(|e| e.to_string_lossy().to_lowercase()) else {
            continue;
        };
        if extensions.contains(&extension) {
            reporter.report(
                &UNWANTED_FILE,
                &path,
                1,
                format!("`.{extension}` files do not belong in an Odoo module; remove the file"),
            );
        }
    }
}

pub const LARGE_FILE: Rule = Rule {
    code: "ODOO003",
    name: "module-large-file",
    summary: "A file inside a module is larger than the limit (1 MiB by default).",
    doc: r#"
## What it does

Reports files in a module larger than a limit, 1024 KiB by default.

## Why is this bad?

Large files bloat the repository, every checkout and every Odoo instance
that installs the module, and git keeps them forever, even after removal.
Compress images, keep demo data small, and store large assets elsewhere.

## Options

```toml
[tool.odoo-lint.rules.module-large-file]
max-kib = 2048
```
"#,
    check: Check::Module(check_large_file),
    min_odoo: None,
    max_odoo: None,
};

fn check_large_file(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let max_kib = ctx
        .settings
        .config
        .rules()
        .module_large_file
        .as_ref()
        .and_then(|c| c.max_kib)
        .unwrap_or(DEFAULT_MAX_KIB);
    for path in module_files(ctx) {
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        let kib = metadata.len().div_ceil(1024);
        if kib > max_kib {
            reporter.report(
                &LARGE_FILE,
                &path,
                1,
                format!("File is {kib} KiB, larger than {max_kib} KiB"),
            );
        }
    }
}

// --- MOD007 unused-logger ---------------------------------------------------

/// Whether the Python file is part of an installable module, where OCA runs
/// its Python checks.
fn in_installable_module(ctx: &PythonContext) -> bool {
    ctx.module.is_some_and(is_installable)
}

/// Deletes whole lines `start..end` (offsets) including the line break.
fn delete_lines(source: &str, start: usize, end: usize) -> Edit {
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = source[end..].find('\n').map_or(source.len(), |i| end + i + 1);
    Edit::delete(line_start, line_end)
}

pub const UNUSED_LOGGER: Rule = Rule {
    code: "MOD007",
    name: "unused-logger",
    summary: "`_logger = logging.getLogger(__name__)` is never used.",
    doc: r#"
## What it does

Reports `_logger = logging.getLogger(__name__)` in a module file that never
uses `_logger`.

## Why is this bad?

Dead code that suggests logging which never happens.

## Fix safety

Safe: the assignment is removed.
"#,
    check: Check::Python(check_unused_logger),
    min_odoo: None,
    max_odoo: None,
};

fn is_get_logger(value: &Expr) -> bool {
    let Expr::Call(call) = value else { return false };
    let Expr::Attribute(func) = &*call.func else {
        return false;
    };
    func.attr.as_str() == "getLogger"
        && matches!(&*func.value, Expr::Name(n) if n.id.as_str() == "logging")
        && call.arguments.keywords.is_empty()
        && matches!(call.arguments.args.as_ref(), [Expr::Name(n)] if n.id.as_str() == "__name__")
}

fn check_unused_logger(ctx: &PythonContext, reporter: &mut Reporter) {
    if !in_installable_module(ctx) {
        return;
    }
    let mut assignment = None;
    let mut used = false;
    walk(ctx.parsed.suite(), |node, _| match node {
        Node::Stmt(Stmt::Assign(assign))
            if matches!(assign.targets.as_slice(), [Expr::Name(n)] if n.id.as_str() == "_logger")
                && is_get_logger(&assign.value) =>
        {
            assignment = Some(assign);
        }
        Node::Expr(Expr::Name(name)) if name.id.as_str() == "_logger" && name.ctx.is_load() => used = true,
        _ => {}
    });
    let Some(assign) = assignment else { return };
    if used {
        return;
    }
    reporter
        .report(
            &UNUSED_LOGGER,
            assign.start(),
            "Unused `_logger` is not allowed in Odoo models. Remove it if not used.",
        )
        .fix = Some(Fix::safe(
        "Remove the unused logger",
        vec![delete_lines(
            ctx.source,
            assign.start().to_usize(),
            assign.end().to_usize(),
        )],
    ));
}

// --- MOD008 use-header-comments ---------------------------------------------

/// Comments allowed in a file's header: shebang, encoding and tool pragmas.
const VALID_HEADER_COMMENTS: &[&str] = &[
    "# !",
    "#!",
    "coding:",
    "fixit:",
    "lint-ignore:",
    "lint-ignore=",
    "flake8:",
    "fmt:",
    "isort:",
    "noqa:",
    "nosec:",
    "oca-hooks:",
    "pylint:",
];

pub const USE_HEADER_COMMENTS: Rule = Rule {
    code: "MOD008",
    name: "use-header-comments",
    summary: "A Python file starts with comments such as a copyright header.",
    doc: r#"
## What it does

Reports comments at the top of a Python file, before its first statement,
other than a shebang, an encoding line and tool pragmas (`pylint:`,
`noqa:`, `fmt:`, `isort:`…).

## Why is this bad?

OCA keeps authorship and licence in the manifest and the README; per-file
copyright headers go stale and clutter every file.

## Fix safety

Unsafe: the comment lines are removed, which deletes their content.

## Opt-in

`ALL` does not include this check: many projects require copyright
headers. Enable it with `select = ["ALL", "MOD008"]` (or its name).
"#,
    check: Check::Python(check_header_comments),
    min_odoo: None,
    max_odoo: None,
};

fn check_header_comments(ctx: &PythonContext, reporter: &mut Reporter) {
    if !in_installable_module(ctx) {
        return;
    }
    let mut comments: Vec<(usize, usize, usize)> = Vec::new(); // line, start, end
    let mut offset = 0;
    let mut code = false;
    for (number, line) in ctx.source.split_inclusive('\n').enumerate() {
        let start = offset;
        offset += line.len();
        let content = line.trim_end_matches(['\n', '\r']);
        if content.trim_matches(' ').is_empty() {
            continue;
        }
        if !content.starts_with('#') {
            code = true;
            break;
        }
        if !VALID_HEADER_COMMENTS.iter().any(|token| content.contains(token)) {
            comments.push((number + 1, start, offset));
        }
    }
    let Some(&(_, last_start, _)) = comments.last() else {
        return;
    };
    if !code {
        // Only comments: OCA leaves such files alone.
        return;
    }
    let lines: Vec<String> = comments.iter().map(|(n, _, _)| n.to_string()).collect();
    let edits = comments
        .iter()
        .map(|&(_, start, end)| Edit::delete(start, end))
        .collect();
    let offset = TextSize::try_from(last_start).unwrap_or_default();
    reporter
        .report(
            &USE_HEADER_COMMENTS,
            offset,
            format!("Use of header comments in lines {}", lines.join(", ")),
        )
        .fix = Some(Fix::unsafe_("Remove the header comments", edits));
}

// --- MOD009 field-string-redundant --------------------------------------------

/// Whether a base of `class` is `Model`, `TransientModel` or
/// `AbstractModel` of `odoo.models` or `openerp.models`.
fn is_odoo_model(ctx: &PythonContext, class: &ruff_python_ast::StmtClassDef) -> bool {
    let bases = class.arguments.as_ref().map(|a| &a.args[..]).unwrap_or(&[]);
    bases.iter().any(|base| {
        let Some(dotted) = crate::semantic::dotted_name(base) else {
            return false;
        };
        let (root, rest) = dotted
            .split_once('.')
            .map_or((dotted.as_str(), ""), |(r, rest)| (r, rest));
        let Some(qualified) = ctx.semantic.resolve_key(root, class.start()) else {
            return false;
        };
        let full = if rest.is_empty() {
            qualified.to_string()
        } else {
            format!("{qualified}.{rest}")
        };
        ["odoo.models.", "openerp.models."].iter().any(|prefix| {
            full.strip_prefix(prefix)
                .is_some_and(|kind| matches!(kind, "Model" | "TransientModel" | "AbstractModel"))
        })
    })
}

/// Where `string` goes among the positional arguments of a field type.
fn string_position(field_type: &str) -> usize {
    match field_type {
        "Selection" | "Reference" | "Many2one" | "Many2many" => 1,
        "One2many" => 2,
        _ => 0,
    }
}

/// Python's `str.title()`.
fn python_title(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_cased = false;
    for c in text.chars() {
        let cased = c.is_lowercase() || c.is_uppercase();
        if cased {
            if previous_cased {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
        } else {
            out.push(c);
        }
        previous_cased = cased;
    }
    out
}

/// The label Odoo derives from a field name: `partner_id` -> `Partner`.
fn default_label(name: &str) -> String {
    let name = name.strip_suffix("_ids").unwrap_or(name);
    let name = name.strip_suffix("_id").unwrap_or(name);
    python_title(&name.replace('_', " "))
}

pub const FIELD_STRING_REDUNDANT: Rule = Rule {
    code: "MOD009",
    name: "field-string-redundant",
    summary: "A field's `string` is the label Odoo derives from its name anyway.",
    doc: r#"
## What it does

Reports a `string` (keyword or positional) on a field of an Odoo model that
equals the label Odoo derives from the field name: the name without `_id`
or `_ids`, with spaces for underscores, in title case (`partner_id` ->
`"Partner"`, `date_order` -> `"Date Order"`). Related fields are left alone,
since their label comes from the related field.

## Why is this bad?

It is noise, and one more string for translators.

## Fix safety

Safe when no positional argument follows the string: it is removed.
"#,
    check: Check::Python(check_field_string_redundant),
    min_odoo: None,
    max_odoo: None,
};

fn check_field_string_redundant(ctx: &PythonContext, reporter: &mut Reporter) {
    if !in_installable_module(ctx) {
        return;
    }
    for class in classes(ctx.parsed.suite()) {
        if !is_odoo_model(ctx, class) {
            continue;
        }
        for stmt in &class.body {
            let Stmt::Assign(assign) = stmt else { continue };
            let [Expr::Name(target)] = assign.targets.as_slice() else {
                continue;
            };
            let Expr::Call(call) = &*assign.value else { continue };
            let Expr::Attribute(func) = &*call.func else { continue };
            let Expr::Name(module) = &*func.value else { continue };
            let resolved = ctx
                .semantic
                .resolve_key(module.id.as_str(), call.start())
                .unwrap_or_default();
            if !(resolved.contains("odoo.fields") || resolved.contains("openerp.fields")) {
                continue;
            }
            let keywords = &call.arguments.keywords;
            if keywords
                .iter()
                .any(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "related"))
            {
                continue;
            }
            let args = &call.arguments.args;
            let keyword = keywords
                .iter()
                .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "string"));
            let position = string_position(func.attr.as_str());
            let (value, range, is_positional) = match keyword {
                Some(k) => (&k.value, k.range(), false),
                None => match args.get(position) {
                    Some(arg) => (arg, arg.range(), true),
                    None => continue,
                },
            };
            let Some(literal) = value.as_string_literal_expr() else {
                continue;
            };
            let text = source_of(ctx.source, value);
            // OCA compares plain one-part strings only.
            if literal.value.is_implicit_concatenated() || text.starts_with(|c: char| c.is_ascii_alphabetic()) {
                continue;
            }
            if literal.value.to_str() != default_label(target.id.as_str()) {
                continue;
            }
            let fix = (!is_positional || args.len() == position + 1).then(|| {
                let all: Vec<_> = args
                    .iter()
                    .map(Ranged::range)
                    .chain(keywords.iter().map(Ranged::range))
                    .collect();
                let index = all.iter().position(|r| *r == range).expect("an argument");
                let edit = if index > 0 {
                    Edit::delete(all[index - 1].end().to_usize(), range.end().to_usize())
                } else if all.len() > 1 {
                    Edit::delete(range.start().to_usize(), all[1].start().to_usize())
                } else {
                    // The only argument: empty the parentheses, trailing
                    // comma and line breaks included.
                    let arguments = call.arguments.range();
                    Edit::delete(arguments.start().to_usize() + 1, arguments.end().to_usize() - 1)
                };
                Fix::safe("Remove the redundant string", vec![edit])
            });
            reporter
                .report(
                    &FIELD_STRING_REDUNDANT,
                    assign.start(),
                    "The 'string' attribute is redundant and should be removed.",
                )
                .fix = fix;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_like_python() {
        let records = read_csv("id,name\n\na,b\n\nc,\"multi\nline\"\nd,e\n").unwrap();
        let lines: Vec<(String, usize)> = records.iter().map(|r| (r.fields[0].clone(), r.line)).collect();
        assert_eq!(
            lines,
            vec![("id".into(), 1), ("a".into(), 3), ("c".into(), 6), ("d".into(), 7)]
        );
        assert_eq!(read_csv("id,name\na,\"x\"\"y\"").unwrap()[1].fields, vec!["a", "x\"y"]);
        assert_eq!(read_csv("id\r\na\r\n").unwrap()[1].line, 2);
    }

    #[test]
    fn labels() {
        assert_eq!(default_label("partner_id"), "Partner");
        assert_eq!(default_label("tag_ids"), "Tag");
        assert_eq!(default_label("date_order"), "Date Order");
        assert_eq!(default_label("x2many_field"), "X2Many Field");
    }

    #[test]
    fn utf8_messages() {
        let bytes: Vec<u8> = [
            b"id,name
"
            .as_slice(),
            &[0xf1, b'x'],
        ]
        .concat();
        let error = std::str::from_utf8(&bytes).unwrap_err();
        assert_eq!(
            utf8_error_message(&bytes, &error),
            "'utf-8' codec can't decode byte 0xf1 in position 8: invalid continuation byte"
        );
    }
}
