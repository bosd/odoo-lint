//! `odl readme`: the output of the OBS client template's generator, byte for
//! byte. The expected files in `tests/fixtures/readme/*/expected` were made
//! by the template's `convert_readme2html.py` (comrak 0.54 via the Python
//! bindings) from the fragments next to them.

use odoo_lint::readme::{outputs, run};
use std::fs;
use std::path::{Path, PathBuf};

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A module `name` in a temporary folder with the fixture's fragments.
fn module(root: &Path, name: &str) -> PathBuf {
    let module = root.join(name);
    copy_dir(
        &Path::new("tests/fixtures/readme").join(name).join("readme"),
        &module.join("readme"),
    );
    fs::write(module.join("__manifest__.py"), "{'name': 'X'}\n").unwrap();
    module
}

#[test]
fn same_output_as_the_template_generator() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["acme_full", "acme_empty", "acme_credits"] {
        let module = module(dir.path(), name);
        let expected = Path::new("tests/fixtures/readme").join(name).join("expected");
        let files = outputs(&module).unwrap();
        assert_eq!(
            files[0].1,
            fs::read_to_string(expected.join("README.md")).unwrap(),
            "{name} README.md"
        );
        assert_eq!(
            files[1].1,
            fs::read_to_string(expected.join("index.html")).unwrap(),
            "{name} index.html"
        );
    }
}

#[test]
fn writes_changed_files_and_checks() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "acme_full");
    // No readme/ folder: left alone.
    fs::create_dir_all(dir.path().join("acme_plain")).unwrap();
    fs::write(dir.path().join("acme_plain/__manifest__.py"), "{}\n").unwrap();

    let root = vec![dir.path().to_path_buf()];
    assert_eq!(run(&root, true).unwrap().len(), 2, "both files are missing");
    assert!(!dir.path().join("acme_full/README.md").exists(), "check writes nothing");
    assert_eq!(run(&root, false).unwrap().len(), 2);
    assert!(dir.path().join("acme_full/static/description/index.html").is_file());
    assert!(run(&root, true).unwrap().is_empty(), "up to date");
    assert!(!dir.path().join("acme_plain/README.md").exists());

    // A changed fragment, passed as a file the way hooks do.
    let usage = dir.path().join("acme_full/readme/USAGE.md");
    fs::write(&usage, "Changed.\n").unwrap();
    let changes = run(&[usage], false).unwrap();
    assert_eq!(changes.len(), 2);
    assert!(fs::read_to_string(dir.path().join("acme_full/README.md"))
        .unwrap()
        .contains("## Usage\nChanged.\n"));
}

#[test]
fn mermaid_diagrams() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("acme_flow");
    fs::create_dir_all(module.join("readme")).unwrap();
    fs::write(module.join("__manifest__.py"), "{}\n").unwrap();
    fs::write(
        module.join("readme/USAGE.md"),
        "Flow:\n\n```mermaid\ngraph TD\n  A-->B\n```\n",
    )
    .unwrap();
    let html = &outputs(&module).unwrap()[1].1;
    assert!(
        html.contains("<div class=\"mermaid\">\ngraph TD\n  A-->B\n</div>"),
        "{html}"
    );
}
