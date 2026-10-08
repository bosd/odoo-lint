//! XML data files: checks from OCA's `oca-checks-odoo-module` (XML001-XML024,
//! same names, same results on its test repository) and odoo-lint's own
//! upgrade checks (XML101+).
//!
//! The checks run per module, on the XML files its manifest loads, and are
//! gated on the module's own Odoo version (from its manifest), as OCA does.

use crate::checker::ModuleInfo;
use crate::diagnostics::Violation;
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::rules::{Check, Rule};
use crate::settings::Settings;
use crate::sources::Sources;
use crate::xml::{self, XmlFile};
use fancy_regex::Regex as FancyRegex;
use regex::Regex;
use roxmltree::Node;
use std::collections::HashMap;
use std::sync::LazyLock;

pub struct XmlContext<'a> {
    pub module: &'a ModuleInfo,
    pub files: &'a [XmlFile<'a>],
    pub settings: &'a Settings,
}

#[derive(Default)]
pub struct XmlReporter {
    pub violations: Vec<Violation>,
}

impl XmlReporter {
    pub fn report(&mut self, rule: &Rule, file: &XmlFile, offset: usize, message: impl Into<String>) -> &mut Violation {
        let (line, column) = file.line_column(offset);
        self.violations.push(Violation {
            file_path: file.display.clone(),
            line,
            column,
            code: rule.code.to_string(),
            name: rule.name.to_string(),
            message: message.into(),
            fix: None,
        });
        self.violations.last_mut().expect("just pushed")
    }
}

/// Runs the XML rules on the files `module`'s manifest loads.
pub fn lint_module(module: &ModuleInfo, rules: &[&Rule], settings: &Settings, sources: &Sources) -> Vec<Violation> {
    let entries = xml::manifest_xml_files(module);
    if entries.is_empty() || rules.is_empty() {
        return Vec::new();
    }
    let texts = xml::read_entries(&entries, sources);
    let files: Vec<XmlFile> = entries
        .iter()
        .zip(&texts)
        .map(|(entry, text)| match text {
            Ok(source) => XmlFile::parse(&entry.path, &entry.section, Some(source), None),
            Err(error) => XmlFile::parse(&entry.path, &entry.section, None, Some(error.clone())),
        })
        .collect();
    let version = xml::module_version(module, settings.target_version);
    let ctx = XmlContext {
        module,
        files: &files,
        settings,
    };
    let mut reporter = XmlReporter::default();
    // Some rules share a check function, which reports for each of them.
    let mut done: Vec<usize> = Vec::new();
    for rule in rules {
        if let Check::Xml(check) = rule.check {
            let key = check as usize;
            let gated = rule.min_odoo.is_some() || rule.max_odoo.is_some();
            let applies = match version {
                Some(version) => rule.applies_to(version),
                None => !gated,
            };
            if applies && !done.contains(&key) {
                done.push(key);
                check(&ctx, &mut reporter);
            }
        }
    }
    // `<!-- oca-hooks:disable=... -->` turns checks off for the whole file.
    reporter.violations.retain(|v| {
        files
            .iter()
            .filter(|f| f.display == v.file_path)
            .all(|f| !f.disabled.contains(&v.name) && !f.disabled.contains(&v.code))
    });
    reporter.violations
}

// --- Helpers -------------------------------------------------------------

/// Whether `node` is an element `name` without a namespace, as an XPath name
/// test matches.
fn is(node: Node, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace().is_none() && node.tag_name().name() == name
}

/// The root element when it is `<odoo>` or `<openerp>`.
fn odoo_root<'a, 'input>(file: &'a XmlFile<'input>) -> Option<Node<'a, 'input>> {
    file.root().filter(|r| is(*r, "odoo") || is(*r, "openerp"))
}

/// Elements `name` below the `<odoo>`/`<openerp>` root, in document order.
fn under_root<'a, 'input>(
    file: &'a XmlFile<'input>,
    names: &'a [&'a str],
) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    odoo_root(file)
        .into_iter()
        .flat_map(|root| root.descendants().skip(1))
        .filter(move |n| names.iter().any(|name| is(*n, name)))
}

/// Direct child elements `name`.
fn children<'a, 'input>(node: Node<'a, 'input>, name: &'a str) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    node.children().filter(move |c| is(*c, name))
}

/// The first direct child `<field name="...">`.
fn child_field<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|c| is(*c, "field") && c.attribute("name") == Some(name))
}

fn has_class(node: Node, class: &str) -> bool {
    node.attribute("class")
        .is_some_and(|classes| classes.split_whitespace().any(|c| c == class))
}

/// Python's `int()` of a string, as OCA reads priorities; `None` when invalid.
fn py_int(text: &str) -> Option<i64> {
    let text = text.trim();
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    // Underscores may separate digits.
    let valid = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.chars().all(|c| c.is_ascii_digit() || c == '_');
    valid.then(|| digits.replace('_', "").parse::<i64>().ok().map(|n| sign * n))?
}

