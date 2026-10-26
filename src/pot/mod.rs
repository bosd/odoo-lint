//! `odl pot`: a module's translation template (`i18n/<module>.pot`), built
//! from its source the way Odoo's export writes it (`TranslationModuleReader`
//! and `PoFileWriter` in odoo/tools/translate.py), without a database.
//!
//! It holds the terms of the code ([`code`]) and those Odoo creates for the
//! module's models ([`models`]): descriptions, field labels, selections and
//! constraint messages, with values from the module and its dependencies.

mod code;
mod markup;
mod merge;
mod models;
mod records;

use crate::index::ModuleIndex;
use crate::manifest::Manifest;
use crate::odoo_version::OdooVersion;
use crate::po::{PoEntry, PoFile};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

pub use code::{code_terms, javascript_terms, python_terms, qweb_terms, spreadsheet_terms};
pub use merge::{merge, Obsolete};
pub use models::model_terms;

/// The extracted comment Odoo adds to the terms of Python code.
const PYTHON_COMMENT: &str = "odoo-python";
/// The extracted comment of JavaScript and QWeb terms.
const JAVASCRIPT_COMMENT: &str = "odoo-javascript";

/// Which extractors Odoo uses for code: its own from 19.0 on
/// (odoo/tools/babel/), Babel's before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extractor {
    /// Odoo's extractors (19.0 and later).
    Odoo,
    /// Babel's extractors, as Odoo 18.0 and older use them.
    Babel,
}

impl Extractor {
    /// The extractors Odoo `version` uses.
    pub fn for_version(version: OdooVersion) -> Self {
        if version.major >= 19 {
            Self::Odoo
        } else {
            Self::Babel
        }
    }
}

/// One translatable term and where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    /// The text to translate (the `msgid`).
    pub source: String,
    /// `(type, name, res_id)` as Odoo sorts them: `("code", path, "0")`.
    pub reference: (String, String, String),
    /// The extracted comment Odoo adds, such as `odoo-python`.
    pub comment: Option<&'static str>,
}

impl Term {
    fn code(source: String, path: &str, comment: &'static str) -> Self {
        Self {
            source,
            reference: ("code".into(), path.into(), "0".into()),
            comment: Some(comment),
        }
    }
}

/// Whether Odoo exports a term: it needs a letter, so `:` or `.00` are left
/// out. Python's `isalpha()` is a letter category (`\p{L}`), not Unicode's
/// wider Alphabetic: `Ⅳ` is no letter.
fn exported(term: &str) -> bool {
    static LETTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{L}").unwrap());
    LETTER.is_match(term)
}

/// The template of `module` as Odoo's `PoFileWriter` writes it, without the
/// creation dates (OCA's templates leave them out to keep diffs small).
pub fn write(module: &str, version: OdooVersion, terms: &[Term]) -> String {
    // Per msgid: its references and extracted comments.
    type Group<'t> = (BTreeSet<&'t (String, String, String)>, BTreeSet<&'static str>);
    let mut grouped: BTreeMap<&str, Group> = BTreeMap::new();
    for term in terms.iter().filter(|t| exported(&t.source)) {
        let (references, comments) = grouped.entry(&term.source).or_default();
        references.insert(&term.reference);
        comments.extend(term.comment);
    }
    let entries = grouped
        .into_iter()
        .map(|(source, (references, comments))| {
            let mut comment = format!("module: {module}");
            for extra in comments {
                comment.push('\n');
                comment.push_str(extra);
            }
            let mut occurrences: Vec<(String, String)> = Vec::new();
            for (kind, name, res_id) in references {
                let occurrence = if kind == "code" {
                    (format!("code:{name}"), "0".to_string())
                } else {
                    (format!("{kind}:{name}:{res_id}"), String::new())
                };
                if !occurrences.contains(&occurrence) {
                    occurrences.push(occurrence);
                }
            }
            PoEntry {
                msgid: source.to_string(),
                comment,
                occurrences,
                ..PoEntry::default()
            }
        })
        .collect();
    let metadata = [
        ("Project-Id-Version", format!("Odoo Server {version}")),
        ("Report-Msgid-Bugs-To", String::new()),
        ("Last-Translator", String::new()),
        ("Language-Team", String::new()),
        ("MIME-Version", "1.0".into()),
        ("Content-Type", "text/plain; charset=UTF-8".into()),
        ("Content-Transfer-Encoding", String::new()),
        ("Plural-Forms", String::new()),
    ];
    PoFile {
        header: format!(
            "Translation of Odoo Server.\nThis file contains the translation of the following modules:\n\t* {module}\n"
        ),
        metadata: metadata.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        metadata_is_fuzzy: false,
        entries,
    }
    .to_po_string()
}

