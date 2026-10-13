//! Completion of XML ids, models and fields in Odoo files, from the index of
//! the addons path: where go to definition finds a name, completion offers
//! the names that fit. The language server answers
//! `textDocument/completion` with it.

use crate::checker::ModuleInfo;
use crate::definition::{python_target, xml_target, Target};
use crate::index::{closure, Closure};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    XmlId,
    Model,
    Field,
}

/// A name to offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub label: String,
    pub kind: Kind,
    /// The field type and comodel, or the module of an XML id or a model.
    pub detail: String,
}

/// What completion offers at a position: the names, and the byte range of
/// the text they replace (from the start of the name to the cursor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub start: usize,
    pub candidates: Vec<Candidate>,
}

/// The text with the value at `offset` closed, as an editor shows it while
/// a name is typed: `<field name="pa` has no closing quote and tag yet.
fn repairs(text: &str, offset: usize, extension: &str) -> Vec<String> {
    let before = &text[..offset];
    let quote = before
        .rfind(['"', '\''])
        .map_or('"', |i| before[i..].chars().next().unwrap_or('"'));
    let insert = |piece: &str| format!("{before}{piece}{}", &text[offset..]);
    match extension {
        "xml" => vec![insert(&format!("{quote}/>")), insert(&format!("{quote}>"))],
        _ => {
            // Close the string and the brackets the statement opened, with
            // or without what follows on the line.
            let closers = open_brackets(before);
            let line_end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
            let mut repaired = vec![
                insert(&quote.to_string()),
                insert(&format!("{quote}{closers}")),
                format!("{before}{quote}{closers}{}", &text[line_end..]),
            ];
            // `order.` before the field is typed: an attribute to complete.
            if before.ends_with('.') {
                repaired.push(insert("_"));
                repaired.push(format!("{before}_{closers}{}", &text[line_end..]));
            }
            repaired
        }
    }
}

/// The brackets to close the ones opened on the lines of the statement
/// that ends at the end of `before`, innermost first.
fn open_brackets(before: &str) -> String {
    // A statement rarely spans more than the last 20 lines.
    let start = before.rmatch_indices('\n').nth(20).map_or(0, |(i, _)| i + 1);
    let mut stack: Vec<char> = Vec::new();
    let mut quote: Option<char> = None;
    let mut previous = ' ';
    for c in before[start..].chars() {
        match quote {
            Some(q) if c == q && previous != '\\' => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' => quote = Some(c),
                '(' => stack.push(')'),
                '[' => stack.push(']'),
                '{' => stack.push('}'),
                ')' | ']' | '}' => {
                    stack.pop();
                }
                _ => {}
            },
        }
        previous = c;
    }
    stack.iter().rev().collect()
}

fn target(text: &str, offset: usize, module: &str, extension: &str) -> Option<(Target, usize)> {
    let find = |text: &str| match extension {
        "xml" => xml_target(text, offset, module),
        "py" => python_target(text, offset, module),
        _ => None,
    };
    find(text).or_else(|| repairs(text, offset, extension).iter().find_map(|t| find(t)))
}

fn xml_ids(closure: &Closure, module: &str) -> Vec<Candidate> {
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    for index in closure.modules() {
        for id in index.xml_ids.keys() {
            // The module's own ids without the module name, as it writes them.
            let label = match id.split_once('.') {
                Some((owner, name)) if owner == module => name.to_string(),
                _ => id.clone(),
            };
            found.insert(label, index.name.clone());
        }
    }
    found
        .into_iter()
        .map(|(label, module)| Candidate {
            label,
            kind: Kind::XmlId,
            detail: module,
        })
        .collect()
}

fn models(closure: &Closure) -> Vec<Candidate> {
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    for index in closure.modules() {
        for class in &index.classes {
            if let Some(name) = &class.name {
                found.entry(name.clone()).or_insert_with(|| index.name.clone());
            }
        }
    }
    found
        .into_iter()
        .map(|(label, module)| Candidate {
            label,
            kind: Kind::Model,
            detail: module,
        })
        .collect()
}

fn fields(closure: &Closure, model: &str) -> Vec<Candidate> {
    let Some(fields) = closure.fields(model) else {
        return Vec::new();
    };
    let mut candidates: Vec<Candidate> = fields
        .values()
        .map(|field| Candidate {
            label: field.name.clone(),
            kind: Kind::Field,
            detail: match &field.comodel {
                Some(comodel) => format!("{} → {comodel}", field.kind),
                None => field.kind.clone(),
            },
        })
        .collect();
    candidates.sort_by(|a, b| a.label.cmp(&b.label));
    candidates
}

/// The model reached from `model` through the relational fields `path`.
fn follow(closure: &Closure, model: &str, path: &[&str]) -> Option<String> {
    let mut current = model.to_string();
    for step in path {
        current = closure.fields(&current)?.get(*step)?.comodel.clone()?;
    }
    Some(current)
}

/// The completion at byte `offset` of `text`, the content of `file` in
/// `module`.
pub fn completion(
    module: &ModuleInfo,
    addons_path: &[PathBuf],
    file: &Path,
    text: &str,
    offset: usize,
) -> Option<Completion> {
    let extension = file.extension()?.to_str()?;
    let (target, start) = target(text, offset, &module.name, extension)?;
    let closure = closure(module, addons_path)?;
    let candidates = match target {
        Target::XmlId(_) => xml_ids(&closure, &module.name),
        Target::Model(_) => models(&closure),
        Target::Field { models, path, segment } => {
            let segments: Vec<&str> = path.split('.').collect();
            let before = &segments[..segment.min(segments.len())];
            let mut found: Vec<Candidate> = Vec::new();
            for model in &models {
                if let Some(current) = follow(&closure, model, before) {
                    found.extend(fields(&closure, &current));
                }
            }
            found.sort_by(|a, b| a.label.cmp(&b.label));
            found.dedup_by(|a, b| a.label == b.label);
            found
        }
        Target::ViewField { model, via, .. } => {
            let via: Vec<&str> = via.iter().map(String::as_str).collect();
            follow(&closure, &model, &via)
                .map(|current| fields(&closure, &current))
                .unwrap_or_default()
        }
    };
    Some(Completion { start, candidates })
}
