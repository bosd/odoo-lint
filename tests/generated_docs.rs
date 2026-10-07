//! Keeps `docs/rules/` in sync with the rule docs in the Rust sources.
//!
//! The pages are committed so Read the Docs can build without a Rust
//! toolchain. Regenerate them with:
//!
//!     UPDATE_DOCS=1 cargo test --test generated_docs

use odoo_lint::rules::{Rule, ALL};
use std::fs;
use std::path::Path;

const HEADER: &str =
    "<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->\n\n";

fn index_page(rules: &[Rule]) -> String {
    let mut page = String::from(HEADER);
    page.push_str("# Rules\n\n");
    page.push_str("Run `odl rule <CODE>` to show the same documentation in the terminal.\n\n");
    page.push_str("| Code | Name | Summary |\n| ---- | ---- | ------- |\n");
    for rule in rules {
        page.push_str(&format!(
            "| [{code}]({code}.md) | `{}` | {} |\n",
            rule.name,
            rule.summary,
            code = rule.code
        ));
    }
    page.push_str("\n```{toctree}\n:hidden:\n\n");
    for rule in rules {
        page.push_str(rule.code);
        page.push('\n');
    }
    page.push_str("```\n");
    page
}

#[test]
fn rule_docs_are_up_to_date() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/rules");
    let mut expected: Vec<(String, String)> = ALL
        .iter()
        .map(|rule| (format!("{}.md", rule.code), format!("{HEADER}{}", rule.to_markdown())))
        .collect();
    expected.push(("index.md".to_string(), index_page(ALL)));

    if std::env::var_os("UPDATE_DOCS").is_some() {
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in &expected {
            fs::write(dir.join(name), content).unwrap();
        }
        return;
    }

    let mut stale = Vec::new();
    for (name, content) in &expected {
        if fs::read_to_string(dir.join(name)).ok().as_deref() != Some(content.as_str()) {
            stale.push(name.clone());
        }
    }
    let mut on_disk: Vec<String> = fs::read_dir(&dir)
        .map(|entries| entries.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    on_disk.retain(|name| !expected.iter().any(|(n, _)| n == name));
    stale.extend(on_disk.into_iter().map(|n| format!("{n} (no longer generated)")));

    assert!(
        stale.is_empty(),
        "docs/rules is out of date: {stale:?}\nRun: UPDATE_DOCS=1 cargo test --test generated_docs"
    );
}
