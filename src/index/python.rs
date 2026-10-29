//! Reading models from Python source: model classes, their fields and
//! constants, without running the code.

use super::{Constraint, Field, FieldLabel, ModelClass, Sanitize, Selection, Translate};
use crate::semantic::func_lib;
use ruff_python_ast::{Expr, Stmt, StmtClassDef};
use ruff_text_size::Ranged;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(super) fn string(expr: &Expr) -> Option<String> {
    expr.as_string_literal_expr().map(|s| s.value.to_str().to_string())
}

/// A value a field definition uses, as far as it can be known without
/// running the code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Literal {
    Str(String),
    Bool(bool),
    /// A list.
    Seq(Vec<Literal>),
    /// A tuple: Odoo reflects the values of a list selection only.
    Tuple(Vec<Literal>),
    /// Anything else: a call, a method, a number, ...
    Other,
}

impl Literal {
    fn into_string(self) -> Option<String> {
        match self {
            Self::Str(value) => Some(value),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The items of a list or a tuple.
    fn into_items(self) -> Option<Vec<Literal>> {
        match self {
            Self::Seq(items) | Self::Tuple(items) => Some(items),
            _ => None,
        }
    }

    /// `(value, label)` pairs of a selection; values that are not text are
    /// left out. `None` when it is not a list or a tuple.
    fn into_pairs(self) -> Option<Vec<(String, Option<String>)>> {
        Some(
            self.into_items()?
                .into_iter()
                .filter_map(|item| {
                    let pair = item.into_items()?;
                    let mut pair = pair.into_iter();
                    let value = pair.next()?.into_string()?;
                    Some((value, pair.next().and_then(Self::into_string)))
                })
                .collect(),
        )
    }
}

/// Where a field class takes `string` as a positional argument.
fn string_position(kind: &str) -> usize {
    match kind {
        "Many2one" | "Selection" | "Reference" => 1,
        "One2many" => 2,
        "Many2many" => 4,
        _ => 0,
    }
}

/// What a class body sees of an expression.
pub(super) type Eval<'e> = dyn Fn(&Expr) -> Literal + 'e;

fn field_label(kind: &str, call: &ruff_python_ast::ExprCall, eval: &Eval) -> FieldLabel {
    let keyword = |name: &str| {
        call.arguments
            .keywords
            .iter()
            .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == name))
            .map(|k| eval(&k.value))
    };
    let positional = |at: usize| call.arguments.args.get(at).map(eval);
    let selection = matches!(kind, "Selection" | "Reference")
        .then(|| keyword("selection").or_else(|| positional(0)))
        .flatten()
        // A tuple is set up as it is, like a method.
        .map(|literal| match literal {
            Literal::Tuple(_) => None,
            literal => literal.into_pairs(),
        })
        .map(|pairs| match pairs {
            Some(pairs) => Selection::List(
                pairs
                    .into_iter()
                    .map(|(value, label)| (value, label.unwrap_or_default()))
                    .collect(),
            ),
            None => Selection::Dynamic,
        });
    FieldLabel {
        string: keyword("string")
            .or_else(|| positional(string_position(kind)))
            .and_then(Literal::into_string),
        help: keyword("help").and_then(Literal::into_string),
        related: keyword("related").and_then(Literal::into_string),
        selection,
        selection_add: keyword("selection_add")
            .and_then(Literal::into_pairs)
            .unwrap_or_default(),
        export_string_translation: keyword("export_string_translation").and_then(|l| l.as_bool()),
        translate: call
            .arguments
            .keywords
            .iter()
            .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "translate"))
            .and_then(|k| translate_mode(&k.value)),
        sanitize: keyword("sanitize").and_then(|value| match value {
            Literal::Bool(true) => Some(Sanitize::Yes),
            Literal::Bool(false) => Some(Sanitize::No),
            Literal::Str(mode) if mode == "email_outgoing" => Some(Sanitize::EmailOutgoing),
            Literal::Str(_) => Some(Sanitize::Yes),
            _ => None,
        }),
    }
}

