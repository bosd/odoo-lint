//! Go to definition: from an XML id, a model or a field named in an Odoo
//! file to where it is defined, using the index of the addons path. The
//! language server answers `textDocument/definition` with it.

use crate::checker::ModuleInfo;
use crate::index::{class_models, closure, module_index, Closure};
use crate::manifest::MANIFEST_FILE_NAMES;
use crate::semantic::{func_lib, func_name};
use crate::visit::{enclosing_class, walk, Node};
use regex::Regex;
use ruff_python_ast::{Expr, Stmt};
use ruff_text_size::Ranged;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Where something is defined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub path: PathBuf,
    /// 1-based.
    pub line: usize,
}

/// What the cursor is on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    XmlId(String),
    Model(String),
    /// A field path (`partner_id.country_id`) on one of `models`, and the
    /// segment the cursor is on.
    Field {
        models: Vec<String>,
        path: String,
        segment: usize,
    },
    /// A `<field name="...">` in a view of `model`, inside the sub-views of
    /// the fields `via`.
    ViewField {
        model: String,
        via: Vec<String>,
        name: String,
    },
}

static EVAL_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bref\(\s*['"]([\w.]+)['"]"#).unwrap());
static ACTION_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"%\(([\w.]+)\)d").unwrap());

fn qualify(module: &str, id: &str) -> String {
    if id.contains('.') {
        id.to_string()
    } else {
        format!("{module}.{id}")
    }
}

/// The comma-separated item of `value` (starting at byte `start` of the
/// text) at `offset`.
fn item_at(value: &str, start: usize, offset: usize) -> Option<&str> {
    let mut position = start;
    for item in value.split(',') {
        if (position..=position + item.len()).contains(&offset) {
            return Some(item.trim().trim_start_matches('!'));
        }
        position += item.len() + 1;
    }
    None
}

/// The capture 1 of `pattern` in `value` (at `start`) that holds `offset`.
fn capture_at(pattern: &Regex, value: &str, start: usize, offset: usize) -> Option<String> {
    pattern.captures_iter(value).find_map(|c| {
        let m = c.get(1)?;
        (start + m.start() <= offset && offset <= start + m.end()).then(|| m.as_str().to_string())
    })
}

fn xml_target(text: &str, offset: usize, module: &str) -> Option<Target> {
    let doc = roxmltree::Document::parse(text).ok()?;
    for node in doc.descendants() {
        if node.is_text() {
            let range = node.range();
            if !(range.start <= offset && offset <= range.end) {
                continue;
            }
            // `<field name="model">sale.order</field>` and the like.
            let parent = node.parent_element()?;
            let name = parent.attribute("name").unwrap_or_default();
            if parent.has_tag_name("field") && matches!(name, "model" | "res_model" | "src_model" | "binding_model") {
                return Some(Target::Model(node.text()?.trim().to_string()));
            }
            continue;
        }
        if !node.is_element() {
            continue;
        }
        for attribute in node.attributes() {
            let range = attribute.range_value();
            if !(range.start <= offset && offset <= range.end) {
                continue;
            }
            let value = attribute.value();
            let tag = node.tag_name().name();
            if let Some(id) = capture_at(&ACTION_REF, value, range.start, offset) {
                return Some(Target::XmlId(qualify(module, &id)));
            }
            return match attribute.name() {
                "ref" | "inherit_id" => Some(Target::XmlId(qualify(module, value.trim()))),
                "parent" | "action" if tag == "menuitem" => Some(Target::XmlId(qualify(module, value.trim()))),
                "groups" => item_at(value, range.start, offset).map(|g| Target::XmlId(qualify(module, g))),
                "t-call" if value.contains('.') => Some(Target::XmlId(value.trim().to_string())),
                "eval" => {
                    capture_at(&EVAL_REF, value, range.start, offset).map(|id| Target::XmlId(qualify(module, &id)))
                }
                "model" if matches!(tag, "record" | "function" | "report" | "delete") => {
                    Some(Target::Model(value.trim().to_string()))
                }
                "name" if tag == "field" => view_field(node, value),
                _ => None,
            };
        }
    }
    None
}

