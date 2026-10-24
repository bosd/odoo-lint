//! `odl pot`: a module's translation template (`i18n/<module>.pot`), built
//! from its source the way Odoo's export writes it (`TranslationModuleReader`
//! and `PoFileWriter` in odoo/tools/translate.py), without a database.
//!
//! This covers the terms of the code: `_()` and `_lt()` in Python, `_t()` in
//! the JavaScript of `static/src`, the text of the QWeb templates there, and
//! spreadsheet dashboards.

use crate::manifest::Manifest;
use crate::odoo_version::OdooVersion;
use crate::po::{PoEntry, PoFile};
use oxc_ast::ast::{Argument, CallExpression, Expression, TaggedTemplateExpression};
use oxc_ast_visit::{walk, Visit};
use regex::Regex;
use ruff_python_ast::token::TokenKind;
use ruff_python_ast::visitor::{self as py_visit, Visitor};
use ruff_python_ast::{BytesLiteral, FString, InterpolatedStringElement, StringLiteral};
use ruff_text_size::{Ranged, TextSize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// The extracted comment Odoo adds to the terms of Python code.
const PYTHON_COMMENT: &str = "odoo-python";
/// The extracted comment of JavaScript and QWeb terms.
const JAVASCRIPT_COMMENT: &str = "odoo-javascript";

/// Which extractors Odoo uses for code: its own from 19.0 on
/// (odoo/tools/babel/), Babel's before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extractor {
    /// Odoo's extractors (19.0 and later).
    Odoo,
    /// Babel's extractors, as Odoo 18.0 and older use them.
    Babel,
}

impl Extractor {
    /// The extractors Odoo `version` uses.
    pub fn for_version(version: OdooVersion) -> Self {
        if version.major >= 19 {
            Self::Odoo
        } else {
            Self::Babel
        }
    }
}

/// One translatable term and where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    /// The text to translate (the `msgid`).
    pub source: String,
    /// `(type, name, res_id)` as Odoo sorts them: `("code", path, "0")`.
    pub reference: (String, String, String),
    /// The extracted comment Odoo adds, such as `odoo-python`.
    pub comment: Option<&'static str>,
}

impl Term {
    fn code(source: String, path: &str, comment: &'static str) -> Self {
        Self {
            source,
            reference: ("code".into(), path.into(), "0".into()),
            comment: Some(comment),
        }
    }
}

/// Whether Odoo exports a term: it needs a letter, so `:` or `.00` are left out.
fn exported(term: &str) -> bool {
    term.trim().chars().any(char::is_alphabetic)
}

/// The template of `module` as Odoo's `PoFileWriter` writes it, without the
/// creation dates (OCA's templates leave them out to keep diffs small).
pub fn write(module: &str, version: OdooVersion, terms: &[Term]) -> String {
    // Per msgid: its references and extracted comments.
    type Group<'t> = (BTreeSet<&'t (String, String, String)>, BTreeSet<&'static str>);
    let mut grouped: BTreeMap<&str, Group> = BTreeMap::new();
    for term in terms.iter().filter(|t| exported(&t.source)) {
        let (references, comments) = grouped.entry(&term.source).or_default();
        references.insert(&term.reference);
        comments.extend(term.comment);
    }
    let entries = grouped
        .into_iter()
        .map(|(source, (references, comments))| {
            let mut comment = format!("module: {module}");
            for extra in comments {
                comment.push('\n');
                comment.push_str(extra);
            }
            let mut occurrences: Vec<(String, String)> = Vec::new();
            for (kind, name, res_id) in references {
                let occurrence = if kind == "code" {
                    (format!("code:{name}"), "0".to_string())
                } else {
                    (format!("{kind}:{name}:{res_id}"), String::new())
                };
                if !occurrences.contains(&occurrence) {
                    occurrences.push(occurrence);
                }
            }
            PoEntry {
                msgid: source.to_string(),
                comment,
                occurrences,
                ..PoEntry::default()
            }
        })
        .collect();
    let metadata = [
        ("Project-Id-Version", format!("Odoo Server {version}")),
        ("Report-Msgid-Bugs-To", String::new()),
        ("Last-Translator", String::new()),
        ("Language-Team", String::new()),
        ("MIME-Version", "1.0".into()),
        ("Content-Type", "text/plain; charset=UTF-8".into()),
        ("Content-Transfer-Encoding", String::new()),
        ("Plural-Forms", String::new()),
    ];
    PoFile {
        header: format!(
            "Translation of Odoo Server.\nThis file contains the translation of the following modules:\n\t* {module}\n"
        ),
        metadata: metadata.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        metadata_is_fuzzy: false,
        entries,
    }
    .to_po_string()
}

