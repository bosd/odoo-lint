//! The agent integrations under `integrations/`: files that are copied
//! between them stay identical, and their JSON is valid.

use std::fs;
use std::path::Path;

fn read(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn json(path: &str) -> serde_json::Value {
    serde_json::from_str(&read(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn shared_files_are_identical() {
    // A published package cannot point outside its folder, so these are copies.
    for file in ["bin/odl", "skills/odoo-lint/SKILL.md"] {
        assert_eq!(
            read(&format!("integrations/claude-code/{file}")),
            read(&format!("integrations/dsh/{file}")),
            "integrations/dsh/{file} differs from the Claude Code copy"
        );
    }
}

#[test]
fn manifests_are_valid() {
    let marketplace = json(".claude-plugin/marketplace.json");
    assert_eq!(marketplace["plugins"][0]["source"], "./integrations/claude-code");
    let plugin = json("integrations/claude-code/.claude-plugin/plugin.json");
    assert_eq!(plugin["name"], marketplace["plugins"][0]["name"]);
    for path in [
        "integrations/claude-code/.mcp.json",
        "integrations/claude-code/hooks/hooks.json",
        "integrations/dsh/hooks.json",
        "integrations/dsh/package.json",
    ] {
        json(path);
    }
}

#[test]
fn plugins_belong_to_this_release() {
    // Anthropic's directory needs an exact version for what `bin/odl` runs
    // through uvx, so the plugins follow odoo-lint's versions.
    let version = env!("CARGO_PKG_VERSION");
    let pypi = version
        .replace("-alpha.", "a")
        .replace("-beta.", "b")
        .replace("-rc.", "rc");
    for path in [
        "integrations/claude-code/.claude-plugin/plugin.json",
        "integrations/dsh/package.json",
    ] {
        assert_eq!(json(path)["version"], version, "{path}: version");
    }
    for path in ["integrations/claude-code/bin/odl", "integrations/dsh/bin/odl"] {
        assert!(
            read(path).contains(&format!("uvx --quiet --from odoo-linter=={pypi} odl")),
            "{path} does not run odoo-linter=={pypi} through uvx"
        );
    }
}

#[test]
fn skill_names_the_mcp_tools() {
    let skill = read("integrations/claude-code/skills/odoo-lint/SKILL.md");
    let input = concat!(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "\n");
    let mut output = Vec::new();
    odoo_lint::mcp::serve(input.as_bytes(), &mut output).unwrap();
    let response: serde_json::Value = serde_json::from_slice(&output).unwrap();
    for tool in response["result"]["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap();
        assert!(
            skill.contains(&format!("`{name}`")),
            "the skill does not mention the `{name}` tool"
        );
    }
}