/// Python's `str.title()`.
fn py_title(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_cased = false;
    for c in text.chars() {
        if c.is_alphabetic() {
            if previous_cased {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            previous_cased = true;
        } else {
            out.push(c);
            previous_cased = false;
        }
    }
    out
}

/// The record key of a duplicate check: the manifest section, the id and the
/// enclosing `noupdate`, as OCA groups them (`None` for a missing id).
fn xmlid_key(file: &XmlFile, node: Node) -> String {
    let id = node.attribute("id").unwrap_or("None");
    let noupdate = node
        .parent_element()
        .and_then(|p| p.attribute("noupdate"))
        .unwrap_or("0");
    format!("{}/{id}_noupdate_{noupdate}", file.section)
}

/// Other places of a duplicate, for the message.
fn also_at(places: &[(&XmlFile, Node)]) -> String {
    places
        .iter()
        .map(|(file, node)| format!("{}:{}", file.display, file.line_column(at(file, *node)).0))
        .collect::<Vec<_>>()
        .join(", ")
}

/// End of an element's start tag (after `>` or `/>`).
fn start_tag_end(file: &XmlFile, node: Node) -> usize {
    let after = node
        .attributes()
        .map(|a| a.range().end)
        .max()
        .unwrap_or(node.range().start + 1);
    file.source[after..]
        .find('>')
        .map_or(node.range().end, |i| after + i + 1)
}

/// Where to report an element: the line its start tag ends on, as lxml
/// (libxml2) numbers elements, at the first character of that line.
fn at(file: &XmlFile, node: Node) -> usize {
    let tag_end = start_tag_end(file, node).saturating_sub(1);
    let line_start = file.source[..tag_end].rfind('\n').map_or(0, |i| i + 1);
    let indent = file.source[line_start..tag_end].len() - file.source[line_start..tag_end].trim_start().len();
    if line_start > node.range().start {
        line_start + indent
    } else {
        node.range().start
    }
}

/// Deletes an element together with its line when nothing else is on it.
fn delete_element(file: &XmlFile, node: Node) -> Edit {
    let range = node.range();
    let source = file.source;
    let line_start = source[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let rest = &source[range.end..];
    let trailing = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let after = &rest[trailing..];
    if source[line_start..range.start].trim().is_empty() && (after.starts_with('\n') || after.starts_with("\r\n")) {
        let newline = if after.starts_with('\n') { 1 } else { 2 };
        Edit::delete(line_start, range.end + trailing + newline)
    } else {
        Edit::delete(range.start, range.end)
    }
}

// --- Rules ----------------------------------------------------------------

const DEFAULT_MIN_PRIORITY: i64 = 99;
const XML_HEADER: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>"#;
static XML_HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^<\?xml[^>]*\?>").unwrap());

pub const XML_SYNTAX_ERROR: Rule = Rule {
    code: "XML001",
    name: "xml-syntax-error",
    summary: "An XML file the manifest loads cannot be read or parsed.",
    doc: r#"
## What it does

Reports XML files listed in the manifest that are missing, not valid UTF-8 or
not well-formed XML.

## Why is this bad?

Odoo fails to install or update the module when it loads the file.
"#,
    check: Check::Xml(check_syntax_error),
    min_odoo: None,
    max_odoo: None,
};

fn check_syntax_error(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        if let Some(error) = &file.error {
            reporter.report(&XML_SYNTAX_ERROR, file, 0, error.clone());
        }
    }
}

pub const XML_HEADER_MISSING: Rule = Rule {
    code: "XML002",
    name: "xml-header-missing",
    summary: "An XML file has no `<?xml ... ?>` declaration.",
    doc: r#"
## What it does

Reports XML files that do not start with the XML declaration
`<?xml version="1.0" encoding="UTF-8" ?>`.

## Why is this bad?

Without it, editors and tools have to guess the encoding, and the files of a
module look different from each other.

## Fix safety

Safe: the declaration is added as the first line.
"#,
    check: Check::Xml(check_header),
    min_odoo: None,
    max_odoo: None,
};

pub const XML_HEADER_WRONG: Rule = Rule {
    code: "XML003",
    name: "xml-header-wrong",
    summary: "The XML declaration is not `<?xml version=\"1.0\" encoding=\"UTF-8\" ?>`.",
    doc: r#"
## What it does

Reports XML declarations that differ from
`<?xml version="1.0" encoding="UTF-8" ?>`, e.g. in quotes, case or spacing.

## Why is this bad?

One declaration for all files keeps diffs and tooling simple.

## Fix safety

Safe: the declaration is replaced.
"#,
    check: Check::Xml(check_header),
    min_odoo: None,
    max_odoo: None,
};

fn check_header(ctx: &XmlContext, reporter: &mut XmlReporter) {
    let missing = ctx.settings.is_selected(&XML_HEADER_MISSING);
    let wrong = ctx.settings.is_selected(&XML_HEADER_WRONG);
    for file in ctx.files {
        let Some((tag, line)) = &file.first_tag else { continue };
        if !tag.starts_with("<?xml ") {
            if missing {
                reporter.report(&XML_HEADER_MISSING, file, 0, "XML missing header").fix = Some(Fix::safe(
                    "Add the XML declaration",
                    vec![Edit::insert(0, format!("{XML_HEADER}\n"))],
                ));
            }
        } else if wrong && XML_HEADER_RE.replace(tag, XML_HEADER) != tag.as_str() {
            let fix = XML_HEADER_RE.find(file.source).map(|found| {
                Fix::safe(
                    "Use the standard XML declaration",
                    vec![Edit::replace(found.start(), found.end(), XML_HEADER)],
                )
            });
            reporter
                .report(
                    &XML_HEADER_WRONG,
                    file,
                    file.line_start(*line),
                    format!("XML header expected '{XML_HEADER}' but received '{tag}'"),
                )
                .fix = fix;
        }
    }
}

pub const XML_RECORD_MISSING_ID: Rule = Rule {
    code: "XML004",
    name: "xml-record-missing-id",
    summary: "A `<record>` or `<menuitem>` has no `id`.",
    doc: r#"
## What it does

Reports `<record>` and `<menuitem>` elements without an `id`.

## Why is this bad?

A record without an XML id is created again on every module update, and
nothing else can refer to it. Give it a unique id to create a record, or an
existing one to update it.
"#,
    check: Check::Xml(check_record_missing_id),
    min_odoo: None,
    max_odoo: None,
};

const RECORD_TAGS: &[&str] = &["record", "menuitem"];

fn check_record_missing_id(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for record in under_root(file, RECORD_TAGS) {
            if record.attribute("id").is_none_or(str::is_empty) {
                reporter.report(
                    &XML_RECORD_MISSING_ID,
                    file,
                    at(file, record),
                    "Record has no id, add a unique one to create a new record, use an existing one to update it",
                );
            }
        }
    }
}

pub const XML_DUPLICATE_RECORD_ID: Rule = Rule {
    code: "XML005",
    name: "xml-duplicate-record-id",
    summary: "Two records of a module have the same XML id.",
    doc: r#"
## What it does

Reports XML ids defined more than once in a module's data files (in the same
manifest section and `noupdate` mode).

## Why is this bad?

The second definition silently overwrites the first one.
"#,
    check: Check::Xml(check_duplicate_record_id),
    min_odoo: None,
    max_odoo: None,
};