/// `<field name="...">` in a data record or in a view's arch.
fn view_field(node: roxmltree::Node, name: &str) -> Option<Target> {
    let parent = node.parent_element()?;
    // A field of a data record.
    if parent.has_tag_name("record") {
        let model = parent.attribute("model")?;
        return Some(Target::Field {
            models: vec![model.to_string()],
            path: name.to_string(),
            segment: 0,
        });
    }
    // A field in an arch: the view's model, through the sub-views it is in.
    let arch = node
        .ancestors()
        .find(|a| a.has_tag_name("field") && a.attribute("name") == Some("arch"))?;
    let record = arch.parent_element().filter(|r| r.has_tag_name("record"))?;
    let model = record
        .children()
        .find(|c| c.has_tag_name("field") && c.attribute("name") == Some("model"))?
        .text()?
        .trim()
        .to_string();
    let mut via: Vec<String> = node
        .ancestors()
        .skip(1)
        .take_while(|a| *a != arch)
        .filter(|a| a.has_tag_name("field"))
        .filter_map(|a| a.attribute("name").map(str::to_string))
        .collect();
    via.reverse();
    Some(Target::ViewField {
        model,
        via,
        name: name.to_string(),
    })
}

fn python_target(text: &str, offset: usize, module: &str) -> Option<Target> {
    let parsed = ruff_python_parser::parse_module(text).ok()?;
    let holds = |expr: &Expr| {
        expr.as_string_literal_expr().is_some()
            && expr.range().start().to_usize() <= offset
            && offset <= expr.range().end().to_usize()
    };
    let string = |expr: &Expr| expr.as_string_literal_expr().map(|s| s.value.to_str().to_string());
    // The offset of the string's content: after its quotes.
    let content_start = |expr: &Expr| {
        let source = &text[expr.range().start().to_usize()..expr.range().end().to_usize()];
        expr.range().start().to_usize() + source.find(['\'', '"']).map_or(0, |q| q + 1)
    };
    let segment = |expr: &Expr| {
        text[content_start(expr)..offset.max(content_start(expr))]
            .matches('.')
            .count()
    };
    let mut found = None;
    walk(parsed.suite(), |node, scopes| {
        if found.is_some() {
            return;
        }
        let class_models = || enclosing_class(scopes).map(class_models).unwrap_or_default();
        match node {
            Node::Expr(Expr::Call(call)) => {
                let name = func_name(&call.func);
                for (i, arg) in call.arguments.args.iter().enumerate() {
                    if !holds(arg) {
                        continue;
                    }
                    let value = string(arg).unwrap_or_default();
                    found = match name {
                        "ref" => Some(Target::XmlId(qualify(module, &value))),
                        "has_group" | "has_groups" | "user_has_groups" => {
                            item_at(&value, content_start(arg), offset).map(|g| Target::XmlId(qualify(module, g)))
                        }
                        "depends" | "onchange" | "constrains" | "mapped" | "filtered" | "sorted" => {
                            Some(Target::Field {
                                models: class_models(),
                                path: value,
                                segment: segment(arg),
                            })
                        }
                        "Many2one" | "One2many" | "Many2many" if func_lib(&call.func) == "fields" && i == 0 => {
                            Some(Target::Model(value))
                        }
                        _ => None,
                    };
                }
                for keyword in &call.arguments.keywords {
                    if !holds(&keyword.value) {
                        continue;
                    }
                    let value = string(&keyword.value).unwrap_or_default();
                    found = match keyword.arg.as_ref().map(|a| a.as_str()) {
                        Some("comodel_name") => Some(Target::Model(value)),
                        Some("related") => Some(Target::Field {
                            models: class_models(),
                            path: value,
                            segment: segment(&keyword.value),
                        }),
                        _ => None,
                    };
                }
            }
            // `self.env["sale.order"]`
            Node::Expr(Expr::Subscript(subscript)) if holds(&subscript.slice) => {
                if matches!(&*subscript.value, Expr::Attribute(a) if a.attr.as_str() == "env")
                    || matches!(&*subscript.value, Expr::Name(n) if n.id.as_str() == "env")
                {
                    found = string(&subscript.slice).map(Target::Model);
                }
            }
            // `_name`, `_inherit` and `_inherits` in a class body.
            Node::Stmt(Stmt::Assign(assign)) => {
                let target = assign
                    .targets
                    .first()
                    .and_then(|t| t.as_name_expr())
                    .map(|n| n.id.as_str());
                if !matches!(target, Some("_name" | "_inherit" | "_inherits")) {
                    return;
                }
                let items: Vec<&Expr> = match &*assign.value {
                    Expr::List(list) => list.elts.iter().collect(),
                    Expr::Tuple(tuple) => tuple.elts.iter().collect(),
                    Expr::Dict(dict) => dict.items.iter().filter_map(|i| i.key.as_ref()).collect(),
                    other => vec![other],
                };
                found = items.into_iter().find(|e| holds(e)).and_then(string).map(Target::Model);
            }
            _ => {}
        }
    });
    found
}