/// The translation mode a `translate=` value names.
fn translate_mode(expr: &Expr) -> Option<Translate> {
    let name = match expr {
        Expr::BooleanLiteral(b) => return Some(if b.value { Translate::Whole } else { Translate::No }),
        Expr::Name(name) => name.id.as_str(),
        Expr::Attribute(attribute) => attribute.attr.as_str(),
        _ => return None,
    };
    match name {
        "xml_translate" => Some(Translate::Xml),
        "html_translate" => Some(Translate::Html),
        _ => None,
    }
}

/// The constraints of a class: `_sql_constraints` entries and
/// `models.Constraint` / `models.UniqueIndex` attributes.
fn class_constraints(class: &StmtClassDef, eval: &Eval) -> Vec<Constraint> {
    let mut found = Vec::new();
    if let Some(entries) = class_value(class, "_sql_constraints").and_then(|e| eval(e).into_items()) {
        for entry in entries {
            let Some(entry) = entry.into_items() else { continue };
            let mut entry = entry.into_iter();
            let Some(name) = entry.next().and_then(Literal::into_string) else {
                continue;
            };
            found.push(Constraint {
                name,
                message: entry.nth(1).and_then(Literal::into_string),
                table_object: false,
            });
        }
    }
    for stmt in &class.body {
        let Stmt::Assign(assign) = stmt else { continue };
        let [Expr::Name(target)] = assign.targets.as_slice() else {
            continue;
        };
        let Some(name) = target.id.strip_prefix('_') else {
            continue;
        };
        let Expr::Call(call) = &*assign.value else { continue };
        let Expr::Attribute(func) = &*call.func else { continue };
        if !matches!(func.attr.as_str(), "Constraint" | "UniqueIndex") {
            continue;
        }
        let message = call
            .arguments
            .keywords
            .iter()
            .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "message"))
            .map(|k| &k.value)
            .or_else(|| call.arguments.args.get(1))
            .map(eval)
            .and_then(Literal::into_string);
        found.push(Constraint {
            name: name.to_string(),
            message,
            table_object: true,
        });
    }
    found
}

pub(super) fn strings(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::StringLiteral(_) => string(expr).into_iter().collect(),
        Expr::List(list) => list.elts.iter().filter_map(string).collect(),
        Expr::Tuple(tuple) => tuple.elts.iter().filter_map(string).collect(),
        _ => Vec::new(),
    }
}

/// The value assigned to `name` in a class body.
fn class_value<'a>(class: &'a StmtClassDef, name: &str) -> Option<&'a Expr> {
    class.body.iter().rev().find_map(|stmt| match stmt {
        Stmt::Assign(assign)
            if assign
                .targets
                .iter()
                .any(|t| t.as_name_expr().is_some_and(|n| n.id.as_str() == name)) =>
        {
            Some(&*assign.value)
        }
        _ => None,
    })
}

/// A string literal, or the name of a string the class body assigns
/// (`_inherits = {_decorator_model: "line_id"}`).
fn class_string(class: &StmtClassDef, expr: &Expr) -> Option<String> {
    string(expr).or_else(|| {
        let name = expr.as_name_expr()?;
        class_value(class, name.id.as_str()).and_then(string)
    })
}

/// The models a class defines or extends: its `_name`, or every model of
/// its `_inherit`.
pub(crate) fn class_models(class: &StmtClassDef) -> Vec<String> {
    if let Some(name) = class_value(class, "_name").and_then(string) {
        return vec![name];
    }
    class_value(class, "_inherit").map(strings).unwrap_or_default()
}

/// The module-level names of a module's Python files, so that a field can
/// use a constant (`selection=STATES`), also one of another file of the
/// module (`from .stock import STATES`, `stock.STATES`).
#[derive(Default)]
pub(super) struct Constants<'a> {
    assigned: HashMap<PathBuf, HashMap<&'a str, &'a Expr>>,
    /// `from .x import NAME [as ALIAS]`: alias to (file, name).
    imported: HashMap<PathBuf, HashMap<&'a str, (PathBuf, &'a str)>>,
    /// `from . import x`: alias to the file of module `x`.
    modules: HashMap<PathBuf, HashMap<&'a str, PathBuf>>,
}

