//! `odl format`: XML files formatted by [lax-markup], which only moves
//! whitespace, with a check that the document still means the same:
//! elements, attributes, comments, every text that is not indentation
//! between elements, and the whitespace of record field values. A file the check rejects is left as it is and reported.
//!
//! Text matters in Odoo XML: a record's `<field>` is loaded as written,
//! surrounding whitespace included, and QWeb renders the spaces between
//! inline elements.
//!
//! [lax-markup]: https://github.com/bartlomieju/lax

use crate::settings::Settings;
use dprint_core::configuration::{ConfigKeyMap, ConfigKeyValue, GlobalConfiguration};
use std::path::{Path, PathBuf};

/// The line width of OCA's prettier configuration.
pub const DEFAULT_LINE_WIDTH: u32 = 88;
pub const DEFAULT_INDENT_WIDTH: u8 = 4;

/// What formatting a file did, or would do.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Already formatted.
    Unchanged,
    /// The new content.
    Formatted(String),
    /// Formatting would change what the document means; left as it is.
    Refused(String),
    /// Not well-formed XML; left as it is.
    Invalid(String),
}

/// The configuration lax-markup gets.
pub fn configuration(line_width: u32, indent_width: u8) -> lax_markup::configuration::Configuration {
    let mut map = ConfigKeyMap::new();
    map.insert("lineWidth".into(), ConfigKeyValue::from_i32(line_width as i32));
    map.insert("indentWidth".into(), ConfigKeyValue::from_i32(i32::from(indent_width)));
    lax_markup::configuration::resolve_config(map, &GlobalConfiguration::default()).config
}

/// A document reduced to what formatting must not change.
#[derive(Debug, PartialEq, Eq)]
enum Item {
    Start(String, Vec<(String, String)>),
    End,
    Text(String),
    Comment(String),
    Pi(String),
}

fn skeleton(source: &str) -> Result<Vec<Item>, String> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(source, options).map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    walk(doc.root_element(), false, &mut items);
    Ok(items)
}

/// `value`: whether the element's text is a value as written: a `<field>`
/// of a record, or a `<value>`, which Odoo loads without stripping.
fn walk(node: roxmltree::Node, value: bool, items: &mut Vec<Item>) {
    let name = node.tag_name();
    let qualified = match name.namespace() {
        Some(ns) => format!("{{{ns}}}{}", name.name()),
        None => name.name().to_string(),
    };
    let attributes = node
        .attributes()
        .map(|a| (a.name().to_string(), a.value().to_string()))
        .collect();
    items.push(Item::Start(qualified, attributes));
    // Whitespace between the children of an element that holds elements and
    // no other text is indentation; other text is content.
    let has_children = node.children().any(|c| c.is_element() || c.is_comment() || c.is_pi());
    let mixed = node
        .children()
        .any(|c| c.is_text() && c.text().is_some_and(|t| !t.trim().is_empty()));
    for child in node.children() {
        match child.node_type() {
            roxmltree::NodeType::Element => {
                let tag = child.tag_name().name();
                let holds_value = (tag == "field" && node.tag_name().name() == "record") || tag == "value";
                walk(child, holds_value, items);
            }
            roxmltree::NodeType::Text => {
                let text = child.text().unwrap_or_default();
                // Whitespace alone in an empty element renders as nothing,
                // except in a value.
                let blank = text.trim().is_empty();
                if !blank || mixed || (!has_children && value) {
                    items.push(Item::Text(text.to_string()));
                }
            }
            // A comment's lines may be re-indented: compared word by word.
            roxmltree::NodeType::Comment => items.push(Item::Comment(
                child
                    .text()
                    .unwrap_or_default()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            )),
            roxmltree::NodeType::PI => {
                let pi = child
                    .pi()
                    .map(|p| format!("{} {}", p.target, p.value.unwrap_or_default()));
                items.push(Item::Pi(pi.unwrap_or_default()));
            }
            roxmltree::NodeType::Root => {}
        }
    }
    items.push(Item::End);
}