/// The module `name` in `dirs`.
fn find_module(dirs: &[PathBuf], name: &str) -> Option<std::sync::Arc<crate::index::ModuleIndex>> {
    dirs.iter()
        .map(|dir| dir.join(name))
        .find(|path| MANIFEST_FILE_NAMES.iter().any(|m| path.join(m).is_file()))
        .and_then(|path| module_index(&path))
}

/// Every module in `dirs`, for definitions outside the closure.
fn all_modules(dirs: &[PathBuf]) -> impl Iterator<Item = std::sync::Arc<crate::index::ModuleIndex>> + '_ {
    dirs.iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten())
        .filter(|entry| MANIFEST_FILE_NAMES.iter().any(|m| entry.path().join(m).is_file()))
        .filter_map(|entry| module_index(&entry.path()))
}

fn at((path, line): (PathBuf, usize)) -> Definition {
    Definition {
        path,
        line: line.max(1),
    }
}

fn resolve(closure: Option<&Closure>, dirs: &[PathBuf], target: Target) -> Option<Definition> {
    match target {
        Target::XmlId(id) => {
            if let Some(found) = closure.and_then(|c| c.xml_id_location(&id)) {
                return Some(at(found));
            }
            let module = id.split_once('.').map(|(m, _)| m)?;
            find_module(dirs, module)
                .and_then(|m| m.xml_id_lines.get(&id).cloned())
                .map(at)
        }
        Target::Model(model) => {
            if let Some(found) = closure.and_then(|c| c.model_location(&model)) {
                return Some(at(found));
            }
            all_modules(dirs)
                .find_map(|m| {
                    m.classes
                        .iter()
                        .find(|c| c.name.as_deref() == Some(model.as_str()))
                        .map(|c| (c.file.clone(), c.line))
                })
                .map(at)
        }
        Target::Field { models, path, segment } => {
            let closure = closure?;
            let segments: Vec<&str> = path.split('.').collect();
            let wanted = segments.get(segment)?;
            models.iter().find_map(|model| {
                let mut current = model.clone();
                for step in &segments[..segment] {
                    current = closure.fields(&current)?.get(*step)?.comodel.clone()?;
                }
                closure.field_location(&current, wanted).map(at)
            })
        }
        Target::ViewField { model, via, name } => {
            let closure = closure?;
            let mut current = model;
            for step in &via {
                current = closure.fields(&current)?.get(step)?.comodel.clone()?;
            }
            closure.field_location(&current, &name).map(at)
        }
    }
}

/// The definition of what is at byte `offset` of `text`, the content of
/// `file` in `module`.
pub fn definition(
    module: &ModuleInfo,
    addons_path: &[PathBuf],
    file: &Path,
    text: &str,
    offset: usize,
) -> Option<Definition> {
    let extension = file.extension()?.to_str()?;
    let target = match extension {
        "xml" => xml_target(text, offset, &module.name)?,
        "py" => python_target(text, offset, &module.name)?,
        _ => return None,
    };
    let closure = closure(module, addons_path);
    let mut dirs: Vec<PathBuf> = module.path.parent().map(PathBuf::from).into_iter().collect();
    dirs.extend(addons_path.iter().cloned());
    resolve(closure.as_ref(), &dirs, target)
}
