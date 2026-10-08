//! Rules on field definitions (`name = fields.X(...)` in a class body) and
//! other class attributes.

use super::{classes, field_definitions, methods, str_value};
use crate::checker::{PythonContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::rules::{Check, Rule};
use crate::semantic::func_name;
use ruff_python_ast::{Expr, ExprCall, Stmt, StmtClassDef};
use ruff_text_size::Ranged;
use std::collections::HashMap;

const DEFAULT_DEPRECATED_ATTRIBUTES: &[&str] = &["_columns", "_defaults", "length"];
const DEFAULT_RENAMED_PARAMETERS: &[(&str, &str)] = &[("digits_compute", "digits"), ("select", "index")];
/// Position of the `string` argument per field type; 0 for the others.
const STRING_POSITION: &[(&str, usize)] = &[
    ("Many2many", 4),
    ("One2many", 2),
    ("Many2one", 1),
    ("Reference", 1),
    ("Selection", 1),
];
const TRANSLATION_METHODS: &[&str] = &["_", "_lt"];

macro_rules! method_name_rule {
    ($const:ident, $check:ident, $code:literal, $name:literal, $kind:literal) => {
        fn $check(ctx: &PythonContext, reporter: &mut Reporter) {
            check_method_names(ctx, reporter, $kind, &$const);
        }

        pub const $const: Rule = Rule {
            code: $code,
            name: $name,
            summary: concat!("A field's `", $kind, "` method is not named `_", $kind, "_...`."),
            doc: concat!(
                "\n## What it does\n\nChecks that the method a field names in `",
                $kind,
                "=` starts with `_",
                $kind,
                "_`.\n\n## Why is this bad?\n\nThe OCA naming convention makes the purpose of a method clear without\nlooking up the field that uses it.\n\n## Example\n\n```python\ntotal = fields.Float(",
                $kind,
                "=\"get_total\")\n```\n\nUse instead:\n\n```python\ntotal = fields.Float(",
                $kind,
                "=\"_",
                $kind,
                "_total\")\n```\n"
            ),
            check: Check::Python($check),
            min_odoo: None,
            max_odoo: None,
        };
    };
}

method_name_rule!(
    METHOD_COMPUTE,
    check_method_compute,
    "C8108",
    "method-compute",
    "compute"
);
method_name_rule!(METHOD_SEARCH, check_method_search, "C8109", "method-search", "search");
method_name_rule!(
    METHOD_INVERSE,
    check_method_inverse,
    "C8110",
    "method-inverse",
    "inverse"
);

pub const TRANSLATION_FIELD: Rule = Rule {
    code: "W8103",
    name: "translation-field",
    summary: "A field label is wrapped in `_()`.",
    doc: r#"
## What it does

Reports `_()` and `_lt()` around arguments of a field definition, such as its
label.

## Why is this bad?

Odoo exports and translates field labels and help texts by itself. The
translation call runs once at import time, in the server language, and adds
nothing.

## Example

```python
name = fields.Char(_("Name"))
```

Use instead:

```python
name = fields.Char("Name")
```
"#,
    check: Check::Python(check_translation_field),
    min_odoo: None,
    max_odoo: None,
};

pub const ATTRIBUTE_DEPRECATED: Rule = Rule {
    code: "W8105",
    name: "attribute-deprecated",
    summary: "A model uses a deprecated class attribute.",
    doc: r#"
## What it does

Reports class attributes of models that the old API used: `_columns`,
`_defaults` and `length`.

## Why is this bad?

They are ignored by the current ORM, so the fields or defaults they define
silently do not exist.

## Configuration

```toml
[tool.odoo-lint.rules.attribute-deprecated]
attributes = ["_columns", "_defaults", "length"]
```
"#,
    check: Check::Python(check_attribute_deprecated),
    min_odoo: None,
    max_odoo: None,
};

pub const RENAMED_FIELD_PARAMETER: Rule = Rule {
    code: "W8111",
    name: "renamed-field-parameter",
    summary: "A field uses a parameter that was renamed.",
    doc: r#"
## What it does

Reports field parameters that Odoo renamed: `digits_compute` (now `digits`)
and `select` (now `index`).

## Why is this bad?

The old names are ignored, so the field silently loses its precision or
index.

## Configuration

```toml
[tool.odoo-lint.rules.renamed-field-parameter]
parameters = { digits_compute = "digits", select = "index" }
```

## Fix safety

`select` to `index` is safe. Other renames, such as `digits_compute` to
`digits`, are unsafe: the new parameter may expect another kind of value.
There is no fix when the new parameter is already set.
"#,
    check: Check::Python(check_renamed_parameters),
    min_odoo: None,
    max_odoo: None,
};

pub const ATTRIBUTE_STRING_REDUNDANT: Rule = Rule {
    code: "W8113",
    name: "attribute-string-redundant",
    summary: "A field label repeats what Odoo derives from the field name.",
    doc: r#"
## What it does

Reports field labels equal to the label Odoo generates from the field name:
the name without `_id`/`_ids`, with spaces and in title case.

## Why is this bad?

The label adds nothing, and has to be kept in sync by hand when the field is
renamed.

## Example

```python
partner_id = fields.Many2one("res.partner", "Partner")
```

Use instead:

```python
partner_id = fields.Many2one("res.partner")
```
"#,
    check: Check::Python(check_string_redundant),
    min_odoo: None,
    max_odoo: None,
};

pub const INHERITABLE_METHOD_STRING: Rule = Rule {
    code: "E8147",
    name: "inheritable-method-string",
    summary: "A field passes a method object instead of its name.",
    doc: r#"
## What it does

Reports `compute=`, `inverse=` and `search=` given a method of the class
instead of the method name as a string.

## Why is this bad?

The field keeps a reference to this exact function, so a module that
overrides the method in an inheriting class is ignored.

## Example

```python
def _compute_total(self): ...

total = fields.Float(compute=_compute_total)
```

Use instead:

```python
total = fields.Float(compute="_compute_total")
```
"#,
    check: Check::Python(check_inheritable_string),
    min_odoo: None,
    max_odoo: None,
};

pub const INHERITABLE_METHOD_LAMBDA: Rule = Rule {
    code: "E8148",
    name: "inheritable-method-lambda",
    summary: "A field passes a method object as `default` or `domain`.",
    doc: r#"
## What it does

Reports `default=` and `domain=` given a method of the class.

## Why is this bad?

The field keeps a reference to this exact function, so overriding the method
in an inheriting class has no effect. A lambda looks the method up at call
time.

## Example

```python
def _default_user(self): ...

user_id = fields.Many2one("res.users", default=_default_user)
```

Use instead:

```python
user_id = fields.Many2one("res.users", default=lambda self: self._default_user())
```
"#,
    check: Check::Python(check_inheritable_lambda),
    min_odoo: None,
    max_odoo: None,
};

/// Keyword arguments of a field call (`None` for `**kwargs`).
fn keywords(call: &ExprCall) -> impl Iterator<Item = (Option<&str>, &Expr)> {
    call.arguments
        .keywords
        .iter()
        .map(|kw| (kw.arg.as_ref().map(|a| a.as_str()), &kw.value))
}

/// Reports `kind=` arguments of field definitions not starting with `_kind_`.
fn check_method_names(ctx: &PythonContext, reporter: &mut Reporter, kind: &str, rule: &Rule) {
    let prefix = format!("_{kind}_");
    for class in classes(ctx.parsed.suite()) {
        for (_, call) in field_definitions(class) {
            for (arg, value) in keywords(call) {
                if arg != Some(kind) {
                    continue;
                }
                if str_value(value).is_some_and(|method| !method.starts_with(&prefix)) {
                    reporter.report(
                        rule,
                        value.start(),
                        format!("Name of {kind} method should start with \"{prefix}\""),
                    );
                }
            }
        }
    }
}

fn check_translation_field(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        for (_, call) in field_definitions(class) {
            let values = call
                .arguments
                .args
                .iter()
                .chain(call.arguments.keywords.iter().map(|kw| &kw.value));
            for value in values {
                if let Expr::Call(inner) = value {
                    if TRANSLATION_METHODS.contains(&func_name(&inner.func)) {
                        reporter.report(
                            &TRANSLATION_FIELD,
                            inner.start(),
                            "Translation method _(\"string\") in fields is not necessary.",
                        );
                    }
                }
            }
        }
    }
}

