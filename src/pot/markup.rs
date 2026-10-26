//! The terms of XML and HTML values (`xml_translate` and `html_translate`
//! in odoo/tools/translate.py): a port of `translate_xml_node` that only
//! collects the terms, on a small tree with lxml's `text` and `tail`, and
//! lxml's serialization of the inline parts of a term.

use std::sync::LazyLock;

use fancy_regex::Regex;

/// How a term's inline elements are written: lxml's `method='xml'` or
/// `method='html'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Method {
    Xml,
    Html,
}

/// An element as lxml models it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Element {
    pub tag: String,
    pub attributes: Vec<(String, String)>,
    pub text: Option<String>,
    pub children: Vec<Child>,
}

/// A child node and the text after it (lxml's `tail`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Child {
    pub node: Node,
    pub tail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Node {
    Element(Element),
    Comment(String),
    /// A processing instruction: target and data.
    Instruction(String, String),
}

impl Element {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn has_attribute(&self, name: &str) -> bool {
        self.attribute(name).is_some()
    }

    /// The tree of a roxmltree element.
    pub(super) fn from_node(node: roxmltree::Node) -> Self {
        let mut element = Self {
            tag: qualified_name(node),
            attributes: node
                .attributes()
                .map(|a| (attribute_name(node, &a), a.value().to_string()))
                .collect(),
            text: None,
            children: Vec::new(),
        };
        for child in node.children() {
            let converted = match child.node_type() {
                roxmltree::NodeType::Text => {
                    let text = child.text().unwrap_or_default();
                    match element.children.last_mut() {
                        Some(last) => last.tail.get_or_insert_with(String::new).push_str(text),
                        None => element.text.get_or_insert_with(String::new).push_str(text),
                    }
                    continue;
                }
                roxmltree::NodeType::Element => Node::Element(Self::from_node(child)),
                roxmltree::NodeType::Comment => Node::Comment(child.text().unwrap_or_default().to_string()),
                roxmltree::NodeType::PI => {
                    let Some(pi) = child.pi() else { continue };
                    Node::Instruction(pi.target.to_string(), pi.value.unwrap_or_default().to_string())
                }
                roxmltree::NodeType::Root => continue,
            };
            element.children.push(Child {
                node: converted,
                tail: None,
            });
        }
        element
    }
}

/// `prefix:name` as the source writes it, for namespaced tags.
fn qualified_name(node: roxmltree::Node) -> String {
    let tag = node.tag_name();
    match tag.namespace().and_then(|ns| node.lookup_prefix(ns)) {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}:{}", tag.name()),
        _ => tag.name().to_string(),
    }
}

fn attribute_name(node: roxmltree::Node, attribute: &roxmltree::Attribute) -> String {
    match attribute.namespace().and_then(|ns| node.lookup_prefix(ns)) {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}:{}", attribute.name()),
        _ => attribute.name().to_string(),
    }
}

/// Elements whose content is never translated.
const SKIPPED_ELEMENTS: &[&str] = &["script", "style", "title"];

/// Elements translated inline, as part of the surrounding text.
const TRANSLATED_ELEMENTS: &[&str] = &[
    "abbr", "b", "bdi", "bdo", "br", "cite", "code", "data", "del", "dfn", "em", "font", "i", "ins", "kbd", "keygen",
    "mark", "math", "meter", "output", "progress", "q", "ruby", "s", "samp", "small", "span", "strong", "sub", "sup",
    "time", "u", "var", "wbr", "text", "select", "option",
];

/// Attributes of views that are translated (and their `t-attf-` forms).
const TRANSLATED_ATTRS: &[&str] = &[
    "string",
    "add-label",
    "help",
    "sum",
    "avg",
    "confirm",
    "placeholder",
    "alt",
    "title",
    "aria-label",
    "aria-keyshortcuts",
    "aria-placeholder",
    "aria-roledescription",
    "aria-valuetext",
    "value_label",
    "data-tooltip",
    "label",
    "confirm-label",
    "confirm-title",
    "cancel-label",
];

fn is_translatable_attrib(key: Option<&str>, node: &Element) -> bool {
    let Some(key) = key.filter(|k| !k.is_empty()) else {
        return false;
    };
    let attr = key.strip_prefix("t-attf-").unwrap_or(key);
    (!node.has_attribute("t-call") && TRANSLATED_ATTRS.contains(&attr)) || key.ends_with(".translate")
}

fn is_translatable_attrib_value(node: &Element) -> bool {
    let classes: Vec<&str> = node.attribute("class").unwrap_or("").split(' ').collect();
    let kind = node.attribute("type");
    (node.tag == "input" && kind.unwrap_or("text") == "text" && !classes.contains(&"datetimepicker-input"))
        || (node.tag == "input" && kind == Some("hidden") && classes.contains(&"o_translatable_input_hidden"))
}