/// The terms of the code of the module in `dir`, with paths as Odoo writes
/// them (`addons/<module>/...`).
pub fn code_terms(dir: &Path, module: &str, extractor: Extractor) -> Vec<Term> {
    let mut terms = Vec::new();
    for entry in walkdir::WalkDir::new(dir).follow_links(true).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(dir) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let display = format!("addons/{module}/{relative}");
        let extension = entry.path().extension().and_then(|e| e.to_str()).unwrap_or("");
        // Odoo matches the folder against `*/static/src*`.
        let folder = format!("/{}", relative.rsplit_once('/').map_or("", |(folder, _)| folder));
        let in_static = folder.contains("/static/src");
        // And spreadsheet dashboards against `*/data/*`, so only in subfolders of data/.
        let file_name = entry.file_name().to_string_lossy();
        let dashboard = folder.contains("/data/") && DASHBOARD_FILE.is_match(&file_name);
        let Ok(source) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let (found, comment) = match extension {
            "py" => (python_terms(&source, extractor), PYTHON_COMMENT),
            "js" if in_static => (javascript_terms(&source, extractor), JAVASCRIPT_COMMENT),
            "xml" if in_static => (qweb_terms(&source), JAVASCRIPT_COMMENT),
            "json" if dashboard => (spreadsheet_terms(&source, extractor), JAVASCRIPT_COMMENT),
            _ => continue,
        };
        terms.extend(found.into_iter().map(|source| Term::code(source, &display, comment)));
    }
    terms
}

static DASHBOARD_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"_dashboard(\.osheet)?\.json$").expect("valid regex"));

/// A `_t("...")` call in a spreadsheet formula; the string is kept as written.
static FORMULA_TERM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:^|[^\w])_t\s*\(\s*(?:"((?:[^"\\]|\\.)*)"|'((?:[^'\\]|\\.)*)')\s*\)"#).expect("valid regex")
});

static MARKDOWN_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[(.+)\]\(.+\)$").expect("valid regex"));

/// The terms of a spreadsheet dashboard (`extract_spreadsheet_terms`): `_t`
/// in formulas, link labels, chart titles and the labels of global filters.
/// Up to 18.0 a chart's `baselineDescr` is text; from 19.0 it and `keyDescr`
/// are objects with a `text`.
pub fn spreadsheet_terms(source: &str, extractor: Extractor) -> Vec<String> {
    let Ok(data) = serde_json::from_str::<serde_json::Value>(source) else {
        return Vec::new();
    };
    let text = serde_json::Value::as_str;
    let mut terms: BTreeSet<&str> = BTreeSet::new();
    for sheet in data["sheets"].as_array().into_iter().flatten() {
        for cell in sheet["cells"].as_object().into_iter().flat_map(|cells| cells.values()) {
            // A cell was an object before saas-18.1.
            let content = cell.as_str().or_else(|| cell["content"].as_str()).unwrap_or("");
            if content.starts_with('=') {
                for found in FORMULA_TERM.captures_iter(content) {
                    terms.extend(found.get(1).or_else(|| found.get(2)).map(|m| m.as_str()));
                }
            } else if let Some(label) = MARKDOWN_LINK.captures(content).and_then(|link| link.get(1)) {
                terms.insert(label.as_str());
            }
        }
        for figure in sheet["figures"].as_array().into_iter().flatten() {
            if figure["tag"] != "chart" {
                continue;
            }
            let chart = &figure["data"];
            terms.extend(text(&chart["title"]).or_else(|| text(&chart["title"]["text"])));
            for axes in chart["axesDesign"].as_object().into_iter().flat_map(|a| a.values()) {
                terms.extend(text(&axes["title"]["text"]));
            }
            match extractor {
                Extractor::Odoo => {
                    terms.extend(text(&chart["baselineDescr"]["text"]));
                    terms.extend(text(&chart["keyDescr"]["text"]));
                }
                Extractor::Babel => terms.extend(text(&chart["baselineDescr"])),
            }
        }
    }
    for filter in data["globalFilters"].as_array().into_iter().flatten() {
        terms.extend(text(&filter["label"]));
    }
    terms
        .into_iter()
        .filter(|t| t.chars().any(char::is_alphabetic))
        .map(str::to_string)
        .collect()
}

