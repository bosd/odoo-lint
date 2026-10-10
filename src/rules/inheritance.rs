//! ODOO008: view inheritance specs (`<xpath>`, `<field position=...>`) that
//! find nothing in the view they inherit, as the modules of `depends` build
//! it. Odoo refuses such a view: "Element ... cannot be located in parent
//! view". Silent without the dependencies of a module (see `addons-path`).

use crate::index::{closure, module_index, Closure, View};
use crate::rules::module::{ModuleContext, ModuleReporter};
use crate::rules::{Check, Rule};
use crate::xpath::{apply_spec, specs, SpecError, Tree};
use std::collections::HashMap;

pub const SPEC_NOT_FOUND: Rule = Rule {
    code: "ODOO008",
    name: "view-spec-not-found",
    summary: "An inheritance spec (`<xpath>`, `<field position=...>`) finds nothing in the view it extends.",
    doc: r#"
## What it does

Builds the view a view inherits from as Odoo does: the base view, then the
extensions of the modules `depends` reaches, in Odoo's order (priority,
then load order). It then applies the view's own specs (`<xpath>`, or an
element such as `<field name="..." position="after">`) one by one, and
reports the first one that matches nothing.

## Why is this bad?

Odoo refuses the view: the module fails to install or update with "Element
... cannot be located in parent view". It typically happens after an
upgrade, when the parent view changed, or when the element comes from a
module that is not in `depends`.

## Limits

XPath outside the subset odoo-lint evaluates (`ancestor::` and other axes,
some functions) is not checked, nor anything after it in the same view. Like
[ODOO004](ODOO004.md), the check needs the dependencies, Odoo's included,
in `addons-path`.
"#,
    check: Check::Module(check_specs),
    min_odoo: None,
    max_odoo: None,
};

/// The views of a closure and the extensions of each view, in the order
/// Odoo applies them.
struct Views<'a> {
    by_id: HashMap<String, (&'a View, usize)>,
    children: HashMap<String, Vec<String>>,
    bases: HashMap<String, Option<Tree>>,
}

