//! What a module's assets register for the web client: the widgets of its
//! JavaScript (`registry.category("fields").add("name", ...)`, also through
//! a variable holding the category) and the names of its OWL templates
//! (`<t t-name="...">` in `static/src`). Read once per module and run.

use oxc_ast::ast::{Argument, BindingPattern, CallExpression, Expression, VariableDeclarator};
use oxc_ast_visit::{walk, Visit};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

/// The registrations and templates of one module.
#[derive(Debug, Default)]
pub struct Assets {
    /// Keys of the `fields` registry (`many2many_tags`, `list.many2many_tags`).
    pub field_widgets: HashSet<String>,
    /// Keys of the `view_widgets` registry (`<widget name="...">`).
    pub view_widgets: HashSet<String>,
    /// OWL template names (`t-name`).
    pub templates: HashSet<String>,
    /// The templates components render: `static template = "..."`.
    pub component_templates: HashSet<String>,
    /// The variables each template defines (`t-set`, `t-as`, ...), also
    /// those its extensions add, by template name.
    pub template_locals: HashMap<String, HashSet<String>>,
    /// The template each one inherits from (`t-inherit`), by name.
    pub parents: HashMap<String, String>,
    /// The templates `t-call` names: they get variables from their caller.
    pub called_templates: HashSet<String>,
}

/// The variables a template defines: `t-set`, `t-as` (and its `_index`,
/// `_value`, `_first`, `_last`, `_size`), `t-slot-scope`, anywhere in it.
pub fn template_locals(template: roxmltree::Node) -> HashSet<String> {
    let mut locals = HashSet::new();
    for node in template.descendants().filter(roxmltree::Node::is_element) {
        if let Some(name) = node.attribute("t-set") {
            locals.insert(name.trim().to_string());
        }
        if let Some(name) = node.attribute("t-as") {
            let name = name.trim();
            for suffix in ["", "_index", "_value", "_first", "_last", "_size"] {
                locals.insert(format!("{name}{suffix}"));
            }
        }
        if let Some(name) = node.attribute("t-slot-scope") {
            locals.insert(name.trim().to_string());
        }
    }
    locals
}

impl Assets {
    pub fn is_empty(&self) -> bool {
        self.field_widgets.is_empty() && self.view_widgets.is_empty() && self.templates.is_empty()
    }
}

/// Whether `path` is inside a library the module ships as it is.
fn is_library(path: &Path) -> bool {
    let parts: Vec<&str> = path.iter().filter_map(|p| p.to_str()).collect();
    parts.windows(2).any(|w| w == ["static", "lib"] || w == ["src", "lib"])
}

fn string<'a>(argument: &'a Argument) -> Option<&'a str> {
    match argument.as_expression()? {
        Expression::StringLiteral(literal) => Some(literal.value.as_str()),
        _ => None,
    }
}

/// The registry category a call makes: `x.category("fields")`.
fn category<'a>(expression: &'a Expression) -> Option<&'a str> {
    let Expression::CallExpression(call) = expression else {
        return None;
    };
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    (member.property.name == "category")
        .then(|| call.arguments.first().and_then(string))
        .flatten()
}

#[derive(Default)]
struct Registrations {
    /// Variables holding a category: `const fieldRegistry = registry.category("fields")`.
    variables: HashMap<String, String>,
    /// (category, key) of every `.add(key, ...)`.
    added: Vec<(String, String)>,
    /// `static template = "..."` of the classes.
    component_templates: Vec<String>,
}

impl Registrations {
    /// The category `.add(...)` is called on: a category, a variable holding
    /// one, or an earlier `.add(...)` of a chain.
    fn category_of(&self, expression: &Expression) -> Option<String> {
        if let Some(name) = category(expression) {
            return Some(name.to_string());
        }
        match expression {
            Expression::Identifier(id) => self.variables.get(id.name.as_str()).cloned(),
            Expression::CallExpression(call) => match &call.callee {
                Expression::StaticMemberExpression(member) if member.property.name == "add" => {
                    self.category_of(&member.object)
                }
                _ => None,
            },
            _ => None,
        }
    }
}