/// The values of the string literals of a Python file, by start offset; `None`
/// for bytes and for f-strings with replacement fields.
#[derive(Default)]
struct PythonStrings(HashMap<TextSize, Option<String>>);

impl<'a> Visitor<'a> for PythonStrings {
    fn visit_string_literal(&mut self, literal: &'a StringLiteral) {
        self.0.insert(literal.start(), Some(literal.value.to_string()));
    }

    fn visit_bytes_literal(&mut self, literal: &'a BytesLiteral) {
        self.0.insert(literal.start(), None);
    }

    fn visit_f_string(&mut self, f_string: &'a FString) {
        let mut value = String::new();
        let mut constant = true;
        for element in &f_string.elements {
            match element {
                InterpolatedStringElement::Literal(literal) => value.push_str(&literal.value),
                InterpolatedStringElement::Interpolation(_) => constant = false,
            }
        }
        self.0.insert(f_string.start(), constant.then_some(value));
        py_visit::walk_f_string(self, f_string);
    }
}

/// A Python token as the extractors see it; an f-string is one string.
enum PyToken<'a> {
    Name(&'a str),
    Lpar,
    Rpar,
    Colon,
    Comma,
    /// The value of a string, `None` when it is not constant text.
    Str(Option<&'a str>),
    Other,
}

fn python_tokens<'a>(
    source: &'a str,
    parsed: &ruff_python_parser::Parsed<ruff_python_ast::ModModule>,
    strings: &'a PythonStrings,
) -> Vec<PyToken<'a>> {
    let value = |start: &TextSize| strings.0.get(start).and_then(Option::as_deref);
    let mut tokens = Vec::new();
    let mut f_string: Option<(TextSize, usize)> = None;
    for token in parsed.tokens().iter() {
        let kind = token.kind();
        match (kind, &mut f_string) {
            (TokenKind::FStringStart | TokenKind::TStringStart, Some((_, depth))) => *depth += 1,
            (TokenKind::FStringStart | TokenKind::TStringStart, None) => f_string = Some((token.start(), 1)),
            (TokenKind::FStringEnd | TokenKind::TStringEnd, Some((start, depth))) => {
                *depth -= 1;
                if *depth == 0 {
                    tokens.push(PyToken::Str(value(start)));
                    f_string = None;
                }
            }
            (_, Some(_)) => {}
            _ => tokens.push(match kind {
                TokenKind::Lpar => PyToken::Lpar,
                TokenKind::Rpar => PyToken::Rpar,
                TokenKind::Colon => PyToken::Colon,
                TokenKind::Comma => PyToken::Comma,
                TokenKind::String => PyToken::Str(value(&token.start())),
                _ if kind == TokenKind::Identifier || kind.is_keyword() || kind.is_soft_keyword() => {
                    PyToken::Name(&source[token.range()])
                }
                _ => PyToken::Other,
            }),
        }
    }
    tokens
}

fn is_python_keyword(name: &str) -> bool {
    matches!(name, "_" | "_lt")
}

/// The terms of `_()` and `_lt()` calls in Python: the strings of the first
/// argument, joined, when it has any.
pub fn python_terms(source: &str, extractor: Extractor) -> Vec<String> {
    let Ok(parsed) = ruff_python_parser::parse_module(source) else {
        return Vec::new();
    };
    let mut strings = PythonStrings::default();
    strings.visit_body(&parsed.syntax().body);
    let tokens = python_tokens(source, &parsed, &strings);
    let messages = match extractor {
        Extractor::Odoo => odoo_python_messages(&tokens),
        Extractor::Babel => babel_python_messages(&tokens),
    };
    // Babel's `extract` drops calls whose first argument has no string.
    messages
        .into_iter()
        .filter_map(|m| m.into_iter().next().flatten())
        .filter(|m| !m.is_empty())
        .collect()
}

fn take(buffer: &mut Vec<&str>) -> Option<String> {
    let joined = (!buffer.is_empty()).then(|| buffer.concat());
    buffer.clear();
    joined
}