fn check_duplicate_record_id(ctx: &XmlContext, reporter: &mut XmlReporter) {
    let mut groups: Vec<(String, Vec<(&XmlFile, Node)>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for file in ctx.files {
        if file.disabled.contains(XML_DUPLICATE_RECORD_ID.name) {
            continue;
        }
        for record in under_root(file, RECORD_TAGS) {
            let key = xmlid_key(file, record);
            match index.get(&key) {
                Some(&i) => groups[i].1.push((file, record)),
                None => {
                    index.insert(key.clone(), groups.len());
                    groups.push((key, vec![(file, record)]));
                }
            }
        }
    }
    for (_, places) in groups.iter().filter(|(_, places)| places.len() > 1) {
        let (file, record) = places[0];
        let id = record.attribute("id").unwrap_or("None");
        reporter.report(
            &XML_DUPLICATE_RECORD_ID,
            file,
            at(file, record),
            format!("Duplicate xml record id `{id}` (also at {})", also_at(&places[1..])),
        );
    }
}

pub const XML_DUPLICATE_FIELDS: Rule = Rule {
    code: "XML006",
    name: "xml-duplicate-fields",
    summary: "A record sets the same field twice.",
    doc: r#"
## What it does

Reports `<field name="...">` elements repeated within one record.

## Why is this bad?

Only the last value is kept; the other one is dead code that looks alive.
"#,
    check: Check::Xml(check_duplicate_fields),
    min_odoo: None,
    max_odoo: None,
};

fn check_duplicate_fields(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for record in under_root(file, RECORD_TAGS) {
            let mut by_name: Vec<(&str, Vec<Node>)> = Vec::new();
            for field in children(record, "field") {
                let Some(name) = field.attribute("name") else { continue };
                match by_name.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, fields)) => fields.push(field),
                    None => by_name.push((name, vec![field])),
                }
            }
            for (name, fields) in by_name.iter().filter(|(_, f)| f.len() > 1) {
                let others: Vec<(&XmlFile, Node)> = fields[1..].iter().map(|f| (file, *f)).collect();
                reporter.report(
                    &XML_DUPLICATE_FIELDS,
                    file,
                    at(file, fields[0]),
                    format!("Duplicate xml field `{name}` (also at {})", also_at(&others)),
                );
            }
        }
    }
}

pub const XML_DUPLICATE_TEMPLATE_ID: Rule = Rule {
    code: "XML007",
    name: "xml-duplicate-template-id",
    summary: "Two templates of a module have the same id.",
    doc: r#"
## What it does

Reports `<template>` ids defined more than once in a module's data files (in
the same manifest section and `noupdate` mode).

## Why is this bad?

The second template silently replaces the first one.
"#,
    check: Check::Xml(check_duplicate_template_id),
    min_odoo: None,
    max_odoo: None,
};

fn check_duplicate_template_id(ctx: &XmlContext, reporter: &mut XmlReporter) {
    let mut groups: Vec<(String, Vec<(&XmlFile, Node)>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for file in ctx.files {
        if file.disabled.contains(XML_DUPLICATE_TEMPLATE_ID.name) {
            continue;
        }
        for template in under_root(file, &["template"]) {
            if template.attribute("id").is_none_or(str::is_empty) {
                continue;
            }
            let key = xmlid_key(file, template);
            match index.get(&key) {
                Some(&i) => groups[i].1.push((file, template)),
                None => {
                    index.insert(key.clone(), groups.len());
                    groups.push((key, vec![(file, template)]));
                }
            }
        }
    }
    for (key, places) in groups.iter().filter(|(_, places)| places.len() > 1) {
        let (file, template) = places[0];
        reporter.report(
            &XML_DUPLICATE_TEMPLATE_ID,
            file,
            at(file, template),
            format!("Duplicate xml template id `{key}` (also at {})", also_at(&places[1..])),
        );
    }
}

pub const XML_REDUNDANT_MODULE_NAME: Rule = Rule {
    code: "XML008",
    name: "xml-redundant-module-name",
    summary: "A record id repeats the name of its own module.",
    doc: r#"
## What it does

Reports `id="module.name"` in module `module`'s own records.

## Why is this bad?

The prefix is implied, and it breaks when the module is renamed.

## Example

```xml
<record id="acme_sale.view_order_form" model="ir.ui.view">
```

Use instead:

```xml
<record id="view_order_form" model="ir.ui.view">
```

## Fix safety

Safe: the prefix is removed; the XML id stays the same.
"#,
    check: Check::Xml(check_redundant_module_name),
    min_odoo: None,
    max_odoo: None,
};

fn check_redundant_module_name(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for record in under_root(file, RECORD_TAGS) {
            let Some(attribute) = record
                .attributes()
                .find(|a| a.name() == "id" && a.namespace().is_none())
            else {
                continue;
            };
            let id = attribute.value();
            let Some((module, name)) = id.split_once('.') else {
                continue;
            };
            if module != ctx.module.name || name.contains('.') {
                continue;
            }
            let tag = record.tag_name().name();
            let range = attribute.range_value();
            reporter
                .report(
                    &XML_REDUNDANT_MODULE_NAME,
                    file,
                    at(file, record),
                    format!("Redundant module name `<{tag} id=\"{id}\"`"),
                )
                .fix = Some(Fix::safe(
                format!("Use `id=\"{name}\"`"),
                vec![Edit::replace(range.start, range.end, name)],
            ));
        }
    }
}

pub const XML_TAG_POSITION: Rule = Rule {
    code: "XML009",
    name: "xml-tag-position",
    summary: "`t-if`, `id` or `class` attributes are not first in a tag.",
    doc: r#"
## What it does

Checks the order of attributes in a tag: the conditions (`t-if`, `t-else`,
`t-elif`) first, then the ids (`id`, `t-att-id`, `t-attf-id`), then the
classes (`class`, `t-att-class`, `t-attf-class`). It applies when a tag has
more than one of them, and to every `<record>`, `<menuitem>` and `<template>`.

## Why is this bad?

When the attributes that decide whether and what an element is come first,
templates are easier to read and to review.

## Example

```xml
<div class="o_row" t-if="record.active">
```

Use instead:

```xml
<div t-if="record.active" class="o_row">
```

## Fix safety

Safe: the attributes are moved; values and layout are kept.
"#,
    check: Check::Xml(check_tag_position),
    min_odoo: None,
    max_odoo: None,
};

const ATTRIBUTE_ORDER: &[&[&str]] = &[
    &["t-if", "t-else", "t-elif"],
    &["id", "t-att-id", "t-attf-id"],
    &["class", "t-att-class", "t-attf-class"],
];