/// `%(xmlid)d` references with their module: Odoo replaces them by record
/// ids when it loads an arch, so an xpath with `%(stock.action)d` matches a
/// button with `%(action)d` in the module `stock`.
fn qualify_refs(value: &str, module: &str) -> Option<String> {
    if !value.contains("%(") {
        return None;
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("%(") {
        out.push_str(&rest[..start + 2]);
        rest = &rest[start + 2..];
        if let Some(end) = rest.find(')') {
            let id = &rest[..end];
            if !id.contains('.') {
                out.push_str(module);
                out.push('.');
            }
        }
    }
    out.push_str(rest);
    Some(out)
}

/// The arch of a view as a tree: its root element.
fn arch_tree(view: &View) -> Option<Tree> {
    let mut tree = parse_arch(view)?;
    let module = view.id.split_once('.').map_or("", |(m, _)| m);
    let root = tree.root()?;
    for node in std::iter::once(root).chain(tree.descendants(root)) {
        let qualified: Vec<(String, String)> = tree
            .attrs(node)
            .iter()
            .filter_map(|(name, value)| Some((name.clone(), qualify_refs(value, module)?)))
            .collect();
        for (name, value) in qualified {
            tree.set_attr(node, &name, Some(value));
        }
    }
    Some(tree)
}

fn parse_arch(view: &View) -> Option<Tree> {
    let doc = roxmltree::Document::parse(view.arch.as_ref()?).ok()?;
    let root = doc.root_element();
    if !root.has_tag_name("__arch__") {
        return Some(Tree::from_node(root));
    }
    let elements: Vec<roxmltree::Node> = root.children().filter(|c| c.is_element()).collect();
    match elements.as_slice() {
        [single] => Some(Tree::from_node(*single)),
        [] => None,
        // Several specs without `<data>`: Odoo wraps them in one.
        _ => {
            let mut tree = Tree::from_node(root);
            let top = tree.root()?;
            tree.set_tag(top, "data");
            Some(tree)
        }
    }
}

impl<'a> Views<'a> {
    fn new(closure: &'a Closure) -> Self {
        let by_id = closure.views();
        // Per parent: (priority, module rank, file rank, offset, id).
        type Keyed = Vec<(i64, usize, usize, usize, String)>;
        let mut children: HashMap<String, Keyed> = HashMap::new();
        for (id, (view, rank)) in &by_id {
            let Some(parent) = &view.inherit_id else { continue };
            if view.primary || !view.active || view.arch.is_none() {
                continue;
            }
            children.entry(parent.clone()).or_default().push((
                view.priority,
                *rank,
                view.position.rank,
                view.position.offset,
                id.clone(),
            ));
        }
        let children = children
            .into_iter()
            .map(|(parent, mut list)| {
                list.sort();
                (parent, list.into_iter().map(|(.., id)| id).collect())
            })
            .collect();
        Views {
            by_id,
            children,
            bases: HashMap::new(),
        }
    }

    /// The primary view a view's arch starts from.
    fn root_of(&self, id: &str) -> Option<String> {
        let mut current = id.to_string();
        for _ in 0..64 {
            let (view, _) = self.by_id.get(&current)?;
            if view.primary {
                return Some(current);
            }
            current = view.inherit_id.clone()?;
        }
        None
    }

    /// Applies the extensions under `id` (depth first, in order) to `tree`,
    /// stopping before `stop`; whether it stopped. Clears `clean` when an
    /// extension uses XPath outside the subset: what follows may miss what it
    /// adds.
    fn apply_extensions(&self, tree: &mut Tree, id: &str, stop: Option<&str>, depth: usize, clean: &mut bool) -> bool {
        if depth > 64 {
            *clean = false;
            return false;
        }
        for child in self.children.get(id).into_iter().flatten() {
            if Some(child.as_str()) == stop {
                return true;
            }
            if let Some(spec_tree) = self.by_id.get(child).and_then(|(view, _)| arch_tree(view)) {
                for spec in specs(&spec_tree) {
                    // A broken extension is reported on its own module.
                    match apply_spec(tree, &spec_tree, spec) {
                        Ok(()) => {}
                        // Odoo refuses such a view too; what it adds is missing either way.
                        Err(SpecError::NotFound) => break,
                        Err(SpecError::Unsupported) => {
                            *clean = false;
                            break;
                        }
                    }
                }
            }
            if self.apply_extensions(tree, child, stop, depth + 1, clean) {
                return true;
            }
        }
        false
    }

    /// The arch of a primary view before its extensions: its own, or for a
    /// primary view that inherits, its parent's complete arch with its specs.
    fn base(&mut self, id: &str, depth: usize) -> Option<Tree> {
        if let Some(found) = self.bases.get(id) {
            return found.clone();
        }
        let (view, _) = *self.by_id.get(id)?;
        let tree = match &view.inherit_id {
            None => arch_tree(view),
            Some(parent) if depth < 16 => {
                let mut tree = self.complete(parent, depth + 1)?;
                let spec_tree = arch_tree(view)?;
                let ok = specs(&spec_tree)
                    .into_iter()
                    .all(|s| apply_spec(&mut tree, &spec_tree, s).is_ok());
                ok.then_some(tree)
            }
            Some(_) => None,
        };
        self.bases.insert(id.to_string(), tree.clone());
        tree
    }

    /// The complete arch of the primary view `id` belongs to.
    fn complete(&mut self, id: &str, depth: usize) -> Option<Tree> {
        let root = self.root_of(id)?;
        let mut tree = self.base(&root, depth)?;
        let mut clean = true;
        self.apply_extensions(&mut tree, &root, None, 0, &mut clean);
        clean.then_some(tree)
    }

    /// The arch the specs of the extension `id` apply to.
    fn before(&mut self, id: &str) -> Option<Tree> {
        let (view, _) = *self.by_id.get(id)?;
        if view.primary {
            return self.complete(view.inherit_id.as_ref()?, 0);
        }
        let root = self.root_of(view.inherit_id.as_ref()?)?;
        let mut tree = self.base(&root, 0)?;
        let mut clean = true;
        let stopped = self.apply_extensions(&mut tree, &root, Some(id), 0, &mut clean);
        (stopped && clean).then_some(tree)
    }
}

/// The byte offsets of an arch's specs in its XML text, in the order
/// `xpath::specs` returns them.
fn spec_offsets(arch: &str) -> Vec<(usize, String)> {
    let Ok(doc) = roxmltree::Document::parse(arch) else {
        return Vec::new();
    };
    let mut root = doc.root_element();
    let several = root.children().filter(|c| c.is_element()).count() > 1;
    if root.has_tag_name("__arch__") && !several {
        match root.children().find(|c| c.is_element()) {
            Some(first) => root = first,
            None => return Vec::new(),
        }
    }
    let describe = |node: roxmltree::Node| {
        let attrs: Vec<String> = node
            .attributes()
            .filter(|a| a.name() != "position")
            .map(|a| format!("{}=\"{}\"", a.name(), a.value()))
            .collect();
        format!(
            "<{}{}{}>",
            node.tag_name().name(),
            if attrs.is_empty() { "" } else { " " },
            attrs.join(" ")
        )
    };
    if !root.has_tag_name("data") && !root.has_tag_name("__arch__") {
        return vec![(root.range().start, describe(root))];
    }
    let mut out = Vec::new();
    let mut stack: Vec<roxmltree::Node> = root.children().filter(|c| c.is_element()).collect();
    stack.reverse();
    while let Some(node) = stack.pop() {
        if node.has_tag_name("data") {
            let mut nested: Vec<roxmltree::Node> = node.children().filter(|c| c.is_element()).collect();
            nested.reverse();
            stack.extend(nested);
        } else {
            out.push((node.range().start, describe(node)));
        }
    }
    out
}

fn check_specs(ctx: &ModuleContext, reporter: &mut ModuleReporter) {
    let module = ctx.module;
    let local_base = module
        .path
        .parent()
        .is_some_and(|dir| dir.join("base").join("__manifest__.py").is_file());
    if ctx.settings.addons_path.is_empty() && !local_base {
        return;
    }
    let Some(closure) = closure(module, &ctx.settings.addons_path) else {
        return;
    };
    let Some(own) = module_index(&module.path) else { return };
    let mut views = Views::new(&closure);
    for view in own.views.iter().filter(|v| v.inherit_id.is_some()) {
        let parent = view.inherit_id.as_deref().unwrap_or_default();
        // A parent outside the closure is ODOO005's.
        if !views.by_id.contains_key(parent) {
            continue;
        }
        let (Some(arch), Some(spec_tree)) = (view.arch.as_ref(), arch_tree(view)) else {
            continue;
        };
        let Some(mut tree) = views.before(&view.id) else {
            continue;
        };
        let offsets = spec_offsets(arch);
        for (i, spec) in specs(&spec_tree).into_iter().enumerate() {
            match apply_spec(&mut tree, &spec_tree, spec) {
                Ok(()) => {}
                Err(SpecError::Unsupported) => break,
                Err(SpecError::NotFound) => {
                    let Ok(text) = std::fs::read_to_string(&view.file) else {
                        break;
                    };
                    let (offset, described) = offsets.get(i).cloned().unwrap_or((view.arch_prefix, String::new()));
                    let at = view.arch_offset + offset.saturating_sub(view.arch_prefix);
                    let line = text[..at.min(text.len())].matches('\n').count() + 1;
                    reporter.report(
                        &SPEC_NOT_FOUND,
                        &view.file,
                        line,
                        format!(
                            "`{described}` matches nothing in `{parent}`, as the modules `depends` reaches build it"
                        ),
                    );
                    break;
                }
            }
        }
    }
}