/// odoo/tools/babel/python_extractor.py: any call is tracked, so terms in
/// nested calls are found; every `(` after a name opens a call.
fn odoo_python_messages(tokens: &[PyToken]) -> Vec<Vec<Option<String>>> {
    struct OpenCall {
        keyword: bool,
        messages: Vec<Option<String>>,
    }
    let mut calls = Vec::new();
    let mut stack: Vec<OpenCall> = Vec::new();
    let mut buffer: Vec<&str> = Vec::new();
    let mut last_name: Option<&str> = None;
    let mut in_def = false;
    for token in tokens {
        if let PyToken::Name("def" | "class") = token {
            in_def = true;
            continue;
        }
        if in_def && matches!(token, PyToken::Lpar | PyToken::Colon) {
            in_def = false;
            continue;
        }
        if matches!(token, PyToken::Lpar) && last_name.is_some() {
            stack.push(OpenCall {
                keyword: last_name.is_some_and(is_python_keyword),
                messages: Vec::new(),
            });
            buffer.clear();
        } else if stack.last().is_some_and(|call| call.keyword) {
            match token {
                PyToken::Rpar => {
                    if let Some(mut call) = stack.pop() {
                        call.messages.push(take(&mut buffer));
                        calls.push(call.messages);
                    }
                }
                PyToken::Str(Some(value)) => buffer.push(value),
                PyToken::Comma => {
                    let message = take(&mut buffer);
                    if let Some(call) = stack.last_mut() {
                        call.messages.push(message);
                    }
                }
                _ => {}
            }
        } else if matches!(token, PyToken::Rpar) {
            stack.pop();
        }
        if let PyToken::Name(name) = token {
            last_name = Some(name);
        }
    }
    calls
}

/// Babel's `extract_python`: a keyword followed by `(` opens a call; strings
/// in parentheses inside it are not read, nor are calls inside other calls.
/// Babel evaluates each string of a call; on the Pythons Odoo 18 and older
/// run (before 3.12) an f-string with fields fails there, and Odoo keeps only
/// the terms found before it in the file.
fn babel_python_messages(tokens: &[PyToken]) -> Vec<Vec<Option<String>>> {
    let mut calls = Vec::new();
    let mut funcname: Option<&str> = None;
    let mut depth: i32 = -1;
    let mut messages: Vec<Option<String>> = Vec::new();
    let mut buffer: Vec<&str> = Vec::new();
    let mut in_def = false;
    for token in tokens {
        match token {
            PyToken::Name("def" | "class") if depth == -1 => in_def = true,
            PyToken::Lpar => {
                if in_def {
                    in_def = false;
                } else if funcname.is_some() {
                    depth += 1;
                }
            }
            PyToken::Colon if in_def => in_def = false,
            _ if funcname.is_some() && depth == 0 => {
                let nested = matches!(token, PyToken::Name(name) if is_python_keyword(name));
                match token {
                    PyToken::Rpar | PyToken::Name(_) if matches!(token, PyToken::Rpar) || nested => {
                        messages.push(take(&mut buffer));
                        calls.push(std::mem::take(&mut messages));
                        depth = -1;
                        funcname = match token {
                            PyToken::Name(name) if nested => Some(name),
                            _ => None,
                        };
                    }
                    PyToken::Str(Some(value)) => buffer.push(value),
                    PyToken::Str(None) => break,
                    PyToken::Comma => messages.push(take(&mut buffer)),
                    _ => {}
                }
            }
            PyToken::Rpar if depth > 0 => depth -= 1,
            _ if funcname.is_some() && depth == -1 => funcname = None,
            PyToken::Name(name) if is_python_keyword(name) => funcname = Some(name),
            _ => {}
        }
    }
    calls
}

/// The terms of `_t()` calls in JavaScript: the strings of the first argument
/// outside nested calls, joined (odoo/tools/babel/javascript_extractor.py),
/// or with Babel the last one unless they are added together with `+`.
pub fn javascript_terms(source: &str, extractor: Extractor) -> Vec<String> {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::mjs()).parse();
    let mut collector = JsTerms {
        source,
        extractor,
        terms: Vec::new(),
        in_template: 0,
    };
    collector.visit_program(&parsed.program);
    collector.terms
}

struct JsTerms<'s> {
    source: &'s str,
    extractor: Extractor,
    terms: Vec<String>,
    /// How many template literals the visitor is inside: Babel reads a
    /// template literal as one token, so it misses calls in its `${}`.
    in_template: usize,
}