fn check_tag_position(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in file.elements() {
            let attributes: Vec<_> = node.attributes().collect();
            if attributes.is_empty() {
                continue;
            }
            // The attribute names as lxml lists them, `{namespace}name` included.
            let keys: Vec<String> = attributes
                .iter()
                .map(|a| match a.namespace() {
                    Some(ns) => format!("{{{ns}}}{}", a.name()),
                    None => a.name().to_string(),
                })
                .collect();
            let expected: Vec<&str> = ATTRIBUTE_ORDER
                .iter()
                .flat_map(|group| group.iter().copied())
                .filter(|name| keys.iter().any(|k| k == name))
                .collect();
            let special = ["record", "menuitem", "template"].iter().any(|t| is(node, t));
            if !(expected.len() > 1 || (expected.len() == 1 && special)) {
                continue;
            }
            if keys
                .iter()
                .take(expected.len())
                .map(String::as_str)
                .eq(expected.iter().copied())
            {
                continue;
            }
            let shown: Vec<String> = expected
                .iter()
                .map(|name| format!("{name}=\"{}\"", node.attribute(*name).unwrap_or_default()))
                .collect();
            let tag = node.tag_name().name();
            // Same slots, new order: the text between attributes stays.
            let mut order: Vec<usize> = expected
                .iter()
                .filter_map(|name| keys.iter().position(|k| k == name))
                .collect();
            let rest: Vec<usize> = (0..attributes.len()).filter(|i| !order.contains(i)).collect();
            order.extend(rest);
            let mut text = String::new();
            for (slot, &i) in order.iter().enumerate() {
                text.push_str(&file.source[attributes[i].range()]);
                if let Some(next) = attributes.get(slot + 1) {
                    text.push_str(&file.source[attributes[slot].range().end..next.range().start]);
                }
            }
            let start = attributes[0].range().start;
            let end = attributes[attributes.len() - 1].range().end;
            reporter
                .report(
                    &XML_TAG_POSITION,
                    file,
                    at(file, node),
                    format!("The expected attributes order is `<{tag} {} ...>`", shown.join(" ")),
                )
                .fix = Some(Fix::safe(
                "Reorder the attributes",
                vec![Edit::replace(start, end, text)],
            ));
        }
    }
}

pub const XML_DEPRECATED_DATA_NODE: Rule = Rule {
    code: "XML010",
    name: "xml-deprecated-data-node",
    summary: "A `<data>` element is the only child of `<odoo>`.",
    doc: r#"
## What it does

Reports `<odoo><data>` when `<data>` is the only element in the file.

## Why is this bad?

The extra level is a leftover from old versions. Use `<odoo>`, or
`<odoo noupdate="1">` instead of `<odoo><data noupdate="1">`.
"#,
    check: Check::Xml(check_deprecated_data_node),
    min_odoo: None,
    max_odoo: None,
};

fn check_deprecated_data_node(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        let Some(root) = odoo_root(file) else { continue };
        if root.children().filter(Node::is_element).count() >= 2 {
            continue;
        }
        for data in children(root, "data") {
            reporter.report(
                &XML_DEPRECATED_DATA_NODE,
                file,
                at(file, data),
                "Deprecated `<data>` node",
            );
        }
    }
}

pub const XML_DEPRECATED_OPENERP_NODE: Rule = Rule {
    code: "XML011",
    name: "xml-deprecated-openerp-node",
    summary: "The root element is `<openerp>`.",
    doc: r#"
## What it does

Reports files whose root element is `<openerp>`.

## Why is this bad?

`<openerp>` is the name from before Odoo 9.0. Use `<odoo>`.
"#,
    check: Check::Xml(check_deprecated_openerp_node),
    min_odoo: None,
    max_odoo: None,
};

fn check_deprecated_openerp_node(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        if let Some(root) = file.root().filter(|r| is(*r, "openerp")) {
            reporter.report(
                &XML_DEPRECATED_OPENERP_NODE,
                file,
                at(file, root),
                "Deprecated `<openerp>` xml node",
            );
        }
    }
}

/// Elements inside a `<template>` below the `<odoo>` root.
fn in_templates<'a, 'input>(file: &'a XmlFile<'input>) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    let inside = odoo_root(file).is_some();
    file.elements()
        .filter(move |n| inside && n.ancestors().skip(1).any(|a| is(a, "template")))
}

pub const XML_DEPRECATED_QWEB_DIRECTIVE: Rule = Rule {
    code: "XML012",
    name: "xml-deprecated-qweb-directive",
    summary: "A template uses `t-esc-options`, `t-field-options` or `t-raw-options`.",
    doc: r#"
## What it does

Reports the QWeb directives `t-esc-options`, `t-field-options` and
`t-raw-options`.

## Why is this bad?

They were replaced by `t-options`.
"#,
    check: Check::Xml(check_deprecated_qweb_directive),
    min_odoo: None,
    max_odoo: None,
};

const OPTIONS_DIRECTIVES: &[&str] = &["t-esc-options", "t-field-options", "t-raw-options"];

fn check_deprecated_qweb_directive(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in in_templates(file) {
            let found: Vec<&str> = OPTIONS_DIRECTIVES
                .iter()
                .copied()
                .filter(|d| node.attribute(*d).is_some())
                .collect();
            if !found.is_empty() {
                reporter.report(
                    &XML_DEPRECATED_QWEB_DIRECTIVE,
                    file,
                    at(file, node),
                    format!(
                        "Deprecated QWeb directive `{}`. Use `t-options` instead",
                        found.join(", ")
                    ),
                );
            }
        }
    }
}

pub const XML_DEPRECATED_QWEB_DIRECTIVE_15: Rule = Rule {
    code: "XML013",
    name: "xml-deprecated-qweb-directive-15",
    summary: "A template uses `t-esc` or `t-raw`, deprecated in Odoo 15.0.",
    doc: r#"
## What it does

Reports `t-esc` and `t-raw` in templates of modules for Odoo 15.0 and later.

## Why is this bad?

Odoo 15.0 replaced both by `t-out`, which escapes text and leaves `Markup`
alone. See [odoo/odoo#70004](https://github.com/odoo/odoo/pull/70004).

## Fix safety

`t-esc` to `t-out` is safe. `t-raw` to `t-out` is unsafe: `t-out` escapes
plain strings that `t-raw` printed as HTML, so wrap such values in `Markup`.
There is no fix when the element already has `t-out`.
"#,
    check: Check::Xml(check_deprecated_qweb_directive_15),
    min_odoo: Some(OdooVersion::new(15, 0)),
    max_odoo: None,
};

fn check_deprecated_qweb_directive_15(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in in_templates(file) {
            let deprecated: Vec<_> = node
                .attributes()
                .filter(|a| a.namespace().is_none() && matches!(a.name(), "t-esc" | "t-raw"))
                .collect();
            if deprecated.is_empty() {
                continue;
            }
            let names: Vec<&str> = deprecated.iter().map(|a| a.name()).collect();
            let fix = (deprecated.len() == 1 && node.attribute("t-out").is_none()).then(|| {
                let range = deprecated[0].range_qname();
                let edits = vec![Edit::replace(range.start, range.end, "t-out")];
                if deprecated[0].name() == "t-esc" {
                    Fix::safe("Use `t-out`", edits)
                } else {
                    Fix::unsafe_("Use `t-out` (escapes text that is not `Markup`)", edits)
                }
            });
            reporter
                .report(
                    &XML_DEPRECATED_QWEB_DIRECTIVE_15,
                    file,
                    at(file, node),
                    format!("Deprecated QWeb directive `{}`. Use `t-out` instead", names.join(", ")),
                )
                .fix = fix;
        }
    }
}

