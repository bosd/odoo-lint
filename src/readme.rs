//! `odl readme`: a module's documentation from its `readme/` fragments, as
//! the OBS client template builds it: the Markdown fragments joined into
//! `README.md`, and that rendered to `static/description/index.html` (the
//! page Odoo shows for the module). Byte for byte the output of the
//! template's Python generator, in milliseconds instead of seconds.

use comrak::{markdown_to_html, Options};
use std::path::{Path, PathBuf};

/// The sections of `README.md`, in order, and the fragment of each.
const SECTIONS: &[(&str, &str)] = &[
    ("Introduction", "DESCRIPTION.md"),
    ("Features", "FEATURES.md"),
    ("Installation", "INSTALL.md"),
    ("Configuration", "CONFIGURE.md"),
    ("Usage", "USAGE.md"),
    ("Context", "CONTEXT.md"),
    ("History", "HISTORY.md"),
    ("Roadmap", "ROADMAP.md"),
    ("Contributors", "CONTRIBUTORS.md"),
    ("Credits", "CREDITS.md"),
];

/// The page around the rendered README (the template's `html_template.html`).
const PAGE: &str = include_str!("readme/page.html");

/// Folders not to look into for modules.
const SKIPPED_DIRS: &[&str] = &[
    ".git",
    ".venv",
    "venv",
    "node_modules",
    "__pycache__",
    "target",
    "build",
    "dist",
];

/// Reads a text file as Python's `open()` does: universal newlines.
fn read_text(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// `README.md` from the fragments in `readme/`: a `## Title` per non-empty
/// fragment, separated by two blank lines, after one blank line.
pub fn aggregate(readme: &Path) -> String {
    let mut out = String::from("\n");
    let mut first = true;
    for (title, file) in SECTIONS {
        let content = read_text(&readme.join(file)).unwrap_or_default();
        let content = content.trim();
        if content.is_empty() {
            continue;
        }
        out.push('\n');
        if !first {
            out.push('\n');
        }
        first = false;
        out.push_str(&format!("## {title}\n{content}\n"));
    }
    out
}

fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.tagfilter = false;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.superscript = true;
    options.extension.footnotes = true;
    options.extension.description_lists = true;
    options.parse.smart = true;
    options.render.hardbreaks = false;
    options.render.r#unsafe = true;
    options.render.github_pre_lang = true;
    options
}

/// Mermaid code blocks as the `<div class="mermaid">` mermaid.js renders.
fn mermaid(html: &str) -> String {
    const OPEN: &str = "<pre lang=\"mermaid\"><code>";
    const CLOSE: &str = "</code></pre>";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find(OPEN) {
        let Some(end) = rest[start + OPEN.len()..].find(CLOSE) else {
            break;
        };
        let code = &rest[start + OPEN.len()..start + OPEN.len() + end];
        let code = code
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&");
        out.push_str(&rest[..start]);
        out.push_str(&format!("<div class=\"mermaid\">\n{code}</div>"));
        rest = &rest[start + OPEN.len() + end + CLOSE.len()..];
    }
    out.push_str(rest);
    out
}

/// `static/description/index.html` for a README.
pub fn page(readme: &str, module: &str) -> String {
    let content = mermaid(&markdown_to_html(readme, &options()));
    let page = PAGE
        .replace("{{ module_name }}", module)
        .replace("{{ content | safe }}", &content);
    // Jinja drops the template's final newline.
    page.strip_suffix('\n').unwrap_or(&page).to_string()
}

/// The files `odl readme` writes for a module, and their contents.
pub fn outputs(module: &Path) -> Option<Vec<(PathBuf, String)>> {
    let readme = module.join("readme");
    if !readme.is_dir() {
        return None;
    }
    let name = module.canonicalize().ok()?.file_name()?.to_string_lossy().into_owned();
    let markdown = aggregate(&readme);
    let html = page(&markdown, &name);
    Some(vec![
        (module.join("README.md"), markdown),
        (module.join("static").join("description").join("index.html"), html),
    ])
}

fn is_module(dir: &Path) -> bool {
    dir.join("__manifest__.py").is_file()
}

/// The modules under `paths`: folders with a manifest, and the module of
/// each file given (as pre-commit passes the changed files).
pub fn modules(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for path in paths {
        if path.is_file() || !path.exists() {
            if let Some(module) = path.ancestors().skip(1).find(|d| is_module(d)) {
                found.push(module.to_path_buf());
            }
            continue;
        }
        let walker = walkdir::WalkDir::new(path).into_iter().filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !(e.file_type().is_dir()
                && e.depth() > 0
                && (name.starts_with('.') || SKIPPED_DIRS.contains(&name.as_ref())))
        });
        for entry in walker.flatten() {
            if entry.file_type().is_dir() && is_module(entry.path()) {
                found.push(entry.path().to_path_buf());
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// What `odl readme` did to one file.
pub struct Change {
    pub path: PathBuf,
    /// Whether the file was written (`false` in check mode).
    pub written: bool,
}

/// Generates the documentation of the modules under `paths`. With `check`,
/// writes nothing and only reports the files that are out of date.
pub fn run(paths: &[PathBuf], check: bool) -> std::io::Result<Vec<Change>> {
    let mut changes = Vec::new();
    for module in modules(paths) {
        let Some(files) = outputs(&module) else { continue };
        for (path, content) in files {
            if std::fs::read(&path).ok().as_deref() == Some(content.as_bytes()) {
                continue;
            }
            if !check {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, &content)?;
            }
            changes.push(Change { path, written: !check });
        }
    }
    Ok(changes)
}