fn is_translatable_attrib_text(node: &Element) -> bool {
    node.tag == "field" && node.attribute("widget").unwrap_or("") == "url"
}

/// Whether `text` has a character other than spaces (and `\u{FEFF}`).
fn nonspace(text: Option<&str>) -> bool {
    text.is_some_and(|t| t.chars().any(|c| !c.is_whitespace() && c != '\u{FEFF}'))
}

fn is_force_inline(node: &Element) -> bool {
    node.attribute("class")
        .unwrap_or("")
        .split_whitespace()
        .any(|c| c == "o_translate_inline")
}

/// Whether a child can be translated as part of the surrounding text.
fn translatable(child: &Node, force_inline: bool) -> bool {
    let Node::Element(node) = child else {
        // A comment or an instruction has no tag lxml would know.
        return force_inline;
    };
    let force_inline = force_inline || is_force_inline(node);
    (force_inline || TRANSLATED_ELEMENTS.contains(&node.tag.as_str()))
        && !node
            .attributes
            .iter()
            .any(|(k, _)| k.starts_with("t-") || k == "groups" || k.ends_with(".translate"))
        && node.children.iter().all(|c| translatable(&c.node, force_inline))
}

/// The text before child `pos` (the parent's text for the first one).
fn text_before(node: &Element, pos: usize) -> Option<&str> {
    if pos == 0 {
        node.text.as_deref()
    } else {
        node.children[pos - 1].tail.as_deref()
    }
}

/// Whether there is text to translate at child position `pos`.
fn hastext(node: &Element, pos: usize, force_inline: bool) -> bool {
    let force_inline = force_inline || is_force_inline(node);
    if nonspace(text_before(node, pos)) {
        return true;
    }
    let Some(child) = node.children.get(pos) else {
        return false;
    };
    if !translatable(&child.node, force_inline) {
        return false;
    }
    let Node::Element(element) = &child.node else {
        return hastext(node, pos + 1, force_inline);
    };
    // Odoo checks the parent's attributes here (`is_translatable_attrib(key,
    // node)`), not the child's.
    element.attributes.iter().any(|(key, value)| {
        !value.is_empty()
            && (is_translatable_attrib(Some(key), node)
                || (key == "value" && is_translatable_attrib_value(element))
                || (key == "text" && is_translatable_attrib_text(element)))
    }) || hastext(element, 0, force_inline)
        || hastext(node, pos + 1, force_inline)
}

/// `#{...}` and `{{...}}` in a format string attribute.
static FORMAT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:#\{(.+?)\})|(?:\{\{(.+?)\}\})").expect("valid regex"));

/// The term of a `t-attf-` value: its expressions become `{{0}}`, `{{1}}`...
fn format_string_term(value: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    for (index, found) in FORMAT_REGEX.find_iter(value).flatten().enumerate() {
        out.push_str(&value[last..found.start()]);
        out.push_str(&format!("{{{{{index}}}}}"));
        last = found.end();
    }
    out.push_str(&value[last..]);
    out
}

/// `<!DOCTYPE` at the start of the root's text keeps it untranslated.
fn starts_with_doctype(text: Option<&str>) -> bool {
    text.unwrap_or("")
        .trim_start()
        .get(..9)
        .is_some_and(|start| start.eq_ignore_ascii_case("<!DOCTYPE"))
}

/// The terms of `root` as `translate_xml_node` finds them, in order.
pub(super) fn terms(root: &Element, method: Method) -> Vec<String> {
    let mut found = Vec::new();
    process(root, true, method, &mut found);
    found
}

fn process(node: &Element, is_root: bool, method: Method, found: &mut Vec<String>) {
    if SKIPPED_ELEMENTS.contains(&node.tag.as_str())
        || node.attribute("t-translation").is_some_and(|v| v.trim() == "off")
        || (node.tag == "attribute"
            && !matches!(node.attribute("name"), Some("value" | "text"))
            && !is_translatable_attrib(node.attribute("name"), node))
        || (is_root && starts_with_doctype(node.text.as_deref()))
    {
        return;
    }
    let mut pos = 0;
    loop {
        if hastext(node, pos, false) {
            // The text and the inline children from `pos` form one term.
            let force_inline = is_force_inline(node);
            let mut end = pos;
            while end < node.children.len() && translatable(&node.children[end].node, force_inline) {
                end += 1;
            }
            let mut content = escape_text(text_before(node, pos).unwrap_or(""), method);
            for child in &node.children[pos..end] {
                serialize_child(child, method, &mut content);
            }
            found.push(python_strip(&content).to_string());
            pos = end;
        }
        let Some(child) = node.children.get(pos) else { break };
        if let Node::Element(element) = &child.node {
            process(element, false, method, found);
        }
        pos += 1;
    }
    for (key, value) in &node.attributes {
        if !nonspace(Some(value)) {
            continue;
        }
        if is_translatable_attrib(Some(key), node)
            || (key == "value" && is_translatable_attrib_value(node))
            || (key == "text" && is_translatable_attrib_text(node))
        {
            let value = python_strip(value);
            found.push(if key.starts_with("t-") {
                format_string_term(value)
            } else {
                value.to_string()
            });
        }
    }
}