impl<'a> Visit<'a> for Registrations {
    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if let (BindingPattern::BindingIdentifier(id), Some(init)) = (&declarator.id, &declarator.init) {
            if let Some(name) = category(init) {
                self.variables.insert(id.name.to_string(), name.to_string());
            }
        }
        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_property_definition(&mut self, property: &oxc_ast::ast::PropertyDefinition<'a>) {
        if property.r#static && property.key.is_specific_static_name("template") {
            if let Some(Expression::StringLiteral(name)) = &property.value {
                self.component_templates.push(name.value.to_string());
            }
        }
        walk::walk_property_definition(self, property);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Expression::StaticMemberExpression(member) = &call.callee {
            if member.property.name == "add" {
                let category = self.category_of(&member.object);
                if let (Some(category), Some(key)) = (category, call.arguments.first().and_then(string)) {
                    self.added.push((category, key.to_string()));
                }
            }
        }
        walk::walk_call_expression(self, call);
    }
}

/// The files under `dir` with `extension`, without libraries.
fn files(dir: &Path, extension: &str) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| !is_library(e.path()))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == extension))
        .map(|e| e.into_path())
        .collect()
}

fn read(module: &Path) -> Assets {
    let mut assets = Assets::default();
    let src = module.join("static").join("src");
    if !src.is_dir() {
        return assets;
    }
    for path in files(&src, "js") {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Most files register nothing; skip parsing them.
        if !source.contains(".add(") && !source.contains("template") {
            continue;
        }
        let allocator = oxc_allocator::Allocator::default();
        let parsed = oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::mjs()).parse();
        let mut registrations = Registrations::default();
        registrations.visit_program(&parsed.program);
        assets.component_templates.extend(registrations.component_templates);
        for (category, key) in registrations.added {
            match category.as_str() {
                "fields" => assets.field_widgets.insert(key),
                "view_widgets" => assets.view_widgets.insert(key),
                _ => false,
            };
        }
    }
    for path in files(&src, "xml") {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(doc) = roxmltree::Document::parse(&source) else {
            continue;
        };
        // The bundle takes the `t-name` children of any root (`<templates>`,
        // `<template>`), or the root itself when it has one. Every `t-name`
        // is registered, an extension's too.
        let root = doc.root_element();
        for node in root.descendants().filter(roxmltree::Node::is_element) {
            if let Some(called) = node.attribute("t-call").filter(|c| !c.contains(['{', '('])) {
                assets.called_templates.insert(called.trim().to_string());
            }
        }
        let templates = std::iter::once(root).chain(root.children().filter(|t| t.is_element()));
        for template in templates {
            let name = template.attribute("t-name");
            let parent = template.attribute("t-inherit");
            if let Some(name) = name {
                assets.templates.insert(name.to_string());
            }
            if let (Some(name), Some(parent)) = (name, parent) {
                if template.attribute("t-inherit-mode") != Some("extension") {
                    assets.parents.insert(name.to_string(), parent.to_string());
                }
            }
            // An extension's variables belong to the template it extends.
            let owner = match (template.attribute("t-inherit-mode"), parent, name) {
                (Some("extension"), Some(parent), _) => parent,
                (_, _, Some(name)) => name,
                _ => continue,
            };
            assets
                .template_locals
                .entry(owner.to_string())
                .or_default()
                .extend(template_locals(template));
        }
    }
    assets
}

/// The assets of the module at `path`, read once per run.
pub fn module_assets(path: &Path) -> Arc<Assets> {
    static CACHE: LazyLock<Mutex<HashMap<PathBuf, Arc<Assets>>>> = LazyLock::new(Default::default);
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(found) = CACHE.lock().expect("assets cache").get(&key) {
        return found.clone();
    }
    let assets = Arc::new(read(&key));
    CACHE.lock().expect("assets cache").insert(key, assets.clone());
    assets
}

/// Whether the field widget `name` is registered in `assets`: as is, or for
/// one view type (`list.name`).
pub fn has_field_widget<'a>(assets: impl IntoIterator<Item = &'a Assets>, name: &str) -> bool {
    let suffix = format!(".{name}");
    assets
        .into_iter()
        .any(|a| a.field_widgets.contains(name) || a.field_widgets.iter().any(|k| k.ends_with(&suffix)))
}
