//! Upgrade rules: what breaks or silently stops working when a module moves
//! to a newer Odoo version. Codes are `U` + the target version + a number
//! (`U1801` is a change of 18.0), so `--select U18` checks a module for 18.0
//! and `odl upgrade-check` picks them up per version step.
//!
//! Sources: Odoo's own code-upgrade scripts (`odoo/upgrade_code`) and the
//! differences in Odoo's view schemas, validators and models between
//! branches. Reimplemented from behaviour, not copied.

pub mod v18;

use crate::fix::Edit;
use crate::rules::xml::{child_field, is};
use crate::xml::XmlFile;
use regex::Regex;
use roxmltree::Node;

/// Elements `names` in a data file. Odoo loads `<odoo>`, `<openerp>` and
/// `<data>` as the root element.
pub(crate) fn data_elements<'a, 'input>(
    file: &'a XmlFile<'input>,
    names: &'a [&'a str],
) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    file.root()
        .filter(|r| ["odoo", "openerp", "data"].iter().any(|name| is(*r, name)))
        .into_iter()
        .flat_map(|root| root.descendants().skip(1))
        .filter(move |n| names.iter().any(|name| is(*n, name)))
}

/// The `arch` field of every `ir.ui.view` record, with the record.
pub(crate) fn view_archs<'a, 'input>(
    file: &'a XmlFile<'input>,
) -> impl Iterator<Item = (Node<'a, 'input>, Node<'a, 'input>)> + 'a {
    data_elements(file, &["record"])
        .filter(|r| r.attribute("model") == Some("ir.ui.view"))
        .filter_map(|r| child_field(r, "arch").map(|arch| (r, arch)))
}

/// Elements inside the arch of a view, or inside a `<template>`.
pub(crate) fn arch_elements<'a, 'input>(file: &'a XmlFile<'input>) -> Vec<Node<'a, 'input>> {
    let mut nodes: Vec<Node> = view_archs(file)
        .flat_map(|(_, arch)| arch.descendants().skip(1).filter(Node::is_element))
        .collect();
    nodes.extend(data_elements(file, &["template"]).flat_map(|t| t.descendants().skip(1).filter(Node::is_element)));
    nodes
}

/// Renames an element's tag: its start tag, and its end tag when it has one.
pub(crate) fn rename_tag(file: &XmlFile, node: Node, new: &str) -> Option<Vec<Edit>> {
    let old = node.tag_name().name();
    let range = node.range();
    let source = file.source;
    let start = range.start + 1;
    if source.get(start..start + old.len()) != Some(old) {
        return None;
    }
    let mut edits = vec![Edit::replace(start, start + old.len(), new)];
    if !source[..range.end].ends_with("/>") {
        let close = source[range.start..range.end].rfind("</")? + range.start + 2;
        if source.get(close..close + old.len()) != Some(old) {
            return None;
        }
        edits.push(Edit::replace(close, close + old.len(), new));
    }
    Some(edits)
}

/// An edit applying `pattern` -> `replacement` to the text of an attribute's
/// value or a text node, as it is in the file; `None` when nothing changes.
pub(crate) fn replace_in(
    file: &XmlFile,
    range: std::ops::Range<usize>,
    pattern: &Regex,
    replacement: &str,
) -> Option<Edit> {
    let text = file.source.get(range.clone())?;
    let new = pattern.replace_all(text, replacement);
    (new != text).then(|| Edit::replace(range.start, range.end, new.into_owned()))
}

/// The text node of an element with a single text child.
pub(crate) fn text_range(node: Node) -> Option<std::ops::Range<usize>> {
    let mut children = node.children();
    let text = children.next().filter(Node::is_text)?;
    children.next().is_none().then(|| text.range())
}

/// Whether `node` is the `<field name="...">` `name` of a record of one of `models`.
pub(crate) fn record_field(node: Node, models: &[&str], name: &str) -> bool {
    is(node, "field")
        && node.attribute("name") == Some(name)
        && node
            .parent_element()
            .is_some_and(|r| is(r, "record") && r.attribute("model").is_some_and(|m| models.contains(&m)))
}