/// Python's `str.strip()`.
fn python_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// Elements HTML writes without an end tag.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "basefont", "br", "col", "embed", "frame", "hr", "img", "input", "isindex", "link", "meta",
    "param", "source", "track", "wbr",
];

fn escape_text(text: &str, method: Method) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' if method == Method::Xml => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
    out
}

/// An attribute value with its quotes, as libxml2 writes it: XML always in
/// double quotes; HTML in single quotes when the value has a `"` but no `'`
/// (`xmlBufWriteQuotedString`).
fn quoted_attribute(value: &str, method: Method) -> String {
    let single = method == Method::Html && value.contains('"') && !value.contains('\'');
    let quote = if single { '\'' } else { '"' };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for c in value.chars() {
        match (c, method) {
            ('&', _) => out.push_str("&amp;"),
            ('"', _) if !single => out.push_str("&quot;"),
            ('<', _) => out.push_str("&lt;"),
            ('>', _) => out.push_str("&gt;"),
            ('\n', Method::Xml) => out.push_str("&#10;"),
            ('\r', Method::Xml) => out.push_str("&#13;"),
            ('\t', Method::Xml) => out.push_str("&#9;"),
            _ => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// A child with its tail, as lxml's `tostring` writes it.
fn serialize_child(child: &Child, method: Method, out: &mut String) {
    match &child.node {
        Node::Element(element) => serialize_element(element, method, out),
        Node::Comment(text) => {
            out.push_str("<!--");
            out.push_str(text);
            out.push_str("-->");
        }
        Node::Instruction(target, data) => {
            out.push_str("<?");
            out.push_str(target);
            if !data.is_empty() {
                out.push(' ');
                out.push_str(data);
            }
            out.push_str(if method == Method::Html { ">" } else { "?>" });
        }
    }
    if let Some(tail) = &child.tail {
        out.push_str(&escape_text(tail, method));
    }
}

fn serialize_element(element: &Element, method: Method, out: &mut String) {
    out.push('<');
    out.push_str(&element.tag);
    for (key, value) in &element.attributes {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&quoted_attribute(value, method));
    }
    let empty = element.text.as_deref().unwrap_or("").is_empty() && element.children.is_empty();
    match method {
        Method::Xml if empty => {
            out.push_str("/>");
            return;
        }
        Method::Html if VOID_ELEMENTS.contains(&element.tag.to_lowercase().as_str()) => {
            out.push('>');
            return;
        }
        _ => out.push('>'),
    }
    if let Some(text) = &element.text {
        out.push_str(&escape_text(text, method));
    }
    for child in &element.children {
        serialize_child(child, method, out);
    }
    out.push_str("</");
    out.push_str(&element.tag);
    out.push('>');
}

/// The element `source` holds (a `<field>` or a `<template>` of a data file).
pub(super) fn parse_element(source: &str) -> Option<Element> {
    let doc = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        },
    )
    .ok()?;
    Some(Element::from_node(doc.root_element()))
}

impl Element {
    /// The loader's `%%` to `%` in the texts and attributes of an `xml` or
    /// `html` field (its `%(xml_id)d` references become database ids, which
    /// no term should hold).
    pub(super) fn unescape_percents(&mut self) {
        let fix = |text: &mut String| {
            if text.contains("%%") {
                *text = text.replace("%%", "%");
            }
        };
        self.text.iter_mut().for_each(fix);
        self.attributes.iter_mut().for_each(|(_, value)| fix(value));
        for child in &mut self.children {
            child.tail.iter_mut().for_each(fix);
            match &mut child.node {
                Node::Element(element) => element.unescape_percents(),
                Node::Comment(text) => fix(text),
                Node::Instruction(..) => {}
            }
        }
    }
}

/// The children of `element` with their tails, as the loader joins them for
/// a `type="xml"` or `type="html"` field (the text before them is dropped).
pub(super) fn serialize_children(element: &Element, method: Method) -> String {
    let mut out = String::new();
    for child in &element.children {
        serialize_child(child, method, &mut out);
    }
    out
}