fn check_attribute_deprecated(ctx: &PythonContext, reporter: &mut Reporter) {
    let deprecated = crate::config::list_or(
        ctx.settings.config.rules().attribute_deprecated.as_ref(),
        |c| c.attributes.as_ref(),
        DEFAULT_DEPRECATED_ATTRIBUTES,
    );
    for class in classes(ctx.parsed.suite()) {
        let bases = class.arguments.as_ref().map(|a| &a.args[..]).unwrap_or(&[]);
        let is_model = bases.iter().any(|b| super::source_of(ctx.source, b).contains("Model"));
        if !is_model {
            continue;
        }
        for stmt in &class.body {
            let Stmt::Assign(assign) = stmt else { continue };
            let Some(Expr::Name(target)) = assign.targets.first() else {
                continue;
            };
            if deprecated.iter().any(|a| a == target.id.as_str()) {
                reporter.report(
                    &ATTRIBUTE_DEPRECATED,
                    target.start(),
                    format!("attribute \"{}\" deprecated", target.id),
                );
            }
        }
    }
}

fn check_renamed_parameters(ctx: &PythonContext, reporter: &mut Reporter) {
    let renamed: HashMap<String, String> = ctx
        .settings
        .config
        .rules()
        .renamed_field_parameter
        .as_ref()
        .and_then(|c| c.parameters.clone())
        .unwrap_or_else(|| {
            DEFAULT_RENAMED_PARAMETERS
                .iter()
                .map(|(old, new)| (old.to_string(), new.to_string()))
                .collect()
        });
    for class in classes(ctx.parsed.suite()) {
        for (_, call) in field_definitions(class) {
            for keyword in &call.arguments.keywords {
                let Some(identifier) = &keyword.arg else { continue };
                let (arg, value) = (identifier.as_str(), &keyword.value);
                // pylint-odoo only looks at renames when the method-name check
                // does not apply to this argument.
                let method_name_issue = ["compute", "search", "inverse"].contains(&arg)
                    && str_value(value).is_some_and(|v| !v.starts_with(&format!("_{arg}_")));
                if method_name_issue {
                    continue;
                }
                if let Some(new) = renamed.get(arg) {
                    let violation = reporter.report(
                        &RENAMED_FIELD_PARAMETER,
                        call.start(),
                        format!("Field parameter \"{arg}\" is no longer supported. Use \"{new}\" instead."),
                    );
                    // Renaming next to an existing `new=` would repeat a keyword.
                    let taken = keywords(call).any(|(other, _)| other == Some(new.as_str()));
                    if !taken {
                        let edits = vec![Edit::replace(
                            identifier.start().to_usize(),
                            identifier.end().to_usize(),
                            new.clone(),
                        )];
                        let title = format!("Rename to `{new}`");
                        // `index` takes the same values as `select`; other
                        // renames can expect other values (`digits_compute`
                        // took a function).
                        violation.fix = Some(if (arg, new.as_str()) == ("select", "index") {
                            Fix::safe(title, edits)
                        } else {
                            Fix::unsafe_(title, edits)
                        });
                    }
                }
            }
        }
    }
}