pub const XML_DEPRECATED_TREE_ATTRIBUTE: Rule = Rule {
    code: "XML014",
    name: "xml-deprecated-tree-attribute",
    summary: "A `<tree>` view uses `string`, `colors` or `fonts`.",
    doc: r#"
## What it does

Reports the `string`, `colors` and `fonts` attributes on `<tree>` in
`ir.ui.view` records.

## Why is this bad?

Odoo ignores them: `colors` and `fonts` were replaced by `decoration-*`
attributes, and list views have no title.
"#,
    check: Check::Xml(check_deprecated_tree_attribute),
    min_odoo: None,
    max_odoo: None,
};

fn views<'a, 'input>(file: &'a XmlFile<'input>) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    under_root(file, RECORD_TAGS).filter(|r| r.attribute("model") == Some("ir.ui.view"))
}

fn check_deprecated_tree_attribute(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for view in views(file) {
            for tree in view.descendants().skip(1).filter(|n| is(*n, "tree")) {
                let found: Vec<&str> = ["colors", "fonts", "string"]
                    .into_iter()
                    .filter(|a| tree.attribute(*a).is_some())
                    .collect();
                if !found.is_empty() {
                    reporter.report(
                        &XML_DEPRECATED_TREE_ATTRIBUTE,
                        file,
                        at(file, tree),
                        format!("Deprecated \"<tree {}=...\"", found.join(",")),
                    );
                }
            }
        }
    }
}

pub const XML_DEPRECATED_OE_CHATTER: Rule = Rule {
    code: "XML015",
    name: "xml-deprecated-oe-chatter",
    summary: "A form uses `<div class=\"oe_chatter\">` instead of `<chatter/>`.",
    doc: r#"
## What it does

Reports `<div class="oe_chatter">` in modules for Odoo 18.0 and later.

## Why is this bad?

Odoo 18.0 added the `<chatter/>` tag for form views. See
[odoo/odoo#156463](https://github.com/odoo/odoo/pull/156463).
"#,
    check: Check::Xml(check_deprecated_oe_chatter),
    min_odoo: Some(OdooVersion::new(18, 0)),
    max_odoo: None,
};

fn check_deprecated_oe_chatter(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for div in file.elements().filter(|n| is(*n, "div") && has_class(*n, "oe_chatter")) {
            reporter.report(
                &XML_DEPRECATED_OE_CHATTER,
                file,
                at(file, div),
                "Please replace old style chatters with the new tag <chatter/>.",
            );
        }
    }
}

pub const XML_DEPRECATED_RES_GROUPS_CATEGORY_ID: Rule = Rule {
    code: "XML016",
    name: "xml-deprecated-res-groups-category-id",
    summary: "A `res.groups` record sets `category_id`, removed in Odoo 19.0.",
    doc: r#"
## What it does

Reports `<field name="category_id">` in `res.groups` records of modules for
Odoo 19.0 and later.

## Why is this bad?

Odoo 19.0 removed the field: installing fails with `ValueError: Invalid field
'category_id' on model 'res.groups'`. Groups shown in the user form now use
`privilege_id`, a `res.groups.privilege` record. See
[odoo/odoo#199988](https://github.com/odoo/odoo/pull/199988).

## Fix safety

Unsafe: the field is removed, as Odoo did for its own technical groups. Set
`privilege_id` instead when the group should appear in the user form.
"#,
    check: Check::Xml(check_res_groups_category_id),
    min_odoo: Some(OdooVersion::new(19, 0)),
    max_odoo: None,
};

fn check_res_groups_category_id(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        if !file.root().is_some_and(|r| is(r, "odoo")) {
            continue;
        }
        for group in under_root(file, &["record"]).filter(|r| r.attribute("model") == Some("res.groups")) {
            for field in children(group, "field").filter(|f| f.attribute("name") == Some("category_id")) {
                reporter
                    .report(
                        &XML_DEPRECATED_RES_GROUPS_CATEGORY_ID,
                        file,
                        at(file, field),
                        "Deprecated `<field name=\"category_id\"` of `res.groups` removed in Odoo 19.0",
                    )
                    .fix = Some(Fix::unsafe_("Remove `category_id`", vec![delete_element(file, field)]));
            }
        }
    }
}

pub const XML_VIEW_DANGEROUS_REPLACE_LOW_PRIORITY: Rule = Rule {
    code: "XML017",
    name: "xml-view-dangerous-replace-low-priority",
    summary: "A view replaces part of another with a priority below 99.",
    doc: r#"
## What it does

Reports `ir.ui.view` records with `position="replace"` in their arch and a
priority below 99.

## Why is this bad?

A replaced element is gone for every other module that inherits the view,
which breaks them in ways that depend on the installation order. Prefer
`position="attributes"`, `position="move"` or `invisible="1"`; replace only as
a last resort, with a high priority.
"#,
    check: Check::Xml(check_view_dangerous_replace),
    min_odoo: None,
    max_odoo: None,
};

fn check_view_dangerous_replace(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for view in views(file) {
            let priority = child_field(view, "priority")
                .and_then(|p| p.attribute("eval").or_else(|| p.text()))
                .filter(|v| !v.is_empty())
                .map_or(Some(0), py_int)
                .unwrap_or(0);
            let replaces = view
                .children()
                .find(|c| is(*c, "field") && c.attribute("name") == Some("arch") && c.attribute("type") == Some("xml"))
                .is_some_and(|arch| {
                    arch.descendants()
                        .skip(1)
                        .any(|n| n.attribute("position") == Some("replace"))
                });
            if replaces && priority < DEFAULT_MIN_PRIORITY {
                reporter.report(
                    &XML_VIEW_DANGEROUS_REPLACE_LOW_PRIORITY,
                    file,
                    at(file, view),
                    format!("Dangerous use of `replace` from view with priority {priority} < {DEFAULT_MIN_PRIORITY}"),
                );
            }
        }
    }
}

