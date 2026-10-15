//! The `obs` style of `odl readme`: one `README.md` per addon in the layout
//! of OBSNL/obs-modules' former `tools/obs_gen_addon_readme.py` (title,
//! badges, table of contents, sections, bug tracker, credits), byte for byte,
//! so that switching to `odl readme` changes no README.

use crate::manifest::Manifest;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Where the README points: `https://github.com/<org>/<repo>/tree/<branch>`.
#[derive(Debug, Clone)]
pub struct Repository {
    pub org: String,
    pub repo: String,
    pub branch: String,
}

/// Fragment name, section title (none: spliced in without one) and the level
/// of that title.
const FRAGMENTS: &[(&str, Option<&str>, usize)] = &[
    ("DESCRIPTION", None, 2),
    ("CONTEXT", Some("Use Cases / Context"), 2),
    ("INSTALL", Some("Installation"), 2),
    ("CONFIGURE", Some("Configuration"), 2),
    ("USAGE", Some("Usage"), 2),
    ("DEVELOP", Some("Development"), 2),
    ("ROADMAP", Some("Known issues / Roadmap"), 2),
    ("HISTORY", Some("Changelog"), 2),
    ("CONTRIBUTORS", Some("Contributors"), 3),
    ("CREDITS", Some("Other credits"), 3),
];

/// Fragments rendered under "Credits" rather than as sections of their own.
const CREDITS_FRAGMENTS: &[&str] = &["CONTRIBUTORS", "CREDITS"];

fn license_badge(license: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match license {
        "AGPL-3" => (
            "https://img.shields.io/badge/license-AGPL--3-blue.png",
            "https://www.gnu.org/licenses/agpl-3.0-standalone.html",
            "License: AGPL-3",
        ),
        "LGPL-3" => (
            "https://img.shields.io/badge/license-LGPL--3-blue.png",
            "https://www.gnu.org/licenses/lgpl-3.0-standalone.html",
            "License: LGPL-3",
        ),
        "GPL-3" => (
            "https://img.shields.io/badge/license-GPL--3-blue.png",
            "https://www.gnu.org/licenses/gpl-3.0-standalone.html",
            "License: GPL-3",
        ),
        "OPL-1" => (
            "https://img.shields.io/badge/license-OPL--1-blue.png",
            "https://www.odoo.com/documentation/master/legal/licenses.html",
            "License: OPL-1",
        ),
        _ => return None,
    })
}

fn status_badge(status: &str) -> Option<(&'static str, &'static str)> {
    Some(match status {
        "mature" => ("https://img.shields.io/badge/maturity-Mature-brightgreen.png", "Mature"),
        "production/stable" => (
            "https://img.shields.io/badge/maturity-Production%2FStable-green.png",
            "Production/Stable",
        ),
        "beta" => ("https://img.shields.io/badge/maturity-Beta-yellow.png", "Beta"),
        "alpha" => ("https://img.shields.io/badge/maturity-Alpha-red.png", "Alpha"),
        _ => return None,
    })
}