fn is_translate_callee(callee: &Expression) -> bool {
    match callee {
        Expression::Identifier(identifier) => identifier.name == "_t",
        Expression::StaticMemberExpression(member) => member.property.name == "_t",
        _ => false,
    }
}

impl JsTerms<'_> {
    /// The strings of an argument in source order, skipping nested calls.
    fn strings(expression: &Expression, out: &mut Vec<String>) {
        match expression {
            Expression::StringLiteral(literal) => out.push(literal.value.to_string()),
            Expression::ParenthesizedExpression(inner) => Self::strings(&inner.expression, out),
            Expression::BinaryExpression(binary) => {
                Self::strings(&binary.left, out);
                Self::strings(&binary.right, out);
            }
            Expression::LogicalExpression(logical) => {
                Self::strings(&logical.left, out);
                Self::strings(&logical.right, out);
            }
            Expression::ConditionalExpression(conditional) => {
                Self::strings(&conditional.test, out);
                Self::strings(&conditional.consequent, out);
                Self::strings(&conditional.alternate, out);
            }
            _ => {}
        }
    }

    /// A template literal as the extractor reads it: Odoo's takes the raw
    /// text up to and including the first `${`, Babel's the whole text.
    fn template_text(&self, template: &oxc_ast::ast::TemplateLiteral) -> String {
        let span = template.span;
        let raw = &self.source[span.start as usize + 1..span.end as usize - 1];
        match self.extractor {
            Extractor::Odoo => match raw.find("${") {
                Some(at) => raw[..at + 2].to_string(),
                None => raw.to_string(),
            },
            Extractor::Babel => match template.quasis.first().and_then(|q| q.value.cooked.as_ref()) {
                Some(cooked) if template.expressions.is_empty() => cooked.to_string(),
                _ => raw.to_string(),
            },
        }
    }
}

impl<'a> Visit<'a> for JsTerms<'_> {
    fn visit_template_literal(&mut self, template: &oxc_ast::ast::TemplateLiteral<'a>) {
        self.in_template += 1;
        walk::walk_template_literal(self, template);
        self.in_template -= 1;
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        let hidden = self.extractor == Extractor::Babel && self.in_template > 0;
        if is_translate_callee(&call.callee) && !hidden {
            let message = match call.arguments.first() {
                Some(Argument::TemplateLiteral(template)) => Some(self.template_text(template)),
                Some(argument) => argument.as_expression().and_then(|expression| {
                    let mut strings = Vec::new();
                    Self::strings(expression, &mut strings);
                    let added = matches!(expression, Expression::BinaryExpression(b) if b.operator.as_str() == "+");
                    match self.extractor {
                        Extractor::Babel if !added => strings.pop(),
                        _ => (!strings.is_empty()).then(|| strings.concat()),
                    }
                }),
                None => None,
            };
            if let Some(message) = message.filter(|m| !m.is_empty()) {
                self.terms.push(message);
            }
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_tagged_template_expression(&mut self, tagged: &TaggedTemplateExpression<'a>) {
        if is_translate_callee(&tagged.tag) && tagged.quasi.expressions.is_empty() {
            if let Some(cooked) = tagged.quasi.quasis.first().and_then(|q| q.value.cooked.as_ref()) {
                if !cooked.is_empty() {
                    self.terms.push(cooked.to_string());
                }
            }
        }
        walk::walk_tagged_template_expression(self, tagged);
    }
}

/// Elements whose content QWeb never translates.
const SKIPPED_ELEMENTS: &[&str] = &["script", "style", "title"];

/// Attributes from QWeb views that are translated (and their `t-attf-` forms).
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

/// The attributes OWL translates on HTML elements (`translatableAttributes`).
const OWL_TRANSLATED_ATTRS: &[&str] = &[
    "alt",
    "aria-label",
    "aria-placeholder",
    "aria-roledescription",
    "aria-valuetext",
    "data-tooltip",
    "label",
    "placeholder",
    "title",
];

fn is_translatable_attrib(key: Option<&str>, node: roxmltree::Node) -> bool {
    let Some(key) = key.filter(|k| !k.is_empty()) else {
        return false;
    };
    let attr = key.strip_prefix("t-attf-").unwrap_or(key);
    (!node.has_attribute("t-call") && TRANSLATED_ATTRS.contains(&attr)) || key.ends_with(".translate")
}

/// The terms of a QWeb template file (`babel_extract_qweb`): texts and the
/// translated attributes, each stripped.
pub fn qweb_terms(source: &str) -> Vec<String> {
    let Ok(doc) = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..roxmltree::ParsingOptions::default()
        },
    ) else {
        return Vec::new();
    };
    let mut terms = Vec::new();
    qweb_walk(doc.root_element(), &mut terms);
    terms
}