pub const XML_DANGEROUS_QWEB_REPLACE_LOW_PRIORITY: Rule = Rule {
    code: "XML018",
    name: "xml-dangerous-qweb-replace-low-priority",
    summary: "A template replaces part of another with a priority below 99.",
    doc: r#"
## What it does

Reports `position="replace"` directly inside a `<template>` with a priority
below 99.

## Why is this bad?

As for views: the replaced part disappears for every other inheriting
template. Prefer `position="attributes"`, `position="move"` or
`t-if="False"`.
"#,
    check: Check::Xml(check_qweb_dangerous_replace),
    min_odoo: None,
    max_odoo: None,
};

fn check_qweb_dangerous_replace(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for template in under_root(file, &["template"]) {
            let priority = template.attribute("priority").and_then(py_int).unwrap_or(0);
            if priority >= DEFAULT_MIN_PRIORITY {
                continue;
            }
            for child in template
                .children()
                .filter(|c| c.attribute("position") == Some("replace"))
            {
                reporter.report(
                    &XML_DANGEROUS_QWEB_REPLACE_LOW_PRIORITY,
                    file,
                    at(file, child),
                    format!("Dangerous use of `replace` from view with priority {priority} < {DEFAULT_MIN_PRIORITY}"),
                );
            }
        }
    }
}

pub const XML_CREATE_USER_WO_RESET_PASSWORD: Rule = Rule {
    code: "XML019",
    name: "xml-create-user-wo-reset-password",
    summary: "A `res.users` record is created without `no_reset_password`.",
    doc: r#"
## What it does

Reports `res.users` records that set `name` (so create a user) without
`context="{'no_reset_password': True}"`.

## Why is this bad?

Creating the user then sends a password reset email, or logs a warning when
mail is not configured.
"#,
    check: Check::Xml(check_create_user),
    min_odoo: None,
    max_odoo: None,
};

fn check_create_user(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for user in under_root(file, RECORD_TAGS).filter(|r| r.attribute("model") == Some("res.users")) {
            let creates = child_field(user, "name").is_some();
            if creates
                && !user
                    .attribute("context")
                    .unwrap_or_default()
                    .contains("no_reset_password")
            {
                reporter.report(
                    &XML_CREATE_USER_WO_RESET_PASSWORD,
                    file,
                    at(file, user),
                    "record res.users without `context=\"{'no_reset_password': True}\"`",
                );
            }
        }
    }
}

pub const XML_NOT_VALID_CHAR_LINK: Rule = Rule {
    code: "XML020",
    name: "xml-not-valid-char-link",
    summary: "A local `href`/`src` has no plain file extension.",
    doc: r#"
## What it does

Reports `<link href="/...">` and `<script src="/...">` whose file name does
not end in a plain extension such as `.js` or `.css`.

## Why is this bad?

Such paths usually have a typo, or a query string or template expression that
the asset bundler does not resolve.
"#,
    check: Check::Xml(check_char_link),
    min_odoo: None,
    max_odoo: None,
};

static EXTENSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[.][a-zA-Z]+$").unwrap());

/// Python's `os.path.splitext(os.path.basename(path))[1]`.
fn extension(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem_start = name.len() - name.trim_start_matches('.').len();
    name[stem_start..].rfind('.').map_or("", |i| &name[stem_start + i..])
}

fn check_char_link(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        let Some(root) = file.root() else { continue };
        for node in root.descendants().skip(1) {
            let resource = if is(node, "link") {
                node.attribute("href")
            } else if is(node, "script") {
                node.attribute("src")
            } else {
                None
            };
            let Some(resource) = resource else { continue };
            // OCA reads `href or src` for both tags.
            let resource = Some(resource)
                .filter(|r| !r.is_empty())
                .or_else(|| node.attribute("src"))
                .unwrap_or_default();
            if resource.starts_with('/') && !EXTENSION.is_match(extension(resource)) {
                reporter.report(
                    &XML_NOT_VALID_CHAR_LINK,
                    file,
                    at(file, node),
                    "The resource in in src/href contains a not valid character",
                );
            }
        }
    }
}

pub const XML_XPATH_TRANSLATABLE_ITEM: Rule = Rule {
    code: "XML021",
    name: "xml-xpath-translatable-item",
    summary: "An `<xpath>` selects an element by its translatable text.",
    doc: r#"
## What it does

Reports `<xpath expr="...">` using `[contains(text(), ...)]` or `[text()=...]`.

## Why is this bad?

The text is translated, so the expression finds nothing for users with
another language, and Odoo raises an error.
"#,
    check: Check::Xml(check_xpath_translatable),
    min_odoo: None,
    max_odoo: None,
};

fn check_xpath_translatable(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in file.elements().filter(|n| is(*n, "xpath")) {
            let expr: String = node.attribute("expr").unwrap_or_default().replace(' ', "");
            if expr.contains("[contains(text()") || expr.contains("[text()=") {
                reporter.report(
                    &XML_XPATH_TRANSLATABLE_ITEM,
                    file,
                    at(file, node),
                    "Use of translatable xpath `text()`",
                );
            }
        }
    }
}

pub const XML_OE_STRUCTURE_MISSING_ID: Rule = Rule {
    code: "XML022",
    name: "xml-oe-structure-missing-id",
    summary: "An `oe_structure` element has no id containing `oe_structure`.",
    doc: r#"
## What it does

Reports elements with the class `oe_structure` whose `id` is missing or does
not contain `oe_structure`.

## Why is this bad?

The website editor saves what users put in an `oe_structure` as a view that
inherits the template by the element's id. Without a stable id, the content
is lost or lands in the wrong place when the template changes. See
[OCA/odoo-pre-commit-hooks#27](https://github.com/OCA/odoo-pre-commit-hooks/issues/27).
"#,
    check: Check::Xml(check_oe_structure),
    min_odoo: None,
    max_odoo: None,
};

fn check_oe_structure(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in file.elements().filter(|n| has_class(*n, "oe_structure")) {
            if node.attribute("id").is_some_and(|id| id.contains("oe_structure")) {
                continue;
            }
            reporter.report(
                &XML_OE_STRUCTURE_MISSING_ID,
                file,
                at(file, node),
                "Consider removing the class `oe_structure` or adding a proper id to the tag. The id must contain `oe_structure`",
            );
        }
    }
}

pub const XML_FIELD_BOOL_WITHOUT_EVAL: Rule = Rule {
    code: "XML023",
    name: "xml-field-bool-without-eval",
    summary: "A boolean field is set as text instead of with `eval`.",
    doc: r#"
## What it does

Reports `<field name="active">False</field>` and the like for common boolean
fields.

## Why is this bad?

The text is the string `"False"`, which is true. Use
`<field name="active" eval="False" />`.

## Fix safety

Safe for `True`. Unsafe for `False`: the stored value changes from true to
false, which is the point, but check that nothing relied on it.
"#,
    check: Check::Xml(check_field_eval),
    min_odoo: None,
    max_odoo: None,
};

