//! `odl format`: stylesheets (SCSS, CSS, Less) formatted by [lax-css], and
//! XML files formatted by [lax-markup]; both only move
//! whitespace, with a check that the document still means the same:
//! elements, attributes, comments, every text that is not indentation
//! between elements, and the whitespace of record field values. A file the check rejects is left as it is and reported.
//!
//! Text matters in Odoo XML: a record's `<field>` is loaded as written,
//! surrounding whitespace included, and QWeb renders the spaces between
//! inline elements.
//!
//! [lax-markup]: https://github.com/bartlomieju/lax
//! [lax-css]: https://github.com/bartlomieju/lax

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

/// `verbatim`: inside a record's `type="html"` field, whose children Odoo
/// stores with their tails, so every bit of text is content.
fn walk(node: roxmltree::Node, verbatim: bool, items: &mut Vec<Item>) {
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
                let html =
                    tag == "field" && node.tag_name().name() == "record" && child.attribute("type") == Some("html");
                walk(child, verbatim || html, items);
            }
            roxmltree::NodeType::Text => {
                let text = child.text().unwrap_or_default();
                // Whitespace between child elements is indentation. Alone in
                // an element it is content: `<attribute name="class"> </attribute>`
                // sets a space, an empty one removes the attribute.
                let blank = text.trim().is_empty();
                if !blank || mixed || !has_children || verbatim {
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

fn is_formatted_extension(extension: &std::ffi::OsStr) -> bool {
    matches!(extension.to_str(), Some("xml" | "scss" | "css" | "less"))
}

fn is_stylesheet(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()), Some("scss" | "css" | "less"))
}

/// Libraries a module ships as they are (`static/lib`, `static/src/lib`).
fn is_vendored(path: &Path) -> bool {
    let parts: Vec<&str> = path.iter().filter_map(|p| p.to_str()).collect();
    parts.windows(2).any(|w| w == ["static", "lib"]) || parts.windows(3).any(|w| w == ["static", "src", "lib"])
}

/// The XML files and stylesheets under `paths`, without excluded ones and
/// libraries.
pub fn files(paths: &[PathBuf], settings: &Settings) -> Vec<PathBuf> {
    crate::linter::collect_files_matching(paths, settings, is_formatted_extension)
        .into_iter()
        .filter(|p| !is_vendored(p))
        .collect()
}

/// The configuration lax-css gets.
pub fn css_configuration(line_width: u32, indent_width: u8) -> lax_css::configuration::Configuration {
    let mut map = ConfigKeyMap::new();
    map.insert("lineWidth".into(), ConfigKeyValue::from_i32(line_width as i32));
    map.insert("indentWidth".into(), ConfigKeyValue::from_i32(i32::from(indent_width)));
    lax_css::configuration::resolve_config(map, &GlobalConfiguration::default()).config
}

/// A stylesheet reduced to its tokens: whitespace runs as one space, none
/// next to `{`, `}`, `;` or `,`, nor after `:` (`color:red` is
/// `color: red`; before it, `a :hover` is not `a:hover`), nor inside
/// parentheses (`:not( .a )` is `:not(.a)`; before one, `url (a)` is not
/// `url(a)`). lax-css moves only whitespace; this checks
/// that it did not join or split two tokens.
fn css_tokens(source: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in source.chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space
            && !out.is_empty()
            && !out.ends_with(['{', '}', ';', ',', ':', '('])
            && !matches!(c, '{' | '}' | ';' | ',' | ')')
        {
            out.push(' ');
        }
        space = false;
        out.push(c);
    }
    out
}

/// Formats the stylesheet `source` of `path`.
pub fn format_stylesheet(path: &Path, source: &str, config: &lax_css::configuration::Configuration) -> Outcome {
    match lax_css::format_text(path, source, config) {
        Ok(None) => Outcome::Unchanged,
        // lax-css adds the `;` after a block's last declaration, as prettier
        // does: it changes nothing.
        Ok(Some(text)) if css_tokens(&text).replace(";}", "}") == css_tokens(source).replace(";}", "}") => {
            Outcome::Formatted(text)
        }
        Ok(Some(_)) => Outcome::Refused("formatting would change more than whitespace".to_string()),
        Err(error) => Outcome::Invalid(error.to_string()),
    }
}

