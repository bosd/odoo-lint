//! `odl requirements`: the `requirements.txt` of an addons folder, from the
//! Python `external_dependencies` of its installable modules, as OCA's
//! `oca-gen-external-dependencies` hook writes it, without building each
//! module's package first: whool's names for common imports (`dateutil` is
//! `python-dateutil`) and its `external_dependencies_override` apply.

use crate::manifest::Manifest;
use crate::pyliteral::is_truthy;
use regex::Regex;
use ruff_python_ast::Expr;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const HEADER: &str = "# generated from manifests external_dependencies\n";

/// Odoo itself and Odoo addons, which are not Python requirements of the
/// folder (by canonical name).
static ODOO_PACKAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(odoo$|odoo\d*-addon-)").unwrap());

/// Import names that manifests list in `external_dependencies`, and the
/// package that provides each (whool's table).
const PACKAGES: &[(&str, &str)] = &[
    ("Asterisk", "py-Asterisk"),
    ("coda", "pycoda"),
    ("cups", "pycups"),
    ("dateutil", "python-dateutil"),
    ("ldap", "python-ldap"),
    ("serial", "pyserial"),
    ("suds", "suds-jurko"),
    ("stdnum", "python-stdnum"),
    ("Crypto.Cipher.DES3", "pycrypto"),
    ("OpenSSL", "pyOpenSSL"),
];

/// What `run` did, or would do, to the file.
#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    Written(PathBuf),
    Removed(PathBuf),
}

/// A requirement's name as packaging compares it: lower case, runs of `-`,
/// `_` and `.` as one `-`.
fn canonical_name(requirement: &str) -> String {
    let mut canonical = String::new();
    let name = requirement
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    for c in name {
        if matches!(c, '-' | '_' | '.') {
            if !canonical.ends_with('-') {
                canonical.push('-');
            }
        } else {
            canonical.push(c.to_ascii_lowercase());
        }
    }
    canonical
}

/// The `[tool.whool] external_dependencies_override` of a module's
/// `pyproject.toml`: import name -> requirements.
fn overrides(module: &Path) -> HashMap<String, Vec<String>> {
    let mut found = HashMap::new();
    let Ok(text) = std::fs::read_to_string(module.join("pyproject.toml")) else {
        return found;
    };
    let Ok(toml) = text.parse::<toml::Table>() else {
        return found;
    };
    let python = toml
        .get("tool")
        .and_then(|t| t.get("whool"))
        .and_then(|w| w.get("external_dependencies_override"))
        .and_then(|o| o.get("python"))
        .and_then(|p| p.as_table());
    for (name, value) in python.into_iter().flatten() {
        let requirements = match value {
            toml::Value::String(s) => vec![s.clone()],
            toml::Value::Array(items) => items.iter().filter_map(|i| i.as_str().map(String::from)).collect(),
            _ => continue,
        };
        found.insert(name.clone(), requirements);
    }
    found
}

/// The Python requirements of one module, or none when it is not installable.
fn module_requirements(module: &Path) -> Vec<String> {
    let Some(source) = crate::manifest::manifest_path(module).and_then(|m| std::fs::read_to_string(m).ok()) else {
        return Vec::new();
    };
    let Some(manifest) = Manifest::parse(&source) else {
        return Vec::new();
    };
    if manifest.get("installable").and_then(is_truthy) == Some(false) {
        return Vec::new();
    }
    let Some(Expr::Dict(external)) = manifest.get("external_dependencies") else {
        return Vec::new();
    };
    let python = external.items.iter().find(|item| {
        item.key
            .as_ref()
            .and_then(|k| k.as_string_literal_expr())
            .is_some_and(|k| k.value.to_str() == "python")
    });
    let elements = match python.map(|item| &item.value) {
        Some(Expr::List(list)) => &list.elts[..],
        Some(Expr::Tuple(tuple)) => &tuple.elts[..],
        _ => return Vec::new(),
    };
    let overrides = overrides(module);
    let mut requirements = Vec::new();
    for element in elements {
        let Some(dependency) = element.as_string_literal_expr() else {
            continue;
        };
        let dependency = dependency.value.to_str();
        match overrides.get(dependency) {
            Some(found) => requirements.extend(found.iter().cloned()),
            None => requirements.push(
                PACKAGES
                    .iter()
                    .find(|(import, _)| *import == dependency)
                    .map_or(dependency, |(_, package)| package)
                    .to_string(),
            ),
        }
    }
    requirements
}