/// The arch an `arch` field sets: its one element, or a `<data>` around
/// several (`_fix_multiple_roots`).
pub(super) fn arch_of_field(field: Element) -> Element {
    let elements = field
        .children
        .iter()
        .filter(|c| matches!(c.node, Node::Element(_)))
        .count();
    if elements == 1 {
        if let Some(element) = field.children.iter().find_map(|c| match &c.node {
            Node::Element(element) => Some(element.clone()),
            _ => None,
        }) {
            return element;
        }
    }
    Element {
        tag: "data".into(),
        attributes: Vec::new(),
        text: None,
        children: field.children,
    }
}

/// The `<div>` `html_translate` wraps an HTML field's value in.
pub(super) fn html_of_field(field: Element) -> Element {
    Element {
        tag: "div".into(),
        attributes: Vec::new(),
        text: None,
        children: field.children,
    }
}

/// The root of a view's arch from the content of its `arch` field: its
/// element, or a `<data>` around several (`_fix_multiple_roots`).
pub(super) fn arch_root(content: &str) -> Option<Element> {
    let wrapped = format!("<data>{content}</data>");
    let doc = roxmltree::Document::parse_with_options(
        &wrapped,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        },
    )
    .ok()?;
    let wrapper = Element::from_node(doc.root_element());
    let elements = wrapper
        .children
        .iter()
        .filter(|c| matches!(c.node, Node::Element(_)))
        .count();
    if elements == 1 {
        wrapper.children.into_iter().find_map(|c| match c.node {
            Node::Element(element) => Some(element),
            _ => None,
        })
    } else {
        Some(Element { text: None, ..wrapper })
    }
}

/// The root of an HTML value: a `<div>` around it, as `html_translate`
/// parses it.
pub(super) fn html_root(content: &str) -> Option<Element> {
    let wrapped = format!("<div>{content}</div>");
    let doc = roxmltree::Document::parse_with_options(
        &wrapped,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        },
    )
    .ok()?;
    Some(Element::from_node(doc.root_element()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xml(content: &str) -> Vec<String> {
        terms(&arch_root(content).expect("valid XML"), Method::Xml)
    }

    #[test]
    fn the_loader_unescapes_percent_signs() {
        let mut root = arch_root("<p>50%% of %%(x)s</p>").expect("valid XML");
        root.unescape_percents();
        assert_eq!(terms(&root, Method::Xml), ["50% of %(x)s"]);
    }

    #[test]
    fn a_text_with_inline_elements_is_one_term() {
        assert_eq!(
            xml("<div><p>Hello <b>dear</b> world</p></div>"),
            ["Hello <b>dear</b> world"]
        );
    }

    #[test]
    fn block_elements_split_terms() {
        assert_eq!(
            xml("<div>First<div>Second</div>Third</div>"),
            ["First", "Second", "Third"]
        );
    }

    #[test]
    fn translated_attributes_are_terms() {
        assert_eq!(
            xml(r#"<form string="Order"><field name="x" placeholder="Name..."/></form>"#),
            ["Name...", "Order"]
        );
    }

    #[test]
    fn empty_inline_elements_close_themselves_in_xml() {
        assert_eq!(xml("<p>Line<br/>Next</p>"), ["Line<br/>Next"]);
    }

    #[test]
    fn html_writes_void_elements_without_end_tags() {
        let root = html_root("<p>Line<br/>Next</p>").expect("valid");
        assert_eq!(terms(&root, Method::Html), ["Line<br>Next"]);
    }

    #[test]
    fn html_quotes_an_attribute_with_double_quotes_in_single_ones() {
        let root = html_root(r#"<p>Say <span title='He said "hi" &lt;bye&gt;'>x</span></p>"#).expect("valid");
        assert_eq!(
            terms(&root, Method::Html),
            [r#"Say <span title='He said "hi" &lt;bye&gt;'>x</span>"#]
        );
    }

    #[test]
    fn text_is_escaped_as_lxml_does() {
        assert_eq!(xml("<p>A &amp; B &gt; C</p>"), ["A &amp; B &gt; C"]);
    }

    #[test]
    fn elements_with_directives_are_not_inline() {
        assert_eq!(xml(r#"<p>Total: <span t-esc="amount"/> due</p>"#), ["Total:", "due"]);
    }

    #[test]
    fn format_string_attributes_hide_their_expressions() {
        assert_eq!(
            xml(r##"<a t-attf-title="Open #{record.name}">x</a>"##),
            ["x", "Open {{0}}"]
        );
    }

    #[test]
    fn untranslated_elements_are_skipped() {
        assert_eq!(xml(r#"<div><p t-translation="off">No</p><p>Yes</p></div>"#), ["Yes"]);
    }

    #[test]
    fn several_roots_get_a_data_root() {
        assert_eq!(xml("<p>One</p><p>Two</p>"), ["One", "Two"]);
    }
}
