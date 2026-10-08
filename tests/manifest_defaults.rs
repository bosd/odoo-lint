//! Fixes that fill in missing manifest keys from `manifest-defaults`.

use odoo_lint::config::OdooLintConfig;
use odoo_lint::fix::FixMode;
use odoo_lint::fixer::fix_paths;
use odoo_lint::settings::{CliOverrides, Settings};
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG: &str = r#"
select = ["C8101", "C8102"]

[manifest-defaults]
"*" = { license = "AGPL-3" }
"acme_*" = { license = "LGPL-3", author = "Acme Corp" }

[rules.manifest-required-author.mapping]
"acme_*" = "Acme Corp"
"#;

fn settings(config: &str) -> Settings {
    let config = OdooLintConfig::from_odoo_lint_toml(config).unwrap();
    Settings::new(config, None, CliOverrides::default()).unwrap().0
}

fn fixed(dir: &Path, name: &str, manifest: &str, config: &str, mode: FixMode) -> String {
    let module = dir.join(name);
    fs::create_dir_all(&module).unwrap();
    fs::write(module.join("README.rst"), "x\n").unwrap();
    let path: PathBuf = module.join("__manifest__.py");
    fs::write(&path, manifest).unwrap();
    let result = fix_paths(&[module], &settings(config), mode);
    result
        .changed
        .into_iter()
        .find(|(p, _, _)| *p == path)
        .map_or_else(|| manifest.to_string(), |(_, _, new)| new)
}

#[test]
fn the_most_specific_pattern_wins_and_the_style_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = "{\n    'name': 'Sale',\n    'version': '18.0.1.0.0'\n}\n";
    assert_eq!(
        fixed(dir.path(), "acme_sale", manifest, CONFIG, FixMode::Safe),
        "{\n    'name': 'Sale',\n    'version': '18.0.1.0.0',\n    'author': 'Acme Corp',\n    'license': 'LGPL-3',\n}\n"
    );
    let manifest = "{\"name\": \"Stock\", \"author\": \"Odoo Community Association (OCA)\",}\n";
    assert_eq!(
        fixed(dir.path(), "stock_extra", manifest, CONFIG, FixMode::Safe),
        "{\"name\": \"Stock\", \"author\": \"Odoo Community Association (OCA)\", \"license\": \"AGPL-3\",}\n"
    );
}

#[test]
fn nothing_is_guessed() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = "{\n    \"name\": \"Sale\",\n}\n";
    // No configuration: neither a license nor the default OCA author.
    assert_eq!(
        fixed(
            dir.path(),
            "acme_sale",
            manifest,
            "select = [\"C8101\", \"C8102\"]\n",
            FixMode::Unsafe
        ),
        manifest
    );
}

#[test]
fn an_existing_author_is_never_changed() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = "{\n    \"name\": \"Sale\",\n    \"author\": \"Someone\",\n    \"license\": \"LGPL-3\",\n}\n";
    assert_eq!(
        fixed(dir.path(), "acme_sale", manifest, CONFIG, FixMode::Unsafe),
        manifest
    );
}