/// Formats every XML file and stylesheet under `paths`; with `check`,
/// writes nothing.
/// Returns each file whose outcome is not `Unchanged`.
pub fn run(paths: &[PathBuf], settings: &Settings, check: bool) -> std::io::Result<Vec<(PathBuf, Outcome)>> {
    let format = &settings.config.format.clone().unwrap_or_default();
    let (width, indent) = (
        format.line_width.unwrap_or(DEFAULT_LINE_WIDTH),
        format.indent_width.unwrap_or(DEFAULT_INDENT_WIDTH),
    );
    let config = configuration(width, indent);
    let css = css_configuration(width, indent);
    let mut results = Vec::new();
    for path in files(paths, settings) {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let outcome = if is_stylesheet(&path) {
            format_stylesheet(&path, &source, &css)
        } else {
            format_source(&path, &source, &config)
        };
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

    /// The text after formatting: the source when the formatter leaves it.
    fn formatted(source: &str) -> String {
        match format(source) {
            Outcome::Formatted(text) => text,
            _ => source.to_string(),
        }
    }

    #[test]
    fn whitespace_alone_in_an_element_is_kept() {
        let source = "<odoo>\n<record id=\"v\" model=\"ir.ui.view\">\n<field name=\"arch\" type=\"xml\">\n<xpath expr=\".\" position=\"attributes\">\n<attribute name=\"class\"> </attribute>\n</xpath>\n</field>\n</record>\n</odoo>\n";
        assert!(formatted(source).contains("<attribute name=\"class\"> </attribute>"));
    }

    #[test]
    fn html_field_content_is_kept() {
        let field = "<field name=\"body_html\" type=\"html\"><p>a <b>b</b> c</p></field>";
        let source = format!("<odoo>\n<record id=\"m\" model=\"mail.template\">\n{field}\n</record>\n</odoo>\n");
        assert!(formatted(&source).contains(field));
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

#[cfg(test)]
mod stylesheet_tests {
    use super::*;

    #[test]
    fn scss_is_indented_and_vendored_files_are_skipped() {
        let out = format_stylesheet(
            Path::new("a.scss"),
            ".o_form_view {\n.o_field_widget{color:red;}\n  &:hover  { color: $primary; }\n}\n",
            &css_configuration(88, 4),
        );
        let Outcome::Formatted(text) = out else {
            panic!("{out:?}")
        };
        assert!(text.contains("\n    .o_field_widget {"), "{text}");
        assert_eq!(
            css_tokens(&text),
            css_tokens(".o_form_view{.o_field_widget{color:red;}&:hover{color: $primary;}}")
        );
        assert!(is_vendored(Path::new("acme/static/lib/x/x.css")));
        assert!(is_vendored(Path::new("acme/static/src/lib/x.scss")));
        assert!(!is_vendored(Path::new("acme/static/src/scss/x.scss")));
    }

    #[test]
    fn a_missing_last_semicolon_does_not_stop_formatting() {
        let out = format_stylesheet(Path::new("a.scss"), ".a { color: red }\n", &css_configuration(88, 4));
        assert!(matches!(out, Outcome::Formatted(_)), "{out:?}");
    }

    #[test]
    fn tokens_keep_combinators() {
        assert_ne!(css_tokens(".a .b{}"), css_tokens(".a.b{}"));
        assert_ne!(css_tokens("a :hover{}"), css_tokens("a:hover{}"));
        assert_eq!(css_tokens("a {\n  color: red ;\n}"), css_tokens("a{color:red;}"));
        assert_eq!(css_tokens("a:not(\n    .b\n) {}"), css_tokens("a:not(.b){}"));
        assert_ne!(css_tokens("a{b: url (x)}"), css_tokens("a{b: url(x)}"));
    }
}