/// The Odoo series of a module: the first two parts of a five-part manifest
/// version, else `default`.
pub fn module_version(dir: &Path, default: OdooVersion) -> OdooVersion {
    std::fs::read_to_string(dir.join("__manifest__.py"))
        .ok()
        .and_then(|source| {
            let manifest = Manifest::parse(&source)?;
            let version = manifest.get_str("version")?;
            let parts: Vec<&str> = version.split('.').collect();
            (parts.len() == 5).then(|| format!("{}.{}", parts[0], parts[1]).parse().ok())?
        })
        .unwrap_or(default)
}

/// What is installed with a module: its dependencies, the modules of
/// `installed` folders (OCA exports from a database with every module of the
/// repository installed), and what Odoo then installs automatically.
pub struct Installation<'p> {
    pub addons_path: &'p [PathBuf],
    pub installed: &'p [PathBuf],
    /// Odoo's `--skip-auto-install` when installing the modules, as in
    /// OCA's CI: only the modules installed with a new database (`base`,
    /// `web` and what they install automatically) come without asking. OCA
    /// creates that database without the option: its templates match these
    /// automatic modules, not `base` alone.
    pub skip_auto_install: bool,
    /// Whether demo data is installed; `None` for Odoo's default: with demo
    /// data up to 18.0, without from 19.0.
    pub demo: Option<bool>,
}

/// A module Odoo installs automatically once `triggers` are installed.
struct AutoInstall {
    name: String,
    path: PathBuf,
    triggers: Vec<String>,
}

impl Installation<'_> {
    /// The modules of the addons path that install automatically, read from
    /// their manifests only.
    fn auto_installs(&self) -> Vec<AutoInstall> {
        let mut found = Vec::new();
        for dir in self.addons_path {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(source) = std::fs::read_to_string(path.join("__manifest__.py")) else {
                    continue;
                };
                let Some(manifest) = Manifest::parse(&source) else {
                    continue;
                };
                let list = |key: &str| -> Vec<String> {
                    match manifest.get(key) {
                        Some(ruff_python_ast::Expr::List(list)) => list
                            .elts
                            .iter()
                            .filter_map(|e| e.as_string_literal_expr().map(|s| s.value.to_string()))
                            .collect(),
                        _ => Vec::new(),
                    }
                };
                // A localization installs for companies of its countries only.
                if manifest.get("countries").is_some() {
                    continue;
                }
                let triggers = match manifest.get("auto_install") {
                    Some(ruff_python_ast::Expr::BooleanLiteral(b)) if b.value => list("depends"),
                    Some(ruff_python_ast::Expr::List(_)) => list("auto_install"),
                    _ => continue,
                };
                let name = entry.file_name().to_string_lossy().into_owned();
                found.push(AutoInstall { name, path, triggers });
            }
        }
        found
    }

    /// The modules installed with the module in `dir`, in install order,
    /// and whether all its dependencies were found.
    fn modules(&self, dir: &Path, auto_installs: &[AutoInstall]) -> (Vec<Arc<ModuleIndex>>, bool) {
        type Modules = BTreeMap<String, Arc<ModuleIndex>>;
        let add = |root: &Path, found: &mut Modules| {
            for module in crate::index::dependencies(root, self.addons_path, false).unwrap_or_default() {
                found.entry(module.name.clone()).or_insert(module);
            }
        };
        // Installs the automatic modules whose triggers are installed, until
        // none is left.
        let auto_install = |found: &mut Modules| {
            // A module that cannot be read must not be retried forever.
            let mut unreadable: BTreeSet<&str> = BTreeSet::new();
            loop {
                let ready: Vec<&AutoInstall> = auto_installs
                    .iter()
                    .filter(|a| !found.contains_key(&a.name) && !unreadable.contains(a.name.as_str()))
                    .filter(|a| a.triggers.iter().all(|t| found.contains_key(t)))
                    .collect();
                if ready.is_empty() {
                    break;
                }
                for auto in ready {
                    add(&auto.path, found);
                    if !found.contains_key(&auto.name) {
                        unreadable.insert(&auto.name);
                    }
                }
            }
        };
        let complete = crate::index::dependencies(dir, self.addons_path, true).is_some();
        let mut found = Modules::new();
        add(dir, &mut found);
        // A new database: `base` and `web` with their automatic modules.
        let mut initial: Modules = found
            .iter()
            .filter(|(name, _)| matches!(name.as_str(), "base" | "web"))
            .map(|(name, module)| (name.clone(), Arc::clone(module)))
            .collect();
        auto_install(&mut initial);
        found.extend(initial.iter().map(|(name, module)| (name.clone(), Arc::clone(module))));
        for module in crate::readme::modules(self.installed) {
            add(&module, &mut found);
        }
        if !self.skip_auto_install {
            auto_install(&mut found);
        }
        // OCA installs the dependencies first, then the repository's modules.
        let mut addons: BTreeSet<String> = crate::readme::modules(self.installed)
            .iter()
            .filter_map(|m| m.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        addons.extend(dir.file_name().map(|n| n.to_string_lossy().into_owned()));
        let phase = |name: &str| {
            if initial.contains_key(name) {
                0
            } else if addons.contains(name) {
                2
            } else {
                1
            }
        };
        (load_order(found.into_values().collect(), phase), complete)
    }
}