impl<'a> Constants<'a> {
    pub(super) fn add(&mut self, file: &Path, suite: &'a [Stmt]) {
        let mut assigned = HashMap::new();
        let mut imported = HashMap::new();
        let mut modules = HashMap::new();
        for stmt in suite {
            match stmt {
                Stmt::Assign(assign) => {
                    for target in &assign.targets {
                        if let Expr::Name(name) = target {
                            assigned.insert(name.id.as_str(), &*assign.value);
                        }
                    }
                }
                Stmt::ImportFrom(import) if import.level > 0 => {
                    let package = import.module.as_ref().map(|m| m.as_str());
                    for alias in &import.names {
                        let local = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                        let submodule = match package {
                            Some(package) => format!("{package}.{}", alias.name),
                            None => alias.name.to_string(),
                        };
                        if let Some(module_file) =
                            relative_file(file, import.level, Some(&submodule)).filter(|f| f.is_file())
                        {
                            modules.insert(local, module_file);
                        } else if let Some(target) = relative_file(file, import.level, package) {
                            imported.insert(local, (target, alias.name.as_str()));
                        }
                    }
                }
                _ => {}
            }
        }
        self.assigned.insert(file.to_path_buf(), assigned);
        self.imported.insert(file.to_path_buf(), imported);
        self.modules.insert(file.to_path_buf(), modules);
    }

    /// The value of `expr` in `file`, inside `class` when given: literals,
    /// names of the class body, the file or an imported file, `_()` and
    /// lists added together.
    pub(super) fn eval(&self, file: &Path, class: Option<&StmtClassDef>, expr: &Expr, depth: usize) -> Literal {
        if depth > 8 {
            return Literal::Other;
        }
        let deeper = |file: &Path, expr: &Expr| self.eval(file, None, expr, depth + 1);
        match expr {
            Expr::StringLiteral(literal) => Literal::Str(literal.value.to_str().to_string()),
            Expr::BooleanLiteral(literal) => Literal::Bool(literal.value),
            Expr::List(list) => Literal::Seq(list.elts.iter().map(|e| self.eval(file, class, e, depth + 1)).collect()),
            Expr::Tuple(tuple) => Literal::Tuple(
                tuple
                    .elts
                    .iter()
                    .map(|e| self.eval(file, class, e, depth + 1))
                    .collect(),
            ),
            Expr::BinOp(binary) if binary.op == ruff_python_ast::Operator::Add => {
                match (
                    self.eval(file, class, &binary.left, depth + 1),
                    self.eval(file, class, &binary.right, depth + 1),
                ) {
                    (Literal::Seq(mut left), Literal::Seq(right)) => {
                        left.extend(right);
                        Literal::Seq(left)
                    }
                    (Literal::Tuple(mut left), Literal::Tuple(right)) => {
                        left.extend(right);
                        Literal::Tuple(left)
                    }
                    (Literal::Str(left), Literal::Str(right)) => Literal::Str(left + &right),
                    _ => Literal::Other,
                }
            }
            Expr::Call(call) if matches!(&*call.func, Expr::Name(f) if matches!(f.id.as_str(), "_" | "_lt")) => call
                .arguments
                .args
                .first()
                .map_or(Literal::Other, |arg| self.eval(file, class, arg, depth + 1)),
            Expr::Name(name) => {
                let id = name.id.as_str();
                // `_description = __doc__`: the class docstring.
                if let Some(class) = class.filter(|_| id == "__doc__") {
                    return match class.body.first() {
                        Some(Stmt::Expr(doc)) => match &*doc.value {
                            Expr::StringLiteral(literal) => Literal::Str(literal.value.to_str().to_string()),
                            _ => Literal::Other,
                        },
                        _ => Literal::Other,
                    };
                }
                if let Some(value) = class.and_then(|c| class_value(c, id)) {
                    return self.eval(file, class, value, depth + 1);
                }
                if let Some(value) = self.assigned.get(file).and_then(|a| a.get(id)) {
                    return deeper(file, value);
                }
                if let Some((target, original)) = self.imported.get(file).and_then(|i| i.get(id)) {
                    if let Some(value) = self.assigned.get(target).and_then(|a| a.get(original)) {
                        return deeper(target, value);
                    }
                }
                Literal::Other
            }
            Expr::Attribute(attribute) => {
                let Expr::Name(module) = &*attribute.value else {
                    return Literal::Other;
                };
                let Some(target) = self.modules.get(file).and_then(|m| m.get(module.id.as_str())) else {
                    return Literal::Other;
                };
                match self.assigned.get(target).and_then(|a| a.get(attribute.attr.as_str())) {
                    Some(value) => deeper(target, value),
                    None => Literal::Other,
                }
            }
            _ => Literal::Other,
        }
    }
}

