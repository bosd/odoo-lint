//! Rules on `__manifest__.py`, ported from pylint-odoo with its codes, names
//! and messages. Positions follow pylint-odoo: on the offending key when there
//! is one, otherwise on the manifest dict.

use crate::checker::ManifestContext;
use crate::fix::{Edit, Fix};
use ruff_python_ast::Expr;
use ruff_text_size::{Ranged, TextSize};

pub mod author;
pub mod files;
pub mod keys;
pub mod values;

/// Offset of `key` in the manifest, or of the dict when the key is absent.
pub(crate) fn key_or_dict(ctx: &ManifestContext, key: &str) -> TextSize {
    ctx.manifest
        .entry(key)
        .map_or_else(|| ctx.manifest.dict().start(), |(key, _)| key.start())
}

/// A TOML value as a Python literal with `quote` for strings; `None` for
/// values a manifest does not hold (tables, dates).
fn python_literal(value: &toml::Value, quote: char) -> Option<String> {
    Some(match value {
        toml::Value::String(s) => {
            let escaped = s
                .replace('\\', "\\\\")
                .replace(quote, &format!("\\{quote}"))
                .replace('\n', "\\n");
            format!("{quote}{escaped}{quote}")
        }
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => if *b { "True" } else { "False" }.to_string(),
        toml::Value::Array(items) => {
            let items: Option<Vec<String>> = items.iter().map(|i| python_literal(i, quote)).collect();
            format!("[{}]", items?.join(", "))
        }
        _ => return None,
    })
}

/// A safe fix adding `key` with its value from `manifest-defaults`, as a
/// new last entry in the manifest's own style (quotes, indentation, trailing
/// comma); `None` when no default is configured.
pub(crate) fn add_default(ctx: &ManifestContext, key: &str) -> Option<Fix> {
    let value = ctx.settings.config.manifest_default(&ctx.module.name, key)?;
    add_value(ctx, key, value)
}

/// A safe fix adding `key: value` as the manifest's last entry.
pub(crate) fn add_value(ctx: &ManifestContext, key: &str, value: &toml::Value) -> Option<Fix> {
    let source = ctx.source;
    let dict = ctx.manifest.dict();
    let first_key = dict.items.first().and_then(|item| item.key.as_ref());
    let quote = first_key
        .and_then(|k| source[k.range()].chars().next())
        .filter(|c| *c == '\'')
        .unwrap_or('"');
    let literal = python_literal(value, quote)?;
    let entry = format!("{quote}{key}{quote}: {literal}");
    let Some(last) = dict.items.last() else {
        // `{}`: put the entry between the braces.
        let at = dict.start().to_usize() + 1;
        return Some(Fix::safe(
            format!("Add `{key}` from the configuration"),
            vec![Edit::insert(at, entry)],
        ));
    };
    let after_value = last.value.end().to_usize();
    let rest = &source[after_value..];
    let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let has_comma = rest[spaces..].starts_with(',');
    // Indentation of the entries, when each has a line of its own.
    let key_start = first_key.map_or(after_value, |k| k.start().to_usize());
    let line_start = source[..key_start].rfind('\n').map_or(0, |i| i + 1);
    let indent = &source[line_start..key_start];
    let multiline = indent.trim().is_empty() && line_start > dict.start().to_usize();
    let (at, text) = match (multiline, has_comma) {
        (true, true) => (after_value + spaces + 1, format!("\n{indent}{entry},")),
        (true, false) => (after_value, format!(",\n{indent}{entry},")),
        (false, true) => (after_value + spaces + 1, format!(" {entry},")),
        (false, false) => (after_value, format!(", {entry}")),
    };
    // Replace the character before the insertion point with itself, so two
    // added entries overlap and the second waits for the next fix pass.
    let before = source[..at].chars().next_back()?;
    let start = at - before.len_utf8();
    Some(Fix::safe(
        format!("Add `{key}` from the configuration"),
        vec![Edit::replace(start, at, format!("{before}{text}"))],
    ))
}

/// String elements of a list or tuple literal.
pub(crate) fn string_elements(expr: &Expr) -> Vec<(&str, &Expr)> {
    let elts = match expr {
        Expr::List(list) => &list.elts[..],
        Expr::Tuple(tuple) => &tuple.elts[..],
        _ => return Vec::new(),
    };
    elts.iter()
        .filter_map(|e| e.as_string_literal_expr().map(|s| (s.value.to_str(), e)))
        .collect()
}