pub const XML_FIELD_NUMERIC_WITHOUT_EVAL: Rule = Rule {
    code: "XML024",
    name: "xml-field-numeric-without-eval",
    summary: "A numeric field is set as text instead of with `eval`.",
    doc: r#"
## What it does

Reports `<field name="sequence">10</field>` and the like for common numeric
fields.

## Why is this bad?

`eval` states that the value is a number instead of relying on a conversion
from text. Use `<field name="sequence" eval="10" />`.

## Fix safety

Safe: the field gets the same value.
"#,
    check: Check::Xml(check_field_eval),
    min_odoo: None,
    max_odoo: None,
};

const BOOLEAN_FIELDS: &[&str] = &["active", "is_published", "website_published"];
const BOOLEAN_FIELDS_BY_MODEL: &[(&str, &[&str])] = &[
    ("account.payment.term", &["is_fixed"]),
    ("account.report", &["filter_journals", "filter_unfold_all"]),
    ("account.report.column", &["sortable"]),
    ("account.report.expression", &["auditable", "green_on_positive"]),
    ("account.report.line", &["foldable", "hide_if_zero", "hierarchy_level"]),
    ("hr.payslip.input.type", &["available_in_attachments"]),
    ("hr.salary.rule", &["appears_on_payroll_report", "appears_on_payslip"]),
    ("hr.work.entry.type", &["is_leave"]),
    ("ir.attachment", &["public"]),
    ("ir.rule", &["perm_create", "perm_read", "perm_unlink", "perm_write"]),
    ("mail.message.subtype", &["default"]),
    ("mail.template", &["auto_delete"]),
    ("payment.method", &["support_express_checkout", "support_tokenization"]),
    ("planning.slot", &["publication_warning"]),
    ("product.product", &["available_in_pos"]),
    ("product.template", &["available_in_pos"]),
    ("res.partner", &["is_company"]),
];
const NUMERIC_FIELDS: &[&str] = &["color", "sequence", "website_sequence"];
const NUMERIC_FIELDS_BY_MODEL: &[(&str, &[&str])] = &[
    ("account.analytic.line", &["amount", "unit_amount"]),
    ("ir.cron", &["interval_number", "numbercall"]),
    ("ir.ui.view", &["priority"]),
    ("product.product", &["list_price", "standard_price", "weight"]),
    ("product.template", &["list_price", "standard_price", "weight"]),
    ("res.currency", &["rounding"]),
    ("res.currency.rate", &["rate"]),
    ("sale.order.line", &["price_unit", "product_uom_qty"]),
    ("stock.move", &["product_uom_qty", "quantity_done"]),
];

fn known_field(common: &[&str], by_model: &[(&str, &[&str])], model: &str, field: &str) -> bool {
    common.contains(&field)
        || by_model
            .iter()
            .any(|(m, fields)| *m == model && fields.contains(&field))
}

static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[+-]?\d+(\.\d+)?\n?$").unwrap());

fn check_field_eval(ctx: &XmlContext, reporter: &mut XmlReporter) {
    let booleans = ctx.settings.is_selected(&XML_FIELD_BOOL_WITHOUT_EVAL);
    let numbers = ctx.settings.is_selected(&XML_FIELD_NUMERIC_WITHOUT_EVAL);
    for file in ctx.files {
        for record in under_root(file, RECORD_TAGS) {
            for field in record.descendants().skip(1).filter(|n| is(*n, "field")) {
                let Some(name) = field.attribute("name") else { continue };
                if field.attribute("eval").is_some()
                    || field.attribute("type").is_some()
                    || !field.children().any(|c| c.is_text())
                {
                    continue;
                }
                let Some(text) = field.text() else { continue };
                let Some(model) = field.parent_element().and_then(|p| p.attribute("model")) else {
                    continue;
                };
                let value = py_title(text);
                let (rule, message) = if booleans
                    && (value == "True" || value == "False")
                    && known_field(BOOLEAN_FIELDS, BOOLEAN_FIELDS_BY_MODEL, model, name)
                {
                    (&XML_FIELD_BOOL_WITHOUT_EVAL, "boolean")
                } else if numbers
                    && NUMBER.is_match(&value)
                    && known_field(NUMERIC_FIELDS, NUMERIC_FIELDS_BY_MODEL, model, name)
                {
                    (&XML_FIELD_NUMERIC_WITHOUT_EVAL, "numeric")
                } else {
                    continue;
                };
                // `<field name="x">10</field>` -> `<field name="x" eval="10" />`.
                let fix = (field.children().count() == 1).then(|| {
                    let value = value.trim_end_matches('\n');
                    let tag_end = start_tag_end(file, field);
                    let open = file.source[field.range().start..tag_end - 1]
                        .trim_end_matches('/')
                        .trim_end();
                    let edits = vec![Edit::replace(
                        at(file, field),
                        field.range().end,
                        format!("{open} eval=\"{value}\" />"),
                    )];
                    let title = format!("Use `eval=\"{value}\"`");
                    if value == "False" {
                        Fix::unsafe_(title, edits)
                    } else {
                        Fix::safe(title, edits)
                    }
                });
                reporter
                    .report(
                        rule,
                        file,
                        at(file, field),
                        format!("Field `{name}` with {message} value without `eval` attribute"),
                    )
                    .fix = fix;
            }
        }
    }
}

// --- odoo-lint's own: upgrades -------------------------------------------

/// Attributes that hold CSS classes.
const CLASS_ATTRIBUTES: &[&str] = &["class", "t-att-class", "t-attf-class"];

