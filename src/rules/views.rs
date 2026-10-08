//! Checks of views against the models of the addons path: fields that do
//! not exist on the model a view shows. Silent without the dependencies of
//! a module: see `addons-path` in the configuration.

use crate::index::{closure, modules_defining, Closure};
use crate::rules::upgrade::view_archs;
use crate::rules::xml::{at, child_field, is, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use crate::xml::XmlFile;
use regex::Regex;
use roxmltree::Node;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::LazyLock;

pub const VIEW_FIELD_NOT_FOUND: Rule = Rule {
    code: "ODOO004",
    name: "view-field-not-found",
    summary: "A view uses a field that does not exist on its model.",
    doc: r#"
## What it does

Reports `<field name="...">` elements in views whose field the model does
not have, as the modules the module's `depends` reach define it. Fields of
an embedded list or form are checked against that field's comodel.

In a view that extends another, a field inserted where the extension cannot
tell which sub-view it lands in is checked against the model and the models
of its x2many fields.

## Why is this bad?

Odoo refuses the view: the module fails to install or update ("Field `x`
does not exist in model `y`"). Often the field is defined in a module that
is not in `depends`, so it works on one database and fails on another.

## Configuration

The check needs the code of the dependencies, Odoo's included: list the
folders in `addons-path`. A module whose dependencies cannot all be found is
not checked, so the check never guesses.

```toml
[tool.odoo-lint]
addons-path = ["../odoo/odoo/addons", "../odoo/addons", "../oca/*"]
```
"#,
    check: Check::Xml(check_view_fields),
    min_odoo: None,
    max_odoo: None,
};

/// Elements of a field that are a view of its comodel.
const SUBVIEWS: &[&str] = &[
    "tree", "list", "form", "kanban", "graph", "pivot", "calendar", "search", "gantt",
];

/// Fields Odoo adds to the views of `res.users` at runtime.
fn runtime_field(name: &str) -> bool {
    name.starts_with("in_group_") || name.starts_with("sel_groups_") || name.starts_with("x_")
}

/// Where a field of a view must exist.
#[derive(Clone)]
enum Scope {
    /// On this model.
    Model(String),
    /// On this model or on the comodel of one of its x2many fields (two
    /// levels deep): an extension of a view whose sub-views are unknown.
    Loose(String),
}

struct Checker<'a, 'f> {
    closure: &'a Closure,
    /// Where the module and its dependencies are, to name the module that
    /// has a missing field.
    dirs: &'a [PathBuf],
    file: &'a XmlFile<'f>,
    reporter: &'a mut XmlReporter,
}

impl Checker<'_, '_> {
    /// `; it is defined in `x`, which `depends` does not reach` when
    /// another module of the addons path has the field.
    fn hint(&self, model: &str, field: &str) -> String {
        let modules = modules_defining(self.dirs, model, field);
        if modules.is_empty() {
            return String::new();
        }
        format!(
            "; it is defined in `{}`, which `depends` does not reach",
            modules.join("`, `")
        )
    }

    /// The comodel of `field` on `model`, if it is relational and known.
    fn comodel(&self, model: &str, field: &str) -> Option<String> {
        self.closure.fields(model)?.get(field)?.comodel.clone()
    }

    /// The models a `Loose` scope accepts fields from.
    fn loose_models(&self, model: &str) -> Vec<String> {
        let mut models = vec![model.to_string()];
        let mut seen: HashSet<String> = HashSet::from([model.to_string()]);
        let mut level = vec![model.to_string()];
        for _ in 0..2 {
            let mut next = Vec::new();
            for current in &level {
                let Some(fields) = self.closure.fields(current) else {
                    continue;
                };
                for field in fields.values().filter(|f| f.is_x2many()) {
                    if let Some(comodel) = &field.comodel {
                        if seen.insert(comodel.clone()) {
                            models.push(comodel.clone());
                            next.push(comodel.clone());
                        }
                    }
                }
            }
            level = next;
        }
        models
    }

    /// Checks one `<field>` in `scope`; returns the model of its sub-views,
    /// when known.
    fn field(&mut self, node: Node, scope: &Scope) -> Option<String> {
        let name = node.attribute("name")?;
        if runtime_field(name) || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        match scope {
            Scope::Model(model) => {
                let fields = self.closure.fields(model)?;
                if !fields.contains_key(name) {
                    self.reporter.report(
                        &VIEW_FIELD_NOT_FOUND,
                        self.file,
                        at(self.file, node),
                        format!("Field `{name}` does not exist on `{model}`{}", self.hint(model, name)),
                    );
                    return None;
                }
                self.comodel(model, name)
            }
            Scope::Loose(model) => {
                let models = self.loose_models(model);
                // Every model must be known, or the field may be on one we
                // know nothing about.
                if models.iter().any(|m| self.closure.fields(m).is_none()) {
                    return None;
                }
                let owner = models
                    .iter()
                    .find(|m| self.closure.fields(m).is_some_and(|f| f.contains_key(name)));
                match owner {
                    Some(owner) => self.comodel(owner, name),
                    None => {
                        self.reporter.report(
                            &VIEW_FIELD_NOT_FOUND,
                            self.file,
                            at(self.file, node),
                            format!(
                                "Field `{name}` does not exist on `{model}` or the models of its x2many fields{}",
                                self.hint(model, name)
                            ),
                        );
                        None
                    }
                }
            }
        }
    }

    /// Checks the fields under `node` (excluded) in `scope`.
    fn children(&mut self, node: Node, scope: &Scope) {
        for child in node.children().filter(Node::is_element) {
            self.element(child, scope);
        }
    }

    fn element(&mut self, node: Node, scope: &Scope) {
        if is(node, "field") || is(node, "groupby") {
            let comodel = self.field(node, scope);
            for child in node.children().filter(Node::is_element) {
                if SUBVIEWS.iter().any(|t| is(child, t)) || is(node, "groupby") {
                    // A sub-view (or a `groupby`'s content) shows the comodel.
                    if let Some(comodel) = &comodel {
                        if is(node, "groupby") {
                            self.element(child, &Scope::Model(comodel.clone()));
                        } else {
                            self.children(child, &Scope::Model(comodel.clone()));
                        }
                    }
                } else {
                    self.element(child, scope);
                }
            }
            return;
        }
        self.children(node, scope);
    }

    /// The scope of what an extension spec (`xpath` or an element with
    /// `position`) inserts.
    fn spec_scope(&self, spec: Node, model: &str) -> Scope {
        static SUBVIEW_PATH: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r#"field\[@name=['"](\w+)['"]\]/(?:tree|list|form|kanban)\b"#).unwrap());
        let Some(expr) = spec.attribute("expr").filter(|_| is(spec, "xpath")) else {
            return Scope::Loose(model.to_string());
        };
        let mut current = model.to_string();
        let mut descended = false;
        for captures in SUBVIEW_PATH.captures_iter(expr) {
            match self.comodel(&current, &captures[1]) {
                Some(comodel) => {
                    current = comodel;
                    descended = true;
                }
                None => return Scope::Loose(model.to_string()),
            }
        }
        if descended {
            Scope::Model(current)
        } else {
            Scope::Loose(model.to_string())
        }
    }

    /// An extension's specs: `<xpath>`s and elements with `position`, at
    /// the top of the arch or in a `<data>`.
    fn extension(&mut self, root: Node, model: &str) {
        for spec in root.children().filter(Node::is_element) {
            if is(spec, "data") {
                self.extension(spec, model);
                continue;
            }
            if matches!(spec.attribute("position"), Some("attributes" | "move")) {
                continue;
            }
            let scope = self.spec_scope(spec, model);
            self.children(spec, &scope);
        }
    }
}

fn check_view_fields(ctx: &XmlContext, reporter: &mut XmlReporter) {
    if ctx.settings.addons_path.is_empty() && !has_local_base(ctx) {
        return;
    }
    let Some(closure) = closure(ctx.module, &ctx.settings.addons_path) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = ctx.module.path.parent().map(PathBuf::from).into_iter().collect();
    dirs.extend(ctx.settings.addons_path.iter().cloned());
    for file in ctx.files {
        for (record, arch) in view_archs(file) {
            let Some(model) = child_field(record, "model").and_then(|m| m.text()).map(str::trim) else {
                continue;
            };
            if closure.fields(model).is_none() {
                continue;
            }
            let extension = child_field(record, "inherit_id").is_some();
            let mut checker = Checker {
                closure: &closure,
                dirs: &dirs,
                file,
                reporter,
            };
            for root in arch.children().filter(Node::is_element) {
                if extension {
                    if root.attribute("position").is_some() || is(root, "xpath") {
                        // The arch is a single spec.
                        let scope = checker.spec_scope(root, model);
                        checker.children(root, &scope);
                    } else {
                        checker.extension(root, model);
                    }
                } else {
                    checker.element(root, &Scope::Model(model.to_string()));
                }
            }
        }
    }
}

/// Whether `base` is next to the module (an Odoo source tree's
/// `odoo/addons`), so the check can run without `addons-path`.
fn has_local_base(ctx: &XmlContext) -> bool {
    ctx.module
        .path
        .parent()
        .is_some_and(|dir| dir.join("base").join("__manifest__.py").is_file())
}
