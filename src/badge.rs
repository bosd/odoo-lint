//! `odl badge`: README badges as [shields.io endpoint](https://shields.io/badges/endpoint-badge)
//! JSON, for a file committed to the repository or published by CI.

use crate::linter::{collect_files, lint_files_with, modules_of};
use crate::odoo_version::OdooVersion;
use crate::settings::Settings;
use crate::sources::Sources;
use crate::upgrade;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const GREEN: &str = "3fa886";
const ORANGE: &str = "ef662f";
const RED: &str = "c0392b";

fn endpoint(label: &str, message: String, color: &str) -> Value {
    json!({"schemaVersion": 1, "label": label, "message": message, "color": color})
}

/// Share of modules without findings: `odoo-lint | 94% clean`.
pub fn clean(settings: &Settings, paths: &[PathBuf]) -> Value {
    let sources = Sources::default();
    let files = collect_files(paths, settings);
    let modules: HashMap<PathBuf, ()> = modules_of(&files, &sources)
        .iter()
        .map(|m| (m.path.clone(), ()))
        .collect();
    let violations = lint_files_with(&files, settings, &sources);
    let dirty: HashSet<&PathBuf> = violations
        .iter()
        .filter_map(|v| upgrade::module_of(&modules, Path::new(&v.file_path)).map(|(path, _)| path))
        .collect();
    if modules.is_empty() {
        return endpoint("odoo-lint", "no modules".to_string(), ORANGE);
    }
    let clean = modules.len() - dirty.len();
    // Rounded down: 100% only when every module is clean.
    let percent = clean * 100 / modules.len();
    let color = match percent {
        100 => GREEN,
        80..=99 => ORANGE,
        _ => RED,
    };
    endpoint("odoo-lint", format!("{percent}% clean"), color)
}

/// Whether the modules are ready for `target`: `Odoo 19.0 | ready`.
pub fn upgrade_ready(settings: &Settings, paths: &[PathBuf], target: OdooVersion) -> Value {
    let report = upgrade::check(settings, paths, target);
    let label = format!("Odoo {target}");
    if report.effort.changes == 0 {
        return endpoint(&label, "ready".to_string(), GREEN);
    }
    let noun = if report.effort.changes == 1 {
        "change"
    } else {
        "changes"
    };
    endpoint(&label, format!("{} {noun}", report.effort.changes), ORANGE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn module(root: &Path, name: &str, clean: bool) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        let license = if clean { "    'license': 'AGPL-3',\n" } else { "" };
        fs::write(
            dir.join("__manifest__.py"),
            format!("{{\n    'name': '{name}',\n    'version': '17.0.1.0.0',\n{license}}}\n"),
        )
        .unwrap();
        fs::write(dir.join("__init__.py"), "").unwrap();
    }

    #[test]
    fn badges() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..3 {
            module(dir.path(), &format!("clean_{i}"), true);
        }
        module(dir.path(), "dirty", false);
        let mut settings = Settings::default();
        settings.select = vec!["C8102".to_string()];
        let badge = clean(&settings, &[dir.path().to_path_buf()]);
        assert_eq!(badge["message"], "75% clean");
        assert_eq!(badge["color"], RED);

        let ready = upgrade_ready(
            &Settings::default(),
            &[dir.path().to_path_buf()],
            OdooVersion::new(18, 0),
        );
        assert_eq!(
            ready,
            json!({"schemaVersion": 1, "label": "Odoo 18.0", "message": "ready", "color": GREEN})
        );
    }
}