/// Python's `str.title()`.
fn py_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut previous_cased = false;
    for c in s.chars() {
        if previous_cased {
            out.extend(c.to_lowercase());
        } else {
            out.extend(c.to_uppercase());
        }
        previous_cased = c.is_alphabetic();
    }
    out
}

/// pylint-odoo's `_get_field_arg_string`: the label of a field definition.
fn field_label(call: &ExprCall) -> Option<String> {
    let field_type = func_name(&call.func);
    let position = STRING_POSITION
        .iter()
        .find(|(t, _)| *t == field_type)
        .map_or(0, |(_, p)| *p);
    match call.arguments.args.get(position) {
        Some(arg) => str_value(arg),
        None => keywords(call)
            .find(|(arg, _)| *arg == Some("string"))
            .and_then(|(_, value)| str_value(value)),
    }
}

fn check_string_redundant(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        for (assign, call) in field_definitions(class) {
            let field_name = match assign.targets.first() {
                Some(Expr::Name(name)) => {
                    let name = name.id.as_str();
                    let name = name.strip_suffix("_ids").unwrap_or(name);
                    let name = name.strip_suffix("_id").unwrap_or(name);
                    name.replace('_', " ")
                }
                _ => String::new(),
            };
            let is_related = keywords(call).any(|(arg, _)| arg == Some("related"));
            if !is_related && field_label(call).is_some_and(|label| label == py_title(&field_name)) {
                reporter.report(
                    &ATTRIBUTE_STRING_REDUNDANT,
                    call.start(),
                    "The attribute string is redundant. String parameter equal to name of variable",
                );
            }
        }
    }
}

/// Whether `name` is a method of `class` defined before `offset`.
fn is_method_before(class: &StmtClassDef, name: &str, offset: ruff_text_size::TextSize) -> bool {
    methods(class).any(|m| m.name.as_str() == name && m.start() < offset)
}

fn check_inheritable_string(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        for (_, call) in field_definitions(class) {
            for (arg, value) in keywords(call) {
                if !matches!(arg, Some("compute" | "search" | "inverse")) {
                    continue;
                }
                let Expr::Name(name) = value else { continue };
                if is_method_before(class, name.id.as_str(), call.start()) {
                    reporter.report(
                        &INHERITABLE_METHOD_STRING,
                        name.start(),
                        format!(
                            "Use string method name `\"{}\"` to preserve inheritability. More info at https://github.com/OCA/odoo-pre-commit-hooks/issues/126",
                            name.id
                        ),
                    );
                }
            }
        }
    }
}