static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\s*)(`{3,}|~{3,})").unwrap());
static ATX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(#{1,6})(\s)").unwrap());
static SETEXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(=+|-+)\s*$").unwrap());
static LIST_OR_QUOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:[-*+>]\s|\d+[.)]\s)").unwrap());
static IMAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(!\[[^\]]*\]\()([^)\s]+)").unwrap());
static LEADING_RELATIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:\.\./|\./)+").unwrap());
static NOT_SLUG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w\s-]").unwrap());

/// Python's `str.strip()`.
fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace())
}

/// The manifest files and fragments, path-sensitive: renaming or deleting
/// a fragment changes the digest.
fn source_digest(addon: &Path) -> String {
    let mut digest = Sha256::new();
    let mut paths: Vec<PathBuf> = ["__manifest__.py", "__openerp__.py"]
        .iter()
        .map(|n| addon.join(n))
        .filter(|p| p.is_file())
        .collect();
    let fragments = addon.join("readme");
    if let Ok(entries) = std::fs::read_dir(&fragments) {
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        paths.extend(files);
    }
    for path in paths {
        let relative = path.strip_prefix(addon).unwrap_or(&path);
        let relative: Vec<String> = relative.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        digest.update(relative.join("/").as_bytes());
        digest.update(std::fs::read(&path).unwrap_or_default());
    }
    format!("sha256:{:x}", digest.finalize())
}

/// Whether `line` can carry a setext underline: not blank, not a heading,
/// not a list item or a quote.
fn is_setext_title(line: &str) -> bool {
    let stripped = py_strip(line);
    !stripped.is_empty() && !ATX.is_match(line) && !LIST_OR_QUOTE.is_match(stripped)
}

/// Pushes a fragment's own headings below its section title, outside fenced
/// code, and rewrites setext headings as ATX ones.
fn shift_headings(text: &str, by: usize) -> String {
    if by == 0 {
        return text.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    let mut fence: Option<String> = None;
    for line in py_splitlines(text) {
        if let Some(captures) = FENCE.captures(line) {
            let marker = &captures[2];
            match &fence {
                None => fence = Some(marker[..1].repeat(3)),
                Some(open) if marker.starts_with(open.as_str()) => fence = None,
                Some(_) => {}
            }
            out.push(line.to_string());
            continue;
        }
        if fence.is_some() {
            out.push(line.to_string());
            continue;
        }
        if let Some(captures) = ATX.captures(line) {
            let hashes = captures[1].len();
            let level = (hashes + by).min(6);
            out.push(format!("{}{}", "#".repeat(level), &line[hashes..]));
            continue;
        }
        if let Some(captures) = SETEXT.captures(line) {
            if let Some(title) = out.pop_if(|previous| is_setext_title(previous)) {
                let base = if captures[1].starts_with('=') { 1 } else { 2 };
                let level = (base + by).min(6);
                out.push(format!("{} {}", "#".repeat(level), py_strip(&title)));
                continue;
            }
        }
        out.push(line.to_string());
    }
    out.join("\n")
}

/// Python's `str.splitlines()` for the line breaks a fragment has.
fn py_splitlines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
}

/// Relative image paths point at raw.githubusercontent, so they work wherever
/// the README is rendered.
fn absolutize_images(text: &str, module_url: &str) -> String {
    IMAGE
        .replace_all(text, |captures: &regex::Captures| {
            let path = &captures[2];
            if path.starts_with("http://")
                || path.starts_with("https://")
                || path.starts_with('#')
                || path.starts_with("data:")
            {
                captures[0].to_string()
            } else {
                format!("{}{module_url}{}", &captures[1], LEADING_RELATIVE.replace(path, ""))
            }
        })
        .into_owned()
}

/// GitHub's heading anchor.
fn slugify(title: &str) -> String {
    let slug = py_strip(title).to_lowercase();
    let slug = NOT_SLUG.replace_all(&slug, "");
    slug.chars().map(|c| if c.is_whitespace() { '-' } else { c }).collect()
}

/// A fragment's text, stripped; `None` when missing or empty. An `.rst`
/// fragment is an error: this style writes Markdown.
fn read_fragment(addon: &Path, name: &str) -> Result<Option<String>, String> {
    let rst = addon.join("readme").join(format!("{name}.rst"));
    if rst.is_file() {
        return Err(format!(
            "{}: .rst fragments are not supported; this generator emits Markdown. Convert it to {name}.md.",
            rst.display()
        ));
    }
    let Ok(text) = std::fs::read_to_string(addon.join("readme").join(format!("{name}.md"))) else {
        return Ok(None);
    };
    let text = py_strip(&text);
    Ok((!text.is_empty()).then(|| text.to_string()))
}

/// The README of the addon at `addon`, or `None` when it has no manifest or
/// no `readme/DESCRIPTION.md` (a hand-written README stays as it is).
pub fn readme(addon: &Path, repository: &Repository) -> Result<Option<String>, String> {
    let Some(source) = ["__manifest__.py", "__openerp__.py"]
        .iter()
        .find_map(|n| std::fs::read_to_string(addon.join(n)).ok())
    else {
        return Ok(None);
    };
    let Some(manifest) = Manifest::parse(&source) else {
        return Ok(None);
    };
    let Some(description) = read_fragment(addon, "DESCRIPTION")? else {
        return Ok(None);
    };
    let name = addon
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let Repository { org, repo, branch } = repository;
    let module_url = format!("https://raw.githubusercontent.com/{org}/{repo}/{branch}/{name}/");
    let mut fragments: Vec<(&str, String)> = Vec::new();
    for (fragment, _, _) in FRAGMENTS {
        let text = if *fragment == "DESCRIPTION" {
            Some(description.clone())
        } else {
            read_fragment(addon, fragment)?
        };
        if let Some(text) = text {
            fragments.push((fragment, absolutize_images(&text, &module_url)));
        }
    }
    let get = |n: &str| fragments.iter().find(|(f, _)| *f == n).map(|(_, t)| t.as_str());
    let addon_url = format!("https://github.com/{org}/{repo}/tree/{branch}/{name}");
    let repo_url = format!("https://github.com/{org}/{repo}");

    let mut badges = Vec::new();
    let status = manifest.get_str("development_status").unwrap_or("Beta").to_lowercase();
    if let Some((image, alt)) = status_badge(&status) {
        badges.push(format!("![{alt}]({image})"));
    }
    if let Some((image, target, alt)) = manifest.get_str("license").and_then(license_badge) {
        badges.push(format!("[![{alt}]({image})]({target})"));
    }
    badges.push(format!(
        "[![{org}/{repo}](https://img.shields.io/badge/github-{}%2F{}-lightgray.png?logo=github)]({addon_url})",
        org.replace('-', "--"),
        repo.replace('-', "--")
    ));

    let digest = source_digest(addon);
    let mut lines: Vec<String> = vec![
        "<!-- This file is generated by obs-gen-addon-readme; do not edit it.".into(),
        "     Edit the fragments under readme/ instead.".into(),
        format!("     source digest: {digest} -->"),
        String::new(),
        format!("# {}", manifest.get_str("name").unwrap_or(&name)),
        String::new(),
        badges.join(" "),
        String::new(),
        get("DESCRIPTION").unwrap_or_default().to_string(),
        String::new(),
    ];
    if status == "alpha" {
        lines.extend(
            [
                "> [!WARNING]",
                "> This is an alpha version: the data model and the design can change",
                "> at any time without warning. For development or testing only.",
                "",
            ]
            .map(String::from),
        );
    }
    let sections: Vec<(&str, &str, usize)> = FRAGMENTS
        .iter()
        .filter_map(|(f, title, level)| title.map(|t| (*f, t, *level)))
        .filter(|(f, _, _)| get(f).is_some() && !CREDITS_FRAGMENTS.contains(f))
        .collect();
    let author = manifest.get_str("author").unwrap_or_default();
    let has_credits = CREDITS_FRAGMENTS.iter().any(|f| get(f).is_some()) || !author.is_empty();
    lines.push("**Table of contents**".into());
    lines.push(String::new());
    for (_, title, _) in &sections {
        lines.push(format!("- [{title}](#{})", slugify(title)));
    }
    lines.push("- [Bug Tracker](#bug-tracker)".into());
    if has_credits {
        lines.push("- [Credits](#credits)".into());
    }
    lines.push(String::new());
    for (fragment, title, level) in &sections {
        lines.push(format!("{} {title}", "#".repeat(*level)));
        lines.push(String::new());
        lines.push(shift_headings(get(fragment).unwrap_or_default(), *level));
        lines.push(String::new());
    }
    lines.extend([
        "## Bug Tracker".to_string(),
        String::new(),
        format!("Bugs are tracked on [GitHub Issues]({repo_url}/issues). In case of trouble,"),
        "please check there if your issue has already been reported. If you spotted it".into(),
        "first, help us to smash it by providing a detailed and welcomed feedback.".into(),
        String::new(),
        "Do not contact contributors directly about support or help with technical".into(),
        "issues.".into(),
        String::new(),
    ]);
    if has_credits {
        lines.push("## Credits".into());
        lines.push(String::new());
        let authors: Vec<&str> = author.split(',').map(py_strip).filter(|a| !a.is_empty()).collect();
        if !authors.is_empty() {
            lines.push("### Authors".into());
            lines.push(String::new());
            lines.extend(authors.iter().map(|a| format!("- {a}")));
            lines.push(String::new());
        }
        for fragment in CREDITS_FRAGMENTS {
            let Some(text) = get(fragment) else { continue };
            let title = FRAGMENTS
                .iter()
                .find(|(f, _, _)| f == fragment)
                .and_then(|(_, t, _)| *t)
                .unwrap_or_default();
            lines.push(format!("### {title}"));
            lines.push(String::new());
            lines.push(shift_headings(text, 3));
            lines.push(String::new());
        }
        let maintainers: Vec<&str> = manifest
            .get("maintainers")
            .map(|m| {
                crate::rules::manifest::string_elements(m)
                    .into_iter()
                    .map(|(s, _)| s)
                    .collect()
            })
            .unwrap_or_default();
        if !maintainers.is_empty() {
            let plural = if maintainers.len() > 1 { "s" } else { "" };
            lines.push("### Maintainers".into());
            lines.push(String::new());
            lines.push(format!("Current maintainer{plural}:"));
            lines.push(String::new());
            lines.push(
                maintainers
                    .iter()
                    .map(|m| format!("[![{m}](https://github.com/{m}.png?size=40px)](https://github.com/{m})"))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            lines.push(String::new());
        }
    }
    lines.push(format!(
        "This module is part of the [{org}/{repo}]({addon_url}) project on"
    ));
    lines.push("GitHub. You are welcome to contribute.".into());
    lines.push(String::new());
    Ok(Some(lines.join("\n")))
}