/// The Python requirements of the installable modules directly in `dir`.
pub fn requirements(dir: &Path) -> BTreeSet<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return BTreeSet::new();
    };
    entries
        .flatten()
        .flat_map(|entry| module_requirements(&entry.path()))
        .filter(|r| !r.trim().is_empty() && !ODOO_PACKAGE.is_match(&canonical_name(r)))
        .collect()
}

/// The content of `requirements.txt` for `requirements`; `None` when there
/// are none and the file goes.
pub fn content(requirements: &BTreeSet<String>) -> Option<String> {
    if requirements.is_empty() {
        return None;
    }
    // In code point order, as Python sorts strings.
    let lines: Vec<&str> = requirements.iter().map(String::as_str).collect();
    Some(format!("{HEADER}{}\n", lines.join("\n")))
}

/// Writes `dir/requirements.txt` (or removes it when no module has a Python
/// dependency); with `check`, only reports what it would change.
pub fn run(dir: &Path, check: bool) -> std::io::Result<Option<Change>> {
    let path = dir.join("requirements.txt");
    let current = std::fs::read_to_string(&path).ok();
    match content(&requirements(dir)) {
        Some(new) if current.as_deref() != Some(new.as_str()) => {
            if !check {
                std::fs::write(&path, new)?;
            }
            Ok(Some(Change::Written(path)))
        }
        None if current.is_some() => {
            if !check {
                std::fs::remove_file(&path)?;
            }
            Ok(Some(Change::Removed(path)))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(root: &Path, name: &str, manifest: &str) {
        std::fs::create_dir_all(root.join(name)).unwrap();
        std::fs::write(root.join(name).join("__manifest__.py"), manifest).unwrap();
    }

    #[test]
    fn requirements_of_installable_modules() {
        let dir = tempfile::tempdir().unwrap();
        module(
            dir.path(),
            "acme_a",
            "{'name': 'A', 'external_dependencies': {'python': ['requests', 'PyYAML>=6', 'odoo-addon-base_x', 'dateutil', 'zeep']}}",
        );
        std::fs::write(
            dir.path().join("acme_a/pyproject.toml"),
            "[tool.whool.external_dependencies_override.python]\nzeep = \"zeep==4.2.1\"\n",
        )
        .unwrap();
        module(
            dir.path(),
            "acme_b",
            "{'name': 'B', 'external_dependencies': {'python': ['requests'], 'bin': ['wkhtmltopdf']}}",
        );
        module(
            dir.path(),
            "acme_c",
            "{'name': 'C', 'installable': False, 'external_dependencies': {'python': ['xlrd']}}",
        );
        module(
            dir.path(),
            "acme_d",
            "{'name': 'D', 'external_dependencies': {'python': []}}",
        );
        let path = dir.path().join("requirements.txt");
        assert_eq!(run(dir.path(), true).unwrap(), Some(Change::Written(path.clone())));
        assert!(!path.exists());
        run(dir.path(), false).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# generated from manifests external_dependencies\nPyYAML>=6\npython-dateutil\nrequests\nzeep==4.2.1\n"
        );
        assert_eq!(run(dir.path(), false).unwrap(), None);
    }

    #[test]
    fn no_requirements_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        module(
            dir.path(),
            "acme_d",
            "{'name': 'D', 'external_dependencies': {'python': []}}",
        );
        let path = dir.path().join("requirements.txt");
        std::fs::write(&path, "# generated from manifests external_dependencies\nzeep\n").unwrap();
        assert_eq!(run(dir.path(), false).unwrap(), Some(Change::Removed(path.clone())));
        assert!(!path.exists());
    }

    #[test]
    fn canonical_names() {
        assert_eq!(canonical_name("Odoo18-Addon_base.x>=1"), "odoo18-addon-base-x");
        assert!(ODOO_PACKAGE.is_match(&canonical_name("odoo")));
        assert!(!ODOO_PACKAGE.is_match(&canonical_name("odoo-test-helper")));
    }
}
