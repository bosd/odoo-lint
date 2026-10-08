//! `odl mcp`: a [Model Context Protocol](https://modelcontextprotocol.io)
//! server, so AI coding agents (Claude Code, OpenCode, dsh, ...) can lint,
//! fix and look up rules. JSON-RPC 2.0 messages, one per line, on
//! stdin/stdout; only tools are offered.

use crate::diagnostics::Violation;
use crate::fix::{Applicability, FixMode};
use crate::settings::{CliOverrides, Settings};
use crate::{fixer, linter, rules};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

/// Protocol versions this server speaks, newest first.
const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Violations listed in one tool result; more would flood the context.
const MAX_LISTED: usize = 200;

const INSTRUCTIONS: &str = "odoo-lint checks Odoo addons: Python code, \
manifests and translation (.po/.pot) files, with pylint-odoo's codes and \
messages. Run `check` after editing an addon, `fix` to apply automatic \
fixes (review unsafe ones with dry_run first), and `rule` to read why a rule \
matters and how to fix it.";

/// Serves requests from `input` until it is closed.
pub fn serve(input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle_message(&message),
            Err(err) => Some(error_response(Value::Null, -32700, &format!("Parse error: {err}"))),
        };
        if let Some(response) = response {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// The response to a message; `None` for notifications and responses.
fn handle_message(message: &Value) -> Option<Value> {
    let method = message.get("method")?.as_str()?;
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match handle_request(method, &params) {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, text)) => error_response(id, code, &text),
    })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn handle_request(method: &str, params: &Value) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str);
            let version = requested
                .filter(|v| PROTOCOL_VERSIONS.contains(v))
                .unwrap_or(PROTOCOL_VERSIONS[0]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "odoo-lint", "title": "odoo-lint", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let outcome = match name {
                "check" => check(&arguments),
                "fix" => fix(&arguments),
                "rule" => rule(&arguments),
                _ => return Err((-32602, format!("Unknown tool: {name}"))),
            };
            Ok(match outcome {
                Ok(text) => json!({"content": [{"type": "text", "text": text}], "isError": false}),
                Err(text) => json!({"content": [{"type": "text", "text": text}], "isError": true}),
            })
        }
        _ => Err((-32601, format!("Method not found: {method}"))),
    }
}

fn tools() -> Value {
    let selection = json!({
        "paths": {
            "type": "array",
            "items": {"type": "string"},
            "description": "Addon directories or files; default: the current directory"
        },
        "select": {
            "type": "array",
            "items": {"type": "string"},
            "description": "Only these rules: codes, names or code prefixes such as PO or C81"
        },
        "ignore": {"type": "array", "items": {"type": "string"}, "description": "Rules to skip"},
        "odoo_version": {
            "type": "string",
            "description": "Target Odoo version such as 18.0; default: from the project's configuration"
        }
    });
    let mut fix_properties = selection.clone();
    fix_properties["unsafe"] = json!({
        "type": "boolean",
        "description": "Also apply unsafe fixes, which can change behaviour (default false)"
    });
    fix_properties["dry_run"] = json!({
        "type": "boolean",
        "description": "Return the changes as a diff instead of writing them (default false)"
    });
    json!([
        {
            "name": "check",
            "title": "Lint Odoo addons",
            "description": "Lint Odoo addons (Python, manifests, .po/.pot translations) and list the violations, marking the ones `fix` can fix.",
            "inputSchema": {"type": "object", "properties": selection},
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
        },
        {
            "name": "fix",
            "title": "Fix Odoo addons",
            "description": "Apply odoo-lint's automatic fixes to Odoo addons and list what is left. Use dry_run to see a diff first.",
            "inputSchema": {"type": "object", "properties": fix_properties},
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
        },
        {
            "name": "rule",
            "title": "Explain an odoo-lint rule",
            "description": "Explain a rule by code or name (e.g. W8161 or prefer-env-translation): what it checks, why, and how to fix it. Without a rule, list all rules.",
            "inputSchema": {
                "type": "object",
                "properties": {"rule": {"type": "string", "description": "Rule code or name"}}
            },
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
        }
    ])
}