/// The file a relative import names: `from ..models.stock import X` in
/// `wizards/a.py` is `models/stock.py` (or `models/stock/__init__.py`).
fn relative_file(file: &Path, level: u32, module: Option<&str>) -> Option<PathBuf> {
    let mut dir = file.parent()?.to_path_buf();
    for _ in 1..level {
        dir = dir.parent()?.to_path_buf();
    }
    let module = module?;
    let base = module.split('.').fold(dir, |path, part| path.join(part));
    let file = base.with_extension("py");
    if file.is_file() {
        Some(file)
    } else {
        Some(base.join("__init__.py"))
    }
}

/// The model class `class` declares, if any; `eval` gives the values its
/// fields use.
pub(super) fn model_class(class: &StmtClassDef, eval: &Eval) -> Option<ModelClass> {
    let name = class_value(class, "_name").and_then(string);
    let inherit = class_value(class, "_inherit").map(strings).unwrap_or_default();
    if name.is_none() && inherit.is_empty() {
        return None;
    }
    let mut delegates: Vec<String> = match class_value(class, "_inherits") {
        Some(Expr::Dict(dict)) => dict
            .items
            .iter()
            .filter_map(|i| i.key.as_ref().and_then(|key| class_string(class, key)))
            .collect(),
        _ => Vec::new(),
    };
    // `name = fields.X(...)` and `name: Type = fields.X(...)`.
    let fields = class
        .body
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Assign(assign) => match assign.targets.as_slice() {
                [Expr::Name(target)] => Some((target, &*assign.value)),
                _ => None,
            },
            Stmt::AnnAssign(assign) => match (&*assign.target, assign.value.as_deref()) {
                (Expr::Name(target), Some(value)) => Some((target, value)),
                _ => None,
            },
            _ => None,
        })
        .filter_map(|(target, value)| {
            let Expr::Call(call) = value else { return None };
            let kind = match &*call.func {
                Expr::Attribute(func) if func_lib(&call.func) == "fields" => func.attr.to_string(),
                // A module's own field classes: `fs_fields.FsFolder()`.
                Expr::Attribute(func)
                    if func_lib(&call.func).ends_with("fields")
                        && func.attr.starts_with(|c: char| c.is_ascii_uppercase()) =>
                {
                    func.attr.to_string()
                }
                // A field class of a module (`JobSerialized(...)` of
                // queue_job), imported by name; constants aside.
                Expr::Name(func)
                    if func.id.starts_with(|c: char| c.is_ascii_uppercase())
                        && !target.id.starts_with('_')
                        && target.id.chars().any(|c| c.is_ascii_lowercase()) =>
                {
                    func.id.to_string()
                }
                _ => return None,
            };
            let comodel = call
                .arguments
                .keywords
                .iter()
                .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "comodel_name"))
                .map(|k| &k.value)
                .or_else(|| call.arguments.args.first())
                .filter(|_| matches!(kind.as_str(), "Many2one" | "One2many" | "Many2many"))
                .and_then(string);
            let has_domain = call
                .arguments
                .keywords
                .iter()
                .any(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "domain"));
            let groups = call
                .arguments
                .keywords
                .iter()
                .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "groups"))
                .and_then(|k| string(&k.value));
            let label = field_label(&kind, call, eval);
            Some(Field {
                name: target.id.to_string(),
                kind,
                comodel,
                has_domain,
                groups,
                // A byte offset until `parse` turns it into a line.
                line: target.start().to_usize(),
                label,
            })
        })
        .collect::<Vec<Field>>();
    // A name assigned twice is the last assignment, as in Python.
    let mut seen = HashSet::new();
    let mut fields: Vec<Field> = fields
        .into_iter()
        .rev()
        .filter(|f| seen.insert(f.name.clone()))
        .collect();
    fields.reverse();
    // `fields.Many2one(..., delegate=True)` is an `_inherits` too.
    for stmt in &class.body {
        let value = match stmt {
            Stmt::Assign(assign) => Some(&*assign.value),
            Stmt::AnnAssign(assign) => assign.value.as_deref(),
            _ => None,
        };
        let Some(Expr::Call(call)) = value else { continue };
        let delegate = call.arguments.keywords.iter().any(|k| {
            k.arg.as_ref().is_some_and(|a| a.as_str() == "delegate")
                && matches!(&k.value, Expr::BooleanLiteral(b) if b.value)
        });
        let comodel = call
            .arguments
            .keywords
            .iter()
            .find(|k| k.arg.as_ref().is_some_and(|a| a.as_str() == "comodel_name"))
            .map(|k| &k.value)
            .or_else(|| call.arguments.args.first());
        if let Some(comodel) = comodel.and_then(string).filter(|_| delegate) {
            delegates.push(comodel);
        }
    }
    let methods = class
        .body
        .iter()
        .flat_map(|stmt| match stmt {
            Stmt::FunctionDef(function) => vec![function.name.to_string()],
            Stmt::Assign(assign) => assign
                .targets
                .iter()
                .filter_map(|t| t.as_name_expr().map(|n| n.id.to_string()))
                .collect(),
            _ => Vec::new(),
        })
        .collect();
    // Byte offsets until `parse` turns them into lines.
    let method_lines = class
        .body
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::FunctionDef(function) => Some((function.name.to_string(), function.name.start().to_usize())),
            _ => None,
        })
        .collect();
    let foreign_bases = class.arguments.as_ref().is_some_and(|a| {
        a.args.iter().any(|b| {
            !crate::semantic::dotted_name(b)
                .is_some_and(|d| d.ends_with("Model") || d.ends_with("AbstractModel") || d.ends_with("TransientModel"))
        })
    });
    let abstract_model = class.arguments.as_ref().is_some_and(|a| {
        a.args
            .iter()
            .any(|b| crate::semantic::dotted_name(b).is_some_and(|d| d.ends_with("AbstractModel")))
    });
    let odoo_model = class.arguments.as_ref().is_some_and(|a| {
        a.args.iter().any(|b| {
            crate::semantic::dotted_name(b)
                .is_some_and(|d| d.ends_with("Model") || d.ends_with("AbstractModel") || d.ends_with("TransientModel"))
        })
    });
    let python_bases = class
        .arguments
        .as_ref()
        .map(|a| {
            a.args
                .iter()
                .filter_map(crate::semantic::dotted_name)
                .filter(|d| !(d.ends_with("Model") || d.ends_with("AbstractModel") || d.ends_with("TransientModel")))
                .map(|d| d.rsplit('.').next().unwrap_or(&d).to_string())
                .collect()
        })
        .unwrap_or_default();
    Some(ModelClass {
        odoo_model,
        python_bases,
        abstract_model,
        file: PathBuf::new(),
        line: 0,
        name,
        inherit,
        delegates,
        fields,
        methods,
        method_lines,
        foreign_bases,
        description: class_value(class, "_description")
            .map(eval)
            .and_then(Literal::into_string),
        table: class_value(class, "_table").and_then(string),
        auto: class_value(class, "_auto").and_then(|e| eval(e).as_bool()),
        log_access: class_value(class, "_log_access").and_then(|e| eval(e).as_bool()),
        constraints: class_constraints(class, eval),
    })
}