/// Formats the XML `source` of `path`.
pub fn format_source(path: &Path, source: &str, config: &lax_markup::configuration::Configuration) -> Outcome {
    let before = match skeleton(source) {
        Ok(items) => items,
        Err(error) => return Outcome::Invalid(error),
    };
    let formatted = match lax_markup::format_text(path, source, config) {
        Ok(Some(text)) => text,
        Ok(None) => return Outcome::Unchanged,
        Err(error) => return Outcome::Invalid(error.to_string()),
    };
    match skeleton(&formatted) {
        Ok(after) if after == before => Outcome::Formatted(formatted),
        Ok(after) => {
            let at = before
                .iter()
                .zip(&after)
                .position(|(a, b)| a != b)
                .unwrap_or(before.len().min(after.len()));
            let what = before.get(at).map_or_else(String::new, |item| match item {
                Item::Text(text) => format!(": the text {:?}", text.chars().take(40).collect::<String>()),
                Item::Start(name, _) => format!(": in <{name}>"),
                _ => String::new(),
            });
            Outcome::Refused(format!("formatting would change the content{what}"))
        }
        Err(error) => Outcome::Refused(format!("formatting would break the XML: {error}")),
    }
}

/// The XML files under `paths`, without excluded ones.
pub fn xml_files(paths: &[PathBuf], settings: &Settings) -> Vec<PathBuf> {
    crate::linter::collect_files(paths, settings)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "xml"))
        .collect()
}

/// Formats every XML file under `paths`; with `check`, writes nothing.
/// Returns each file whose outcome is not `Unchanged`.
pub fn run(paths: &[PathBuf], settings: &Settings, check: bool) -> std::io::Result<Vec<(PathBuf, Outcome)>> {
    let format = &settings.config.format.clone().unwrap_or_default();
    let config = configuration(
        format.line_width.unwrap_or(DEFAULT_LINE_WIDTH),
        format.indent_width.unwrap_or(DEFAULT_INDENT_WIDTH),
    );
    let mut results = Vec::new();
    for path in xml_files(paths, settings) {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let outcome = format_source(&path, &source, &config);
        if let Outcome::Formatted(text) = &outcome {
            if !check {
                std::fs::write(&path, text)?;
            }
        }
        if outcome != Outcome::Unchanged {
            results.push((path, outcome));
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(source: &str) -> Outcome {
        format_source(Path::new("data.xml"), source, &configuration(88, 4))
    }

    #[test]
    fn indentation_moves() {
        let out = format(
            "<odoo>\n<record id=\"a\" model=\"res.partner\">\n<field name=\"name\">A</field>\n</record>\n</odoo>\n",
        );
        assert_eq!(
            out,
            Outcome::Formatted(
                "<odoo>\n    <record id=\"a\" model=\"res.partner\">\n        <field name=\"name\">A</field>\n    </record>\n</odoo>\n"
                    .to_string()
            )
        );
        let Outcome::Formatted(text) = out else { unreachable!() };
        assert_eq!(format(&text), Outcome::Unchanged);
    }

    #[test]
    fn domains_stay_as_written() {
        let source = "<odoo>\n    <record id=\"a\" model=\"ir.rule\">\n        <field name=\"domain_force\">[('user_id', '=', user.id), ('amount', '&lt;', 10)]</field>\n    </record>\n</odoo>\n";
        let text = match format(source) {
            Outcome::Formatted(text) => text,
            Outcome::Unchanged => source.to_string(),
            other => panic!("{other:?}"),
        };
        assert!(
            text.contains("[('user_id', '=', user.id), ('amount', '&lt;', 10)]"),
            "{text}"
        );
    }

    #[test]
    fn text_that_would_change_is_refused() {
        // lax-markup 0.3 moves CDATA onto its own line inside the field.
        let source = "<odoo>\n<record id=\"a\" model=\"ir.config_parameter\">\n<field name=\"value\"><![CDATA[TEST<br/>]]></field>\n</record>\n</odoo>\n";
        assert!(matches!(format(source), Outcome::Refused(_)), "{:?}", format(source));
    }

    #[test]
    fn invalid_xml() {
        assert!(matches!(format("<odoo><record></odoo>"), Outcome::Invalid(_)));
    }
}