fn string_list(arguments: &Value, key: &str) -> Vec<String> {
    arguments
        .get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// The paths and settings a tool call asks for.
fn settings(arguments: &Value) -> Result<(Vec<PathBuf>, Settings), String> {
    let mut paths: Vec<PathBuf> = string_list(arguments, "paths").into_iter().map(PathBuf::from).collect();
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    if let Some(missing) = paths.iter().find(|p| !p.exists()) {
        return Err(format!("Path not found: {}", missing.display()));
    }
    let select = string_list(arguments, "select");
    let overrides = CliOverrides {
        target_version: arguments
            .get("odoo_version")
            .and_then(Value::as_str)
            .map(str::to_string),
        select: (!select.is_empty()).then_some(select),
        ignore: string_list(arguments, "ignore"),
        ..CliOverrides::default()
    };
    let loaded = Settings::load(&paths[0], None, overrides)?;
    Ok((paths, loaded.settings))
}

fn check(arguments: &Value) -> Result<String, String> {
    let (paths, settings) = settings(arguments)?;
    let violations = linter::lint_paths(&paths, &settings);
    Ok(describe(&violations, &settings))
}

fn fix(arguments: &Value) -> Result<String, String> {
    let (paths, settings) = settings(arguments)?;
    let flag = |key: &str| arguments.get(key).and_then(Value::as_bool).unwrap_or(false);
    let mode = if flag("unsafe") { FixMode::Unsafe } else { FixMode::Safe };
    let result = fixer::fix_paths(&paths, &settings, mode);
    if flag("dry_run") {
        if result.changed.is_empty() {
            return Ok("No changes.".to_string());
        }
        let mut text = format!(
            "{} fix(es) would change {} file(s):\n\n",
            result.fixed,
            result.changed.len()
        );
        for (path, old, new) in &result.changed {
            text.push_str(&fixer::unified_diff(path, old, new));
        }
        return Ok(text);
    }
    let mut written = Vec::new();
    for (path, _, new) in &result.changed {
        std::fs::write(path, new).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
        written.push(path.display().to_string());
    }
    let mut text = format!("Fixed {} violation(s)", result.fixed);
    if written.is_empty() {
        text.push_str(".\n");
    } else {
        text.push_str(&format!(" in {} file(s): {}.\n", written.len(), written.join(", ")));
    }
    text.push('\n');
    text.push_str(&describe(&result.remaining, &settings));
    Ok(text)
}

fn rule(arguments: &Value) -> Result<String, String> {
    match arguments.get("rule").and_then(Value::as_str) {
        Some(code) => rules::find(code)
            .map(rules::Rule::to_markdown)
            .ok_or_else(|| format!("Unknown rule '{code}'. Call `rule` without arguments to list all rules.")),
        None => Ok(rules::ALL
            .iter()
            .map(|r| format!("{} {}: {}\n", r.code, r.name, r.summary))
            .collect()),
    }
}

/// Violations as text, one per line, with how to fix them.
fn describe(violations: &[Violation], settings: &Settings) -> String {
    if violations.is_empty() {
        return format!("No violations found (Odoo {}).", settings.target_version);
    }
    let mut text = String::new();
    for v in violations.iter().take(MAX_LISTED) {
        text.push_str(&format!(
            "{}:{}:{}: {} {} ({})",
            v.file_path, v.line, v.column, v.code, v.message, v.name
        ));
        if let Some(fix) = &v.fix {
            let kind = match fix.applicability {
                Applicability::Safe => "fix",
                Applicability::Unsafe => "unsafe fix",
            };
            text.push_str(&format!(" [{kind}: {}]", fix.title));
        }
        text.push('\n');
    }
    if violations.len() > MAX_LISTED {
        text.push_str(&format!(
            "... and {} more; narrow down with `paths` or `select`.\n",
            violations.len() - MAX_LISTED
        ));
    }
    let count = |a: Applicability| {
        violations
            .iter()
            .filter(|v| v.fix.as_ref().is_some_and(|f| f.applicability == a))
            .count()
    };
    text.push_str(&format!(
        "\n{} violation(s) (Odoo {}); {} fixable with `fix`, {} more with `unsafe: true`. Use `rule` to read about a code.",
        violations.len(),
        settings.target_version,
        count(Applicability::Safe),
        count(Applicability::Unsafe),
    ));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exchange(requests: &[Value]) -> Vec<Value> {
        let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
        let mut output = Vec::new();
        serve(input.as_bytes(), &mut output).unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn handshake_and_tools() {
        let responses = exchange(&[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "rule", "arguments": {"rule": "W8161"}}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "check", "arguments": {"paths": ["/nonexistent/addons"]}}}),
            json!({"jsonrpc": "2.0", "id": 5, "method": "resources/list"}),
        ]);
        assert_eq!(responses.len(), 5, "no response to the notification");
        assert_eq!(responses[0]["result"]["protocolVersion"], "2025-03-26");
        let names: Vec<&str> = responses[1]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["check", "fix", "rule"]);
        let doc = responses[2]["result"]["content"][0]["text"].as_str().unwrap();
        assert!(doc.starts_with("# prefer-env-translation (W8161)"));
        assert_eq!(responses[3]["result"]["isError"], true);
        assert_eq!(responses[4]["error"]["code"], -32601);
    }

    #[test]
    fn unknown_protocol_version_gets_the_newest() {
        let responses = exchange(&[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "1999-01-01"}}),
        ]);
        assert_eq!(responses[0]["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn invalid_json() {
        let mut output = Vec::new();
        serve("{not json\n".as_bytes(), &mut output).unwrap();
        let response: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(response["error"]["code"], -32700);
    }
}