fn push_term(terms: &mut Vec<String>, text: Option<&str>) {
    if let Some(text) = text.map(str::trim).filter(|t| exported(t)) {
        terms.push(text.to_string());
    }
}

/// lxml's `.text`: the text before the first child node.
fn text<'a>(node: roxmltree::Node<'a, '_>) -> Option<&'a str> {
    node.first_child()
        .filter(roxmltree::Node::is_text)
        .and_then(|n| n.text())
}

/// lxml's `.tail`: the text right after the element.
fn tail<'a>(node: roxmltree::Node<'a, '_>) -> Option<&'a str> {
    node.next_sibling()
        .filter(roxmltree::Node::is_text)
        .and_then(|n| n.text())
}

fn qweb_walk(element: roxmltree::Node, terms: &mut Vec<String>) {
    // Comments and processing instructions are skipped with the text after them.
    for child in element.children().filter(roxmltree::Node::is_element) {
        let tag = child.tag_name().name();
        let skipped = SKIPPED_ELEMENTS.contains(&tag.to_lowercase().as_str())
            || child.has_attribute("t-js")
            || (tag == "attribute" && !is_translatable_attrib(child.attribute("name"), child))
            || child.attribute("t-translation").is_some_and(|v| v.trim() == "off");
        if !skipped {
            push_term(terms, text(child));
            let is_component = tag.starts_with(|c: char| c.is_uppercase())
                || child.has_attribute("t-component")
                || child.has_attribute("t-set-slot");
            for attribute in child.attributes() {
                let name = attribute.name();
                if (!is_component && OWL_TRANSLATED_ATTRS.contains(&name))
                    || (is_component && name.ends_with(".translate"))
                {
                    push_term(terms, Some(attribute.value()));
                }
            }
            qweb_walk(child, terms);
        }
        push_term(terms, tail(child));
    }
}

/// The Odoo series of a module: the first two parts of a five-part manifest
/// version, else `default`.
pub fn module_version(dir: &Path, default: OdooVersion) -> OdooVersion {
    std::fs::read_to_string(dir.join("__manifest__.py"))
        .ok()
        .and_then(|source| {
            let manifest = Manifest::parse(&source)?;
            let version = manifest.get_str("version")?;
            let parts: Vec<&str> = version.split('.').collect();
            (parts.len() == 5).then(|| format!("{}.{}", parts[0], parts[1]).parse().ok())?
        })
        .unwrap_or(default)
}

/// The template of the module in `dir`.
pub fn generate(dir: &Path, default_version: OdooVersion) -> Option<String> {
    let module = dir.file_name()?.to_str()?;
    let version = module_version(dir, default_version);
    let terms = code_terms(dir, module, Extractor::for_version(version));
    Some(write(module, version, &terms))
}

/// What `odl pot` did to one template.
pub struct Change {
    pub path: PathBuf,
    /// Whether the file was written (`false` in check mode).
    pub written: bool,
}