/// Bootstrap 4 classes with a single Bootstrap 5 replacement (Odoo 15.0
/// moved to Bootstrap 5), as `(pattern, replacement)`.
static BOOTSTRAP_RENAMES: LazyLock<Vec<(FancyRegex, &'static str)>> = LazyLock::new(|| {
    let bp = r"((?:sm|md|lg|xl|xxl)-)?";
    let token = |body: String| format!(r"(?<![\w-]){body}(?![\w-])");
    [
        (format!("ml-{bp}(0|1|2|3|4|5|auto)"), "ms-$1$2"),
        (format!("mr-{bp}(0|1|2|3|4|5|auto)"), "me-$1$2"),
        (format!("pl-{bp}(0|1|2|3|4|5)"), "ps-$1$2"),
        (format!("pr-{bp}(0|1|2|3|4|5)"), "pe-$1$2"),
        (format!("text-{bp}left"), "text-${1}start"),
        (format!("text-{bp}right"), "text-${1}end"),
        (format!("float-{bp}left"), "float-${1}start"),
        (format!("float-{bp}right"), "float-${1}end"),
        ("border-left".into(), "border-start"),
        ("border-right".into(), "border-end"),
        ("rounded-left".into(), "rounded-start"),
        ("rounded-right".into(), "rounded-end"),
        ("dropdown-menu-left".into(), "dropdown-menu-start"),
        ("dropdown-menu-right".into(), "dropdown-menu-end"),
        ("font-weight-(lighter|light|normal|bold|bolder)".into(), "fw-$1"),
        ("font-italic".into(), "fst-italic"),
        ("text-monospace".into(), "font-monospace"),
        ("sr-only-focusable".into(), "visually-hidden-focusable"),
        ("sr-only".into(), "visually-hidden"),
        ("badge-pill".into(), "rounded-pill"),
        ("custom-select".into(), "form-select"),
        ("no-gutters".into(), "g-0"),
    ]
    .into_iter()
    .map(|(pattern, replacement)| (FancyRegex::new(&token(pattern)).expect("valid pattern"), replacement))
    .collect()
});

/// Bootstrap 4 classes that Bootstrap 5 dropped without a replacement.
static BOOTSTRAP_REMOVED: LazyLock<FancyRegex> = LazyLock::new(|| {
    FancyRegex::new(
        r"(?<![\w-])(form-group|form-row|btn-block|media|jumbotron|card-deck|input-group-append|input-group-prepend|custom-control)(?![\w-])",
    )
    .expect("valid pattern")
});

/// The class attributes of every element, with their raw text in the file.
fn class_attributes<'a, 'input>(
    file: &'a XmlFile<'input>,
) -> impl Iterator<Item = (Node<'a, 'input>, std::ops::Range<usize>)> + 'a {
    file.elements().flat_map(|node| {
        node.attributes()
            .filter(|a| a.namespace().is_none() && CLASS_ATTRIBUTES.contains(&a.name()))
            .map(move |a| (node, a.range_value()))
            .collect::<Vec<_>>()
    })
}

pub const XML_BOOTSTRAP4_CLASS: Rule = Rule {
    code: "XML101",
    name: "xml-bootstrap4-class",
    summary: "A Bootstrap 4 class that Bootstrap 5 renamed, in Odoo 15.0 or later.",
    doc: r#"
## What it does

Reports Bootstrap 4 classes in `class`, `t-att-class` and `t-attf-class` that
Bootstrap 5 renamed: spacing (`ml-*`, `mr-*`, `pl-*`, `pr-*`), text and float
alignment, border and rounded directions, dropdown alignment, the typography
helpers, `sr-only`, `badge-pill`, `custom-select` and `no-gutters`.

## Why is this bad?

Odoo 15.0 moved to Bootstrap 5, where these classes no longer exist: they
silently do nothing. Migrations keep them, because nothing converts them.

## Example

```xml
<div class="ml-3 text-right font-weight-bold">
```

Use instead:

```xml
<div class="ms-3 text-end fw-bold">
```

## Fix safety

Safe: each class has exactly one Bootstrap 5 equivalent. Stylesheets of the
module that target the old names are not changed.
"#,
    check: Check::Xml(check_bootstrap4_class),
    min_odoo: Some(OdooVersion::new(15, 0)),
    max_odoo: None,
};

fn check_bootstrap4_class(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for (node, range) in class_attributes(file) {
            let original = &file.source[range.clone()];
            let mut value = original.to_string();
            let mut renamed = Vec::new();
            for (pattern, replacement) in BOOTSTRAP_RENAMES.iter() {
                let mut found: Vec<String> = Vec::new();
                if let Ok(matches) = pattern.find_iter(&value).collect::<Result<Vec<_>, _>>() {
                    found.extend(matches.iter().map(|m| m.as_str().to_string()));
                }
                if found.is_empty() {
                    continue;
                }
                let new = pattern.replace_all(&value, *replacement).into_owned();
                for old in found {
                    let replaced = pattern.replace(&old, *replacement).into_owned();
                    renamed.push(format!("{old} -> {replaced}"));
                }
                value = new;
            }
            if renamed.is_empty() {
                continue;
            }
            reporter
                .report(
                    &XML_BOOTSTRAP4_CLASS,
                    file,
                    at(file, node),
                    format!("Bootstrap 4 class renamed in Bootstrap 5: {}", renamed.join(", ")),
                )
                .fix = Some(Fix::safe(
                "Use the Bootstrap 5 classes",
                vec![Edit::replace(range.start, range.end, value)],
            ));
        }
    }
}

pub const XML_BOOTSTRAP4_REMOVED_CLASS: Rule = Rule {
    code: "XML102",
    name: "xml-bootstrap4-removed-class",
    summary: "A Bootstrap 4 class that Bootstrap 5 removed, in Odoo 15.0 or later.",
    doc: r#"
## What it does

Reports Bootstrap 4 classes without a Bootstrap 5 replacement: `form-group`,
`form-row`, `btn-block`, `media`, `jumbotron`, `card-deck`,
`input-group-append`, `input-group-prepend` and `custom-control`.

## Why is this bad?

Odoo 15.0 moved to Bootstrap 5, where these classes do nothing. The markup
needs rethinking, e.g. spacing utilities (`mb-3`) instead of `form-group`, a
grid instead of `form-row`, `d-grid` instead of `btn-block`. See the
[Bootstrap 5 migration guide](https://getbootstrap.com/docs/5.0/migration/).
"#,
    check: Check::Xml(check_bootstrap4_removed_class),
    min_odoo: Some(OdooVersion::new(15, 0)),
    max_odoo: None,
};

fn check_bootstrap4_removed_class(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for (node, range) in class_attributes(file) {
            let value = &file.source[range];
            let Ok(found) = BOOTSTRAP_REMOVED.find_iter(value).collect::<Result<Vec<_>, _>>() else {
                continue;
            };
            if found.is_empty() {
                continue;
            }
            let classes: Vec<&str> = found.iter().map(|m| m.as_str()).collect();
            reporter.report(
                &XML_BOOTSTRAP4_REMOVED_CLASS,
                file,
                at(file, node),
                format!("Bootstrap 4 class removed in Bootstrap 5: {}", classes.join(", ")),
            );
        }
    }
}