fn check_inheritable_lambda(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        for (_, call) in field_definitions(class) {
            for (arg, value) in keywords(call) {
                let Some(arg @ ("default" | "domain")) = arg else {
                    continue;
                };
                let Expr::Name(name) = value else { continue };
                if is_method_before(class, name.id.as_str(), call.start()) {
                    reporter.report(
                        &INHERITABLE_METHOD_LAMBDA,
                        name.start(),
                        format!(
                            "Use `{arg}=lambda self: self.{}()` to preserve inheritability. More info at https://github.com/OCA/odoo-pre-commit-hooks/issues/126",
                            name.id
                        ),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::run_python_rule;

    fn messages(rule: &Rule, src: &str) -> Vec<(usize, String)> {
        run_python_rule(rule, src)
            .into_iter()
            .map(|v| (v.line, v.message))
            .collect()
    }

    const HEADER: &str = "from odoo import fields, models\n\n\nclass A(models.Model):\n    _name = 'a'\n";

    #[test]
    fn method_names() {
        let src = format!(
            "{HEADER}    a = fields.Float(compute='get_a', inverse='_inverse_a', search=f'x{{1}}')\n    b = fields.Float(compute=_compute_b)\n"
        );
        let compute: Vec<_> = run_python_rule(&METHOD_COMPUTE, &src)
            .into_iter()
            .map(|v| v.message)
            .collect();
        assert_eq!(compute, vec!["Name of compute method should start with \"_compute_\""]);
        let search: Vec<_> = run_python_rule(&METHOD_SEARCH, &src)
            .into_iter()
            .map(|v| v.code)
            .collect();
        assert_eq!(search, vec!["C8109"]);
        assert!(run_python_rule(&METHOD_INVERSE, &src).is_empty());
    }

    #[test]
    fn translation_field() {
        let src = format!("{HEADER}    a = fields.Char(_('A'), help=_lt('help'))\n    b = fields.Char('B')\n");
        assert_eq!(messages(&TRANSLATION_FIELD, &src).len(), 2);
    }

    #[test]
    fn attribute_deprecated() {
        let src = format!("{HEADER}    _columns = {{}}\n    _order = 'id'\n");
        assert_eq!(
            messages(&ATTRIBUTE_DEPRECATED, &src),
            vec![(6, "attribute \"_columns\" deprecated".into())]
        );
        assert!(messages(&ATTRIBUTE_DEPRECATED, "class B(object):\n    _columns = {}\n").is_empty());
    }

    #[test]
    fn renamed_parameter() {
        let src = format!("{HEADER}    a = fields.Float(digits_compute=get, select=True)\n");
        let m = messages(&RENAMED_FIELD_PARAMETER, &src);
        assert_eq!(m.len(), 2);
        assert_eq!(
            m[0].1,
            "Field parameter \"digits_compute\" is no longer supported. Use \"digits\" instead."
        );
    }

    #[test]
    fn string_redundant() {
        let src = format!(
            "{HEADER}    partner_id = fields.Many2one('res.partner', 'Partner')\n    name = fields.Char(string='Name')\n    tag_ids = fields.Many2many('t', 'r', 'a', 'b', 'Tag')\n    other = fields.Char('Something')\n    ref = fields.Char('Ref', related='x')\n"
        );
        assert_eq!(messages(&ATTRIBUTE_STRING_REDUNDANT, &src).len(), 3);
        assert_eq!(py_title("sale order line"), "Sale Order Line");
    }

    #[test]
    fn inheritable_methods() {
        let src = format!(
            "{HEADER}    def _compute_a(self):\n        pass\n\n    def _default_b(self):\n        pass\n\n    a = fields.Float(compute=_compute_a)\n    b = fields.Char(default=_default_b)\n    c = fields.Char(default=later)\n\n    def later(self):\n        pass\n"
        );
        let strings = messages(&INHERITABLE_METHOD_STRING, &src);
        assert_eq!(strings.len(), 1);
        assert!(strings[0].1.starts_with("Use string method name `\"_compute_a\"`"));
        let lambdas = messages(&INHERITABLE_METHOD_LAMBDA, &src);
        assert_eq!(lambdas.len(), 1);
        assert!(lambdas[0].1.starts_with("Use `default=lambda self: self._default_b()`"));
    }
}
