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
pub(crate) enum Target {
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
    /// A method `name` of the records reached from one of `models` through
    /// the fields `path` (dotted, empty for the model itself).
    Method {
        models: Vec<String>,
        path: String,
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
/// text) at `offset`, and where its name starts.
fn item_at(value: &str, start: usize, offset: usize) -> Option<(&str, usize)> {
    let mut position = start;
    for item in value.split(',') {
        if (position..=position + item.len()).contains(&offset) {
            let name = item.trim_start().trim_start_matches('!').trim_start();
            return Some((name.trim_end(), position + item.len() - name.len()));
        }
        position += item.len() + 1;
    }
    None
}

/// The capture 1 of `pattern` in `value` (at `start`) that holds `offset`,
/// and where it starts.
fn capture_at(pattern: &Regex, value: &str, start: usize, offset: usize) -> Option<(String, usize)> {
    pattern.captures_iter(value).find_map(|c| {
        let m = c.get(1)?;
        (start + m.start() <= offset && offset <= start + m.end()).then(|| (m.as_str().to_string(), start + m.start()))
    })
}

/// What is at `offset` of an XML file, and where the name there starts.
pub(crate) fn xml_target(text: &str, offset: usize, module: &str) -> Option<(Target, usize)> {
    let doc = roxmltree::Document::parse(text).ok()?;
    xml_target_in(&doc, offset, module)
}

/// [`xml_target`] in a parsed document.
pub(crate) fn xml_target_in(doc: &roxmltree::Document, offset: usize, module: &str) -> Option<(Target, usize)> {
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
                let raw = node.text()?;
                let start = range.start + raw.len() - raw.trim_start().len();
                return Some((Target::Model(raw.trim().to_string()), start));
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
            if let Some((id, start)) = capture_at(&ACTION_REF, value, range.start, offset) {
                return Some((Target::XmlId(qualify(module, &id)), start));
            }
            let start = range.start + value.len() - value.trim_start().len();
            let target = match attribute.name() {
                "ref" | "inherit_id" => Target::XmlId(qualify(module, value.trim())),
                "parent" | "action" if tag == "menuitem" => Target::XmlId(qualify(module, value.trim())),
                "groups" => {
                    let (group, start) = item_at(value, range.start, offset)?;
                    return Some((Target::XmlId(qualify(module, group)), start));
                }
                "t-call" => Target::XmlId(qualify(module, value.trim())),
                "eval" => {
                    let (id, start) = capture_at(&EVAL_REF, value, range.start, offset)?;
                    return Some((Target::XmlId(qualify(module, &id)), start));
                }
                "model" if matches!(tag, "record" | "function" | "report" | "delete") => {
                    Target::Model(value.trim().to_string())
                }
                "name" if tag == "field" => view_field(node, value)?,
                // `<button type="object" name="action_confirm">`
                "name" if tag == "button" && node.attribute("type") == Some("object") => {
                    match view_field(node, value)? {
                        Target::ViewField { model, via, name } => Target::Method {
                            models: vec![model],
                            path: via.join("."),
                            name,
                        },
                        _ => return None,
                    }
                }
                _ => return None,
            };
            return Some((target, start));
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

/// What is at `offset` of a Python file, and where the name there starts.
pub(crate) fn python_target(text: &str, offset: usize, module: &str) -> Option<(Target, usize)> {
    let parsed = ruff_python_parser::parse_module(text).ok()?;
    python_target_in(&parsed, text, offset, module)
}

/// [`python_target`] in a parsed module.
pub(crate) fn python_target_in(
    parsed: &ruff_python_parser::Parsed<ruff_python_ast::ModModule>,
    text: &str,
    offset: usize,
    module: &str,
) -> Option<(Target, usize)> {
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
    // Where the segment of a field path at the cursor starts.
    let segment_start = |expr: &Expr| {
        let start = content_start(expr);
        start + text[start..offset.max(start)].rfind('.').map_or(0, |i| i + 1)
    };
    let receivers = crate::receiver::Receivers::new(parsed.suite());
    let mut found = None;
    walk(parsed.suite(), |node, scopes| {
        if found.is_some() {
            return;
        }
        let class_models = || enclosing_class(scopes).map(class_models).unwrap_or_default();
        match node {
            // `order.action_confirm()`
            Node::Expr(Expr::Call(call))
                if matches!(&*call.func, Expr::Attribute(a)
                    if a.attr.range().start().to_usize() <= offset && offset <= a.attr.range().end().to_usize()) =>
            {
                let Expr::Attribute(attribute) = &*call.func else {
                    return;
                };
                found = receivers.records(&attribute.value, scopes).map(|records| {
                    (
                        Target::Method {
                            models: records.models,
                            path: records.path.join("."),
                            name: attribute.attr.to_string(),
                        },
                        attribute.attr.range().start().to_usize(),
                    )
                });
            }
            Node::Expr(Expr::Call(call)) => {
                let name = func_name(&call.func);
                for (i, arg) in call.arguments.args.iter().enumerate() {
                    if !holds(arg) {
                        continue;
                    }
                    let value = string(arg).unwrap_or_default();
                    found = match name {
                        "ref" => Some((Target::XmlId(qualify(module, &value)), content_start(arg))),
                        "has_group" | "has_groups" | "user_has_groups" => item_at(&value, content_start(arg), offset)
                            .map(|(g, start)| (Target::XmlId(qualify(module, g)), start)),
                        "depends" | "onchange" | "constrains" | "mapped" | "filtered" | "sorted" => Some((
                            Target::Field {
                                models: class_models(),
                                path: value,
                                segment: segment(arg),
                            },
                            segment_start(arg),
                        )),
                        "Many2one" | "One2many" | "Many2many" if func_lib(&call.func) == "fields" && i == 0 => {
                            Some((Target::Model(value), content_start(arg)))
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
                        Some("comodel_name") => Some((Target::Model(value), content_start(&keyword.value))),
                        // `compute="_compute_total"` and the other methods a field names.
                        Some("compute" | "inverse" | "search" | "selection" | "group_expand")
                            if func_lib(&call.func) == "fields" =>
                        {
                            Some((
                                Target::Method {
                                    models: class_models(),
                                    path: String::new(),
                                    name: value,
                                },
                                content_start(&keyword.value),
                            ))
                        }
                        Some("related") => Some((
                            Target::Field {
                                models: class_models(),
                                path: value,
                                segment: segment(&keyword.value),
                            },
                            segment_start(&keyword.value),
                        )),
                        _ => None,
                    };
                }
            }
            // `order.partner_id`, with the model of `order` as the code shows it.
            Node::Expr(Expr::Attribute(attribute))
                if attribute.attr.range().start().to_usize() <= offset
                    && offset <= attribute.attr.range().end().to_usize() =>
            {
                found = receivers.records(&attribute.value, scopes).map(|records| {
                    let segment = records.path.len();
                    let mut path = records.path;
                    path.push(attribute.attr.to_string());
                    (
                        Target::Field {
                            models: records.models,
                            path: path.join("."),
                            segment,
                        },
                        attribute.attr.range().start().to_usize(),
                    )
                });
            }
            // `self.env["sale.order"]`
            Node::Expr(Expr::Subscript(subscript)) if holds(&subscript.slice) => {
                if matches!(&*subscript.value, Expr::Attribute(a) if a.attr.as_str() == "env")
                    || matches!(&*subscript.value, Expr::Name(n) if n.id.as_str() == "env")
                {
                    found = string(&subscript.slice).map(|m| (Target::Model(m), content_start(&subscript.slice)));
                }
            }
            // A method's definition, or an override of it.
            Node::Stmt(Stmt::FunctionDef(function))
                if function.name.range().start().to_usize() <= offset
                    && offset <= function.name.range().end().to_usize()
                    && matches!(scopes.last(), Some(crate::visit::Scope::Class(_))) =>
            {
                let models = class_models();
                if !models.is_empty() {
                    found = Some((
                        Target::Method {
                            models,
                            path: String::new(),
                            name: function.name.to_string(),
                        },
                        function.name.range().start().to_usize(),
                    ));
                }
            }
            // A field's own definition: `partner_id = fields.Many2one(...)`.
            Node::Stmt(Stmt::Assign(assign))
                if matches!(assign.targets.as_slice(), [Expr::Name(n)]
                    if n.range().start().to_usize() <= offset && offset <= n.range().end().to_usize())
                    && matches!(&*assign.value, Expr::Call(call) if func_lib(&call.func) == "fields")
                    && matches!(scopes.last(), Some(crate::visit::Scope::Class(_))) =>
            {
                let name = &assign.targets[0];
                found = Some((
                    Target::Field {
                        models: class_models(),
                        path: name.as_name_expr().map(|n| n.id.to_string()).unwrap_or_default(),
                        segment: 0,
                    },
                    name.range().start().to_usize(),
                ));
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
                found = items
                    .into_iter()
                    .find(|e| holds(e))
                    .and_then(|e| Some((Target::Model(string(e)?), content_start(e))));
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

fn at((path, line): (PathBuf, usize)) -> Definition {
    Definition {
        path,
        line: line.max(1),
    }
}

pub(crate) fn resolve(closure: Option<&Closure>, dirs: &[PathBuf], target: Target) -> Option<Definition> {
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
            crate::index::modules_mentioning(dirs, &[model.as_str()])
                .iter()
                .find_map(|m| {
                    m.classes
                        .iter()
                        .find(|c| c.name.as_deref() == Some(model.as_str()) && !c.inherit.contains(&model))
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
        Target::Method { models, path, name } => {
            let closure = closure?;
            models.iter().find_map(|model| {
                let mut current = model.clone();
                for step in path.split('.').filter(|s| !s.is_empty()) {
                    current = closure.fields(&current)?.get(step)?.comodel.clone()?;
                }
                closure.method_location(&current, &name).map(at)
            })
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
        "xml" => xml_target(text, offset, &module.name)?.0,
        "py" => python_target(text, offset, &module.name)?.0,
        _ => return None,
    };
    let closure = closure(module, addons_path);
    let mut dirs: Vec<PathBuf> = module.path.parent().map(PathBuf::from).into_iter().collect();
    dirs.extend(addons_path.iter().cloned());
    resolve(closure.as_ref(), &dirs, target)
}