/// Writes `i18n/<module>.pot` for the modules under `paths`, or into
/// `output_dir` when given; in check mode only reports the outdated ones.
///
/// # Errors
///
/// When a template or its folder cannot be written.
pub fn run(
    paths: &[PathBuf],
    check: bool,
    output_dir: Option<&Path>,
    default_version: OdooVersion,
) -> std::io::Result<Vec<Change>> {
    let mut changes = Vec::new();
    for dir in crate::readme::modules(paths) {
        let Some(module) = dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        let Some(content) = generate(&dir, default_version) else {
            continue;
        };
        let path = match output_dir {
            Some(out) => out.join(format!("{module}.pot")),
            None => dir.join("i18n").join(format!("{module}.pot")),
        };
        if std::fs::read_to_string(&path).is_ok_and(|current| current == content) {
            continue;
        }
        if !check {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &content)?;
        }
        changes.push(Change { path, written: !check });
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod python_terms {
        use super::*;

        fn odoo(source: &str) -> Vec<String> {
            python_terms(source, Extractor::Odoo)
        }

        fn babel(source: &str) -> Vec<String> {
            python_terms(source, Extractor::Babel)
        }

        #[test]
        fn reads_the_first_argument_only() {
            assert_eq!(odoo(r#"_("Nothing to %s", "do")"#), ["Nothing to %s"]);
        }

        #[test]
        fn reads_env_translation_and_lazy_calls() {
            assert_eq!(odoo(r#"self.env._("With env") + _lt("Lazy")"#), ["With env", "Lazy"]);
        }

        #[test]
        fn joins_implicitly_concatenated_strings() {
            assert_eq!(odoo("_(\n    \"Split \"\n    \"over lines\"\n)"), ["Split over lines"]);
        }

        #[test]
        fn reads_constant_f_strings() {
            assert_eq!(odoo(r#"_(f"Constant")"#), ["Constant"]);
        }

        #[test]
        fn skips_calls_without_a_string() {
            assert!(odoo(r#"_(name); _(foo("Not a term")); _(""); print("No")"#).is_empty());
        }

        #[test]
        fn ignores_definitions_named_like_keywords() {
            assert!(odoo("def _(self, arg='Default'):\n    pass\n").is_empty());
        }

        #[test]
        fn odoo_reads_a_parenthesis_after_a_call_as_a_new_call() {
            let source = "{\"name\": _(\"Products\"), \"domain\": [(\"id\", \"in\", ids)]}";
            assert_eq!(odoo(source), ["Products", "id"]);
        }

        #[test]
        fn babel_reads_only_the_call_itself() {
            let source = "{\"name\": _(\"Products\"), \"domain\": [(\"id\", \"in\", ids)]}";
            assert_eq!(babel(source), ["Products"]);
        }

        #[test]
        fn odoo_reads_strings_in_nested_parentheses() {
            assert_eq!(odoo(r#"_(("Wrapped " "message"), name=1)"#), ["Wrapped message"]);
        }

        #[test]
        fn babel_skips_strings_in_nested_parentheses() {
            assert!(babel(r#"_(("Wrapped " "message"), name=1)"#).is_empty());
        }

        #[test]
        fn odoo_skips_an_f_string_with_fields() {
            let source = "_(\"Before\")\n_(f\"Batch {name}: \" \"failed\")\n_(\"After\")\n";
            assert_eq!(odoo(source), ["Before", "failed", "After"]);
        }

        #[test]
        fn babel_stops_the_file_at_an_f_string_with_fields() {
            let source = "_(\"Before\")\n_(f\"Batch {name}: \" \"failed\")\n_(\"After\")\n";
            assert_eq!(babel(source), ["Before"]);
        }
    }

    mod javascript_terms {
        use super::*;

        fn odoo(source: &str) -> Vec<String> {
            javascript_terms(source, Extractor::Odoo)
        }

        fn babel(source: &str) -> Vec<String> {
            javascript_terms(source, Extractor::Babel)
        }

        #[test]
        fn reads_calls_and_member_calls() {
            assert_eq!(odoo(r#"_t("Hello"); this.env._t("Via env");"#), ["Hello", "Via env"]);
        }

        #[test]
        fn joins_added_strings() {
            assert_eq!(babel(r#"_t("Joined " + "strings");"#), ["Joined strings"]);
        }

        #[test]
        fn reads_the_first_argument_of_nested_calls() {
            assert_eq!(odoo(r#"notify(_t("Saved %s", record.name));"#), ["Saved %s"]);
        }

        #[test]
        fn skips_calls_without_a_string() {
            assert!(odoo("_t(name);").is_empty());
        }

        #[test]
        fn odoo_cuts_a_template_after_its_first_field() {
            assert_eq!(odoo("_t(`Partial ${x} text`);"), ["Partial ${"]);
        }

        #[test]
        fn babel_keeps_the_whole_template() {
            assert_eq!(babel("_t(`Partial ${x} text`);"), ["Partial ${x} text"]);
        }

        #[test]
        fn odoo_reads_calls_inside_template_fields() {
            assert_eq!(odoo(r#"`${count}${_t("s")}`;"#), ["s"]);
        }

        #[test]
        fn babel_misses_calls_inside_template_fields() {
            assert!(babel(r#"`${count}${_t("s")}`;"#).is_empty());
        }
    }

    mod qweb_terms {
        use super::*;

        fn terms(body: &str) -> Vec<String> {
            qweb_terms(&format!("<templates><t t-name=\"acme.Widget\">{body}</t></templates>"))
        }

        #[test]
        fn reads_text_and_tails_stripped() {
            assert_eq!(
                terms("<span>Hello <b>world</b> again</span>"),
                ["Hello", "world", "again"]
            );
        }

        #[test]
        fn reads_translated_html_attributes() {
            assert_eq!(terms(r#"<input class="x" placeholder="Search..."/>"#), ["Search..."]);
        }

        #[test]
        fn reads_only_translate_attributes_on_components() {
            let body = r#"<MyComponent label="Not this" title.translate="Component title"/>"#;
            assert_eq!(terms(body), ["Component title"]);
        }

        #[test]
        fn skips_the_text_after_a_comment() {
            assert!(terms("<div><!-- note --> after comment</div>").is_empty());
        }

        #[test]
        fn skips_scripts_but_not_their_tail() {
            assert_eq!(terms("<div><script>skipped</script> tail</div>"), ["tail"]);
        }

        #[test]
        fn skips_untranslated_elements() {
            assert!(terms(r#"<p t-translation="off">Untranslated</p>"#).is_empty());
        }

        #[test]
        fn skips_text_without_letters() {
            assert!(terms("<span>123</span>").is_empty());
        }
    }

    mod spreadsheet_terms {
        use super::*;

        const DASHBOARD: &str = r#"{
            "sheets": [{
                "cells": {
                    "A1": "=_t(\"Top Vendors\")",
                    "A2": {"content": "[Open orders](odoo://view/1)"},
                    "A3": "Plain text",
                    "A4": "=PIVOT(1) + _t(\" days\")"
                },
                "figures": [{"tag": "chart", "data": {
                    "title": {"text": "Amount"},
                    "axesDesign": {"x": {"title": {"text": "Period"}}}
                }}]
            }],
            "globalFilters": [{"label": "Buyer"}, {"label": "123"}]
        }"#;

        const TEXT_BASELINE: &str = r#"{"sheets": [{"cells": {}, "figures": [{"tag": "chart", "data": {
            "title": "Purchased", "baselineDescr": "since last period"
        }}]}]}"#;

        #[test]
        fn reads_formulas_links_charts_and_filters() {
            assert_eq!(
                spreadsheet_terms(DASHBOARD, Extractor::Odoo),
                [" days", "Amount", "Buyer", "Open orders", "Period", "Top Vendors"]
            );
        }

        #[test]
        fn babel_reads_a_text_baseline_description() {
            assert_eq!(
                spreadsheet_terms(TEXT_BASELINE, Extractor::Babel),
                ["Purchased", "since last period"]
            );
        }

        #[test]
        fn odoo_reads_only_a_baseline_description_object() {
            assert_eq!(spreadsheet_terms(TEXT_BASELINE, Extractor::Odoo), ["Purchased"]);
        }
    }

    #[test]
    fn write_groups_by_msgid_and_sorts_like_odoo() {
        let terms = vec![
            Term::code("Zeta".into(), "addons/acme/models/b.py", PYTHON_COMMENT),
            Term::code("Alpha".into(), "addons/acme/models/b.py", PYTHON_COMMENT),
            Term::code("Alpha".into(), "addons/acme/models/a.py", PYTHON_COMMENT),
            Term::code("Alpha".into(), "addons/acme/models/a.py", PYTHON_COMMENT),
            Term::code(".00".into(), "addons/acme/models/a.py", PYTHON_COMMENT),
        ];
        assert_eq!(
            write("acme", OdooVersion::new(19, 0), &terms),
            r#"# Translation of Odoo Server.
# This file contains the translation of the following modules:
# 	* acme
#
msgid ""
msgstr ""
"Project-Id-Version: Odoo Server 19.0\n"
"Report-Msgid-Bugs-To: \n"
"Last-Translator: \n"
"Language-Team: \n"
"MIME-Version: 1.0\n"
"Content-Type: text/plain; charset=UTF-8\n"
"Content-Transfer-Encoding: \n"
"Plural-Forms: \n"

#. module: acme
#. odoo-python
#: code:addons/acme/models/a.py:0 code:addons/acme/models/b.py:0
msgid "Alpha"
msgstr ""

#. module: acme
#. odoo-python
#: code:addons/acme/models/b.py:0
msgid "Zeta"
msgstr ""
"#
        );
    }
}