/// Modules in the order Odoo installs them: by `phase` (each installation
/// step), then by depth (the longest path to `base` through dependencies),
/// then by name.
fn load_order(modules: Vec<Arc<ModuleIndex>>, phase: impl Fn(&str) -> u8) -> Vec<Arc<ModuleIndex>> {
    fn depth(name: &str, modules: &BTreeMap<&str, &ModuleIndex>, memo: &mut BTreeMap<String, usize>) -> usize {
        if let Some(found) = memo.get(name) {
            return *found;
        }
        // A cycle counts as no deeper.
        memo.insert(name.to_string(), 0);
        let found = match modules.get(name) {
            _ if name == "base" => 0,
            Some(module) => {
                1 + module
                    .depends
                    .iter()
                    .map(|dependency| depth(dependency, modules, memo))
                    .max()
                    .unwrap_or(0)
            }
            None => 0,
        };
        memo.insert(name.to_string(), found);
        found
    }
    let by_name: BTreeMap<&str, &ModuleIndex> = modules.iter().map(|m| (m.name.as_str(), m.as_ref())).collect();
    let mut memo = BTreeMap::new();
    let mut keyed: Vec<((u8, usize), Arc<ModuleIndex>)> = modules
        .iter()
        .map(|module| {
            let key = (phase(&module.name), depth(&module.name, &by_name, &mut memo));
            (key, Arc::clone(module))
        })
        .collect();
    keyed.sort_by(|(a_key, a), (b_key, b)| a_key.cmp(b_key).then_with(|| a.name.cmp(&b.name)));
    keyed.into_iter().map(|(_, module)| module).collect()
}

/// A generated template.
pub struct Template {
    pub content: String,
    /// Whether dependencies of the module were not found on the addons
    /// path, so labels they define may be missing or wrong.
    pub incomplete: bool,
    /// Whether the module has no terms: Odoo then exports nothing.
    pub empty: bool,
}

/// The template of the module in `dir`, installed as `installation` says.
pub fn generate(dir: &Path, default_version: OdooVersion, installation: &Installation) -> Option<Template> {
    generate_with(dir, default_version, installation, &installation.auto_installs())
}

fn generate_with(
    dir: &Path,
    default_version: OdooVersion,
    installation: &Installation,
    auto_installs: &[AutoInstall],
) -> Option<Template> {
    let module = dir.file_name()?.to_str()?;
    let version = module_version(dir, default_version);
    let mut terms = code_terms(dir, module, Extractor::for_version(version));
    let (modules, complete) = installation.modules(dir, auto_installs);
    // Odoo gives the module XML ids for what exists when it loads; the
    // labels are those after every installed module loaded.
    let loaded = modules
        .iter()
        .position(|m| m.name == module)
        .map_or(modules.len(), |at| at + 1);
    let at_load = crate::index::closure_of(modules[..loaded].to_vec());
    let installed = crate::index::closure_of(modules.clone());
    terms.extend(model_terms(module, &at_load, &installed, version));
    let models = models::Models::new(&installed, version);
    let demo = installation.demo.unwrap_or(version.major < 19);
    terms.extend(records::record_terms(module, &modules, &models, demo));
    Some(Template {
        content: write(module, version, &terms),
        incomplete: !complete,
        empty: terms.is_empty(),
    })
}

/// What `odl pot` did to one template.
pub struct Change {
    pub path: PathBuf,
    /// Whether the file was written (`false` in check mode).
    pub written: bool,
    /// See [`Template::incomplete`] (for a template).
    pub incomplete: bool,
    /// Why the file was left as it is: a `.po` file that does not parse.
    pub skipped: Option<String>,
}

/// Writes `i18n/<module>.pot` for the modules under `paths`, or into
/// `output_dir` when given, and with `merge_po` brings the module's `.po`
/// files in line with it; in check mode only reports the outdated files.
///
/// # Errors
///
/// When a template or its folder cannot be written.
pub fn run(
    paths: &[PathBuf],
    check: bool,
    output_dir: Option<&Path>,
    default_version: OdooVersion,
    installation: &Installation,
    merge_po: Option<Obsolete>,
) -> std::io::Result<Vec<Change>> {
    let auto_installs = installation.auto_installs();
    let mut changes = Vec::new();
    for dir in crate::readme::modules(paths) {
        let Some(module) = dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        let Some(Template {
            content,
            incomplete,
            empty,
        }) = generate_with(&dir, default_version, installation, &auto_installs)
        else {
            continue;
        };
        let path = match output_dir {
            Some(out) => out.join(format!("{module}.pot")),
            None => dir.join("i18n").join(format!("{module}.pot")),
        };
        let mut update = |path: PathBuf, content: &str, incomplete: bool| -> std::io::Result<()> {
            if std::fs::read_to_string(&path).is_ok_and(|current| current == content) {
                return Ok(());
            }
            if !check {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, content)?;
            }
            changes.push(Change {
                path,
                written: !check,
                incomplete,
                skipped: None,
            });
            Ok(())
        };
        // No template is created for nothing; one that exists is updated.
        if !empty || path.exists() {
            update(path, &content, incomplete)?;
        }
        if let (Some(obsolete), None) = (merge_po, output_dir) {
            let Ok(template) = PoFile::parse(&content) else {
                continue;
            };
            let mut skipped = Vec::new();
            for po_path in po_files(&dir.join("i18n")) {
                let source = std::fs::read_to_string(&po_path)?;
                match PoFile::parse(&source) {
                    Ok(po) => update(po_path, &merge(&po, &template, obsolete).to_po_string(), false)?,
                    // Left as it is: rewriting it could lose what did not parse.
                    Err(error) => skipped.push(Change {
                        path: po_path,
                        written: false,
                        incomplete: false,
                        skipped: Some(format!("line {}: {}", error.line, error.message)),
                    }),
                }
            }
            changes.extend(skipped);
        }
    }
    Ok(changes)
}

/// The `.po` files of an `i18n` folder, sorted.
fn po_files(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "po"))
        .collect();
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_terms_with_a_letter_as_python_sees_it() {
        assert!(exported("Ok"));
        assert!(exported("日本"));
        assert!(!exported(" .00 "));
        assert!(!exported("Ⅳ"));
    }

    #[test]
    fn write_groups_by_msgid_and_sorts_like_odoo() {
        let terms = vec![
            Term::code("Zeta".into(), "addons/acme/models/b.py", PYTHON_COMMENT),
            Term::code("Alpha".into(), "addons/acme/models/b.py", PYTHON_COMMENT),
            Term::code("Alpha".into(), "addons/acme/models/a.py", PYTHON_COMMENT),
            Term::code("Alpha".into(), "addons/acme/models/a.py", PYTHON_COMMENT),
            Term::code(".00".into(), "addons/acme/models/a.py", PYTHON_COMMENT),
        ];
        assert_eq!(
            write("acme", OdooVersion::new(19, 0), &terms),
            r#"# Translation of Odoo Server.
# This file contains the translation of the following modules:
# 	* acme
#
msgid ""
msgstr ""
"Project-Id-Version: Odoo Server 19.0\n"
"Report-Msgid-Bugs-To: \n"
"Last-Translator: \n"
"Language-Team: \n"
"MIME-Version: 1.0\n"
"Content-Type: text/plain; charset=UTF-8\n"
"Content-Transfer-Encoding: \n"
"Plural-Forms: \n"

#. module: acme
#. odoo-python
#: code:addons/acme/models/a.py:0 code:addons/acme/models/b.py:0
msgid "Alpha"
msgstr ""

#. module: acme
#. odoo-python
#: code:addons/acme/models/b.py:0
msgid "Zeta"
msgstr ""
"#
        );
    }
}
