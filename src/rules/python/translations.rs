//! Translation rules: pylint-odoo's own checks on `_()` calls, and pylint's
//! logging checks that pylint-odoo applies to `_()` (`translation-*`).

use super::format_strings::{
    format_positional_count, parse_format_fields, parse_percent, printf_positional_count, PercentError,
};
use super::{source_of, str_value};
use crate::checker::{PythonContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::pyliteral::repr_str;
use crate::rules::{Check, Rule};
use crate::semantic::func_name;
use crate::visit::{walk, Node, Scope};
use ruff_python_ast::{Expr, ExprCall, Operator, Stmt};
use ruff_text_size::{Ranged, TextSize};
use std::path::Path;

const TRANSLATION_METHODS: &[&str] = &["_", "_lt"];
const ODOO_EXCEPTIONS: &[&str] = &[
    "AccessDenied",
    "AccessError",
    "CacheMiss",
    "except_orm",
    "MissingError",
    "RedirectWarning",
    "UserError",
    "ValidationError",
    "Warning",
];
/// pylint's list of formattings that may stay lazy, as printed with the
/// logging checks themselves disabled (the pylint-odoo setup).
const LAZY_FORMATTING: &str = "lazy % or .format() or %";
const V14: Option<OdooVersion> = Some(OdooVersion::new(14, 0));

/// What pylint's logging checker finds in a translation call.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Finding {
    NotLazy,
    FormatInterpolation,
    FstringInterpolation,
    UnsupportedFormat(char, usize),
    FormatTruncated,
    TooManyArgs,
    TooFewArgs,
}

macro_rules! logging_rule {
    ($const:ident, $check:ident, $code:literal, $name:literal, $summary:literal, $doc:literal, $matches:pat) => {
        pub const $const: Rule = Rule {
            code: $code,
            name: $name,
            summary: $summary,
            doc: $doc,
            check: Check::Python($check),
            min_odoo: V14,
            max_odoo: None,
        };

        fn $check(ctx: &PythonContext, reporter: &mut Reporter) {
            for (offset, finding) in logging_findings(ctx) {
                if let $matches = finding {
                    reporter.report(&$const, offset, message(finding));
                }
            }
        }
    };
}

logging_rule!(
    TRANSLATION_NOT_LAZY,
    check_not_lazy,
    "W8301",
    "translation-not-lazy",
    "A string is formatted with `%` or `+` before it is translated.",
    r#"
## What it does

Reports `_("..." % value)`, `_("..." + value)` and `_("...") % value`.

## Why is this bad?

`_()` looks up the exact string in the translation catalogue. Once the value
is formatted in, the string is never found and stays untranslated. Pass the
values as arguments instead, so `_()` formats after translating.

## Example

```python
raise UserError(_("Order %s is locked") % order.name)
```

Use instead:

```python
raise UserError(_("Order %s is locked", order.name))
```

Ruff's `INT003` (flake8-gettext) is similar, for `_()` only.
"#,
    Finding::NotLazy
);

logging_rule!(
    TRANSLATION_FORMAT_INTERPOLATION,
    check_format_interpolation,
    "W8302",
    "translation-format-interpolation",
    "A string is formatted with `.format()` before it is translated.",
    r#"
## What it does

Reports `_("...".format(...))` and `_("...").format(...)`.

## Why is this bad?

The formatted string is not in the translation catalogue, so it stays
untranslated. Pass the values as arguments to `_()`.

Ruff's `INT002` (flake8-gettext) is similar, for `_()` only.
"#,
    Finding::FormatInterpolation
);

logging_rule!(
    TRANSLATION_FSTRING_INTERPOLATION,
    check_fstring_interpolation,
    "W8303",
    "translation-fstring-interpolation",
    "An f-string is translated.",
    r#"
## What it does

Reports f-strings passed to `_()`.

## Why is this bad?

The f-string is formatted before `_()` sees it, so the translation catalogue
never contains it and it stays untranslated.

## Example

```python
_(f"Invoice {name} is paid")
```

Use instead:

```python
_("Invoice %s is paid", name)
```

Ruff's `INT001` (flake8-gettext) is similar, for `_()` only.
"#,
    Finding::FstringInterpolation
);

logging_rule!(
    TRANSLATION_UNSUPPORTED_FORMAT,
    check_unsupported_format,
    "E8300",
    "translation-unsupported-format",
    "A translated string has an invalid `%` conversion character.",
    r#"
## What it does

Reports `_()` format strings with an unknown `%` conversion, such as `%y`,
when arguments are passed.

## Why is this bad?

Formatting raises `ValueError` at run time, in the user's language only.
"#,
    Finding::UnsupportedFormat(..)
);

logging_rule!(
    TRANSLATION_FORMAT_TRUNCATED,
    check_format_truncated,
    "E8301",
    "translation-format-truncated",
    "A translated format string ends in the middle of a `%` conversion.",
    r#"
## What it does

Reports `_()` format strings that end inside a `%` specifier, such as
`"50%"`, when arguments are passed. Write `%%` for a literal percent sign.

## Why is this bad?

Formatting raises `ValueError` at run time.
"#,
    Finding::FormatTruncated
);

logging_rule!(
    TRANSLATION_TOO_MANY_ARGS,
    check_too_many_args,
    "E8305",
    "translation-too-many-args",
    "`_()` gets more arguments than its format string uses.",
    r#"
## What it does

Reports `_("...", args)` calls with more positional arguments than the
format string has placeholders.

## Why is this bad?

Formatting raises `TypeError` at run time.
"#,
    Finding::TooManyArgs
);

logging_rule!(
    TRANSLATION_TOO_FEW_ARGS,
    check_too_few_args,
    "E8306",
    "translation-too-few-args",
    "`_()` gets fewer arguments than its format string needs.",
    r#"
## What it does

Reports `_("...", args)` calls with fewer positional arguments than the
format string has placeholders.

## Why is this bad?

Formatting raises `TypeError` at run time.
"#,
    Finding::TooFewArgs
);

pub const TRANSLATION_REQUIRED: Rule = Rule {
    code: "C8107",
    name: "translation-required",
    summary: "A user-facing string is not translated.",
    doc: r#"
## What it does

Reports string literals passed untranslated to Odoo exceptions (`raise
UserError("...")`, `ValidationError` and the other exceptions of
`odoo.exceptions`) and to `message_post()` outside test files.

## Why is this bad?

Users see these texts; without `_()` they are always in English.

## Example

```python
raise UserError("You cannot delete a posted entry.")
```

Use instead (`self.env._` from Odoo 18.0):

```python
raise UserError(self.env._("You cannot delete a posted entry."))
```
"#,
    check: Check::Python(check_translation_required),
    min_odoo: None,
    max_odoo: None,
};

pub const TRANSLATION_CONTAINS_VARIABLE: Rule = Rule {
    code: "W8115",
    name: "translation-contains-variable",
    summary: "A translated string is formatted inside `_()` (Odoo 13.0 and earlier).",
    doc: r#"
## What it does

For Odoo 13.0 and earlier, reports `_("..." % values)` and
`_("...".format(...))` and shows the corrected call. From Odoo 14.0 the
`translation-*` checks cover this.

## Why is this bad?

The formatted string is not in the translation catalogue, so it stays
untranslated.
"#,
    check: Check::Python(check_contains_variable),
    min_odoo: None,
    max_odoo: Some(OdooVersion::new(13, 0)),
};

pub const TRANSLATION_POSITIONAL_USED: Rule = Rule {
    code: "W8120",
    name: "translation-positional-used",
    summary: "A translated string has several positional placeholders.",
    doc: r#"
## What it does

Reports translated strings with two or more positional placeholders, such as
`%s ... %s` or `{} ... {}`.

## Why is this bad?

Translators cannot change the order of positional placeholders, but other
languages often need the values in another order.

## Example

```python
_("%s created %s", user.name, record.name)
```

Use instead:

```python
_("%(user)s created %(record)s", user=user.name, record=record.name)
```
"#,
    check: Check::Python(check_positional_used),
    min_odoo: None,
    max_odoo: None,
};

pub const PREFER_ENV_TRANSLATION: Rule = Rule {
    code: "W8161",
    name: "prefer-env-translation",
    summary: "`_()` is used instead of `self.env._()`.",
    doc: r#"
## What it does

Reports calls to the global `_()` function from Odoo 18.0.

## Why is this bad?

`self.env._()` translates in the language of the environment directly. The
global `_()` has to inspect the call stack to find it, which is slower and
fails outside methods. See [odoo/odoo#174844](https://github.com/odoo/odoo/pull/174844).

## Fix safety

Safe: `_(...)` becomes `self.env._(...)` inside methods of Odoo models
where `self` is the record. Elsewhere (static methods, functions, a lambda
with its own `self`) there is no fix. Remove the unused `_` import afterwards
(Ruff's `F401` does so).
"#,
    check: Check::Python(check_prefer_env),
    min_odoo: Some(OdooVersion::new(18, 0)),
    max_odoo: None,
};

pub const TRANSLATION_INJECTION: Rule = Rule {
    code: "E8151",
    name: "translation-injection",
    summary: "`.format()` is called on a translated string.",
    doc: r#"
## What it does

Reports `_("...").format(...)`.

## Why is this bad?

A translation can contain attribute or index lookups such as
`{0.__class__}`, which `str.format` evaluates: a translator, or anyone who can
edit translations, can read data through it. Use `%` placeholders and pass
the values to `_()`. See
[Be careful with str.format](https://lucumr.pocoo.org/2016/12/29/careful-with-str-format/).
"#,
    check: Check::Python(check_injection),
    min_odoo: None,
    max_odoo: None,
};

fn message(finding: Finding) -> String {
    match finding {
        Finding::NotLazy | Finding::FormatInterpolation | Finding::FstringInterpolation => {
            format!("Use {LAZY_FORMATTING} formatting in odoo._ functions")
        }
        Finding::UnsupportedFormat(c, index) => format!(
            "Unsupported odoo._ format character {} ({:#x}) at index {index}",
            repr_str(&c.to_string()),
            c as u32
        ),
        Finding::FormatTruncated => "Logging format string ends in middle of conversion specifier".into(),
        Finding::TooManyArgs => "Too many arguments for odoo._ format string".into(),
        Finding::TooFewArgs => "Not enough arguments for odoo._ format string".into(),
    }
}

fn is_str_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::StringLiteral(_))
}

/// Whether astroid infers `expr` to a string constant: a literal or a `+`
/// of such.
fn infers_to_str(expr: &Expr) -> bool {
    match expr {
        Expr::StringLiteral(_) => true,
        Expr::BinOp(b) if matches!(b.op, Operator::Add) => infers_to_str(&b.left) && infers_to_str(&b.right),
        _ => false,
    }
}

/// pylint's `_is_node_explicit_str_concatenation`.
fn is_explicit_concatenation(expr: &Expr) -> bool {
    let Expr::BinOp(b) = expr else { return false };
    (is_str_literal(&b.left) || is_explicit_concatenation(&b.left))
        && (is_str_literal(&b.right) || is_explicit_concatenation(&b.right))
}

/// pylint's `is_complex_format_str` for a string literal.
fn is_complex_format(s: &str) -> bool {
    parse_format_fields(s).is_some_and(|fields| fields.iter().any(|f| !f.spec.is_empty()))
}

fn has_star_args(call: &ExprCall) -> bool {
    call.arguments.args.iter().any(Expr::is_starred_expr) || call.arguments.keywords.iter().any(|kw| kw.arg.is_none())
}

/// pylint's `_check_log_method` for `_(format, *args)`, with pylint-odoo's
/// change that a string without arguments is not checked.
fn check_translation_call(format: &Expr, extra_args: &[Expr]) -> Option<Finding> {
    match format {
        Expr::BinOp(binop) => {
            let emit = match binop.op {
                Operator::Mod => true,
                Operator::Add if !is_explicit_concatenation(format) => {
                    infers_to_str(&binop.left) || infers_to_str(&binop.right)
                }
                _ => false,
            };
            emit.then_some(Finding::NotLazy)
        }
        Expr::Call(call) => {
            let Expr::Attribute(attr) = &*call.func else {
                return None;
            };
            let Expr::StringLiteral(s) = &*attr.value else {
                return None;
            };
            (attr.attr.as_str() == "format" && !is_complex_format(s.value.to_str()))
                .then_some(Finding::FormatInterpolation)
        }
        Expr::FString(f) => {
            let literal: String = super::str_value(&Expr::FString(f.clone())).unwrap_or_default();
            let percent_formatting =
                literal.contains('%') && ["%s", "%d", "%f", "%r"].iter().any(|p| literal.contains(p));
            (!percent_formatting).then_some(Finding::FstringInterpolation)
        }
        Expr::StringLiteral(_)
        | Expr::BytesLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_) => {
            let supplied = extra_args.len();
            if supplied == 0 {
                return None;
            }
            let format_string = match format {
                Expr::StringLiteral(s) => Some(s.value.to_str().to_string()),
                Expr::BytesLiteral(b) => {
                    let bytes: Vec<u8> = b
                        .value
                        .as_slice()
                        .iter()
                        .flat_map(|p| p.value.iter().copied())
                        .collect();
                    Some(String::from_utf8_lossy(&bytes).into_owned())
                }
                _ => None,
            };
            let required = match format_string {
                Some(s) => match parse_percent(&s) {
                    Ok(parsed) if !parsed.keys.is_empty() => return None,
                    Ok(parsed) => parsed.num_args,
                    Err(PercentError::Unsupported(index)) => {
                        let c = s.chars().nth(index).unwrap_or('?');
                        return Some(Finding::UnsupportedFormat(c, index));
                    }
                    Err(PercentError::Incomplete) => return Some(Finding::FormatTruncated),
                },
                None => 0,
            };
            if supplied > required {
                Some(Finding::TooManyArgs)
            } else if supplied < required {
                Some(Finding::TooFewArgs)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// All findings of the logging-derived checks, at their report positions.
fn logging_findings(ctx: &PythonContext) -> Vec<(TextSize, Finding)> {
    let mut findings = Vec::new();
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(expr) = node else { return };
        match expr {
            Expr::Call(call) => {
                // `_("...").format(x)` is checked as `_("...".format(x))`.
                if let Expr::Attribute(attr) = &*call.func {
                    if let Expr::Call(inner) = &*attr.value {
                        if attr.attr.as_str() == "format"
                            && func_name(&inner.func) == "_"
                            && inner.arguments.args.len() == 1
                            && inner.arguments.keywords.is_empty()
                        {
                            if let Expr::StringLiteral(s) = &inner.arguments.args[0] {
                                if !is_complex_format(s.value.to_str()) {
                                    findings.push((call.start(), Finding::FormatInterpolation));
                                }
                            }
                            return;
                        }
                    }
                }
                if func_name(&call.func) != "_" || has_star_args(call) {
                    return;
                }
                let Some((format, rest)) = call.arguments.args.split_first() else {
                    return;
                };
                if let Some(finding) = check_translation_call(format, rest) {
                    findings.push((call.start(), finding));
                }
            }
            // `_("...") % x` is checked as `_("..." % x)`.
            Expr::BinOp(binop) if matches!(binop.op, Operator::Mod) => {
                let Expr::Call(left) = &*binop.left else { return };
                let single_arg = left.arguments.args.len() == 1 && left.arguments.keywords.is_empty();
                if func_name(&left.func) == "_" && single_arg {
                    findings.push((binop.start(), Finding::NotLazy));
                }
            }
            _ => {}
        }
    });
    findings
}

fn translation_method(ctx: &PythonContext) -> &'static str {
    if ctx.settings.target_version >= OdooVersion::new(18, 0) {
        "self.env._"
    } else {
        "_"
    }
}

/// astroid's `as_string()` for the nodes these messages quote: string
/// literals as their `repr`, other expressions on a single line (astroid
/// renders them without the source's line breaks, which matters for checks
/// that count placeholders per line).
fn as_string(ctx: &PythonContext, expr: &Expr) -> String {
    match expr {
        Expr::StringLiteral(s) => repr_str(s.value.to_str()),
        other => single_line(source_of(ctx.source, other)),
    }
}

/// Joins continuation lines: each line break and the indentation after it
/// are removed.
fn single_line(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut lines = source.split('\n');
    if let Some(first) = lines.next() {
        out.push_str(first.trim_end_matches('\r'));
    }
    for line in lines {
        out.push_str(line.trim_end_matches('\r').trim_start());
    }
    out
}

fn is_string_like(expr: &Expr) -> bool {
    matches!(expr, Expr::StringLiteral(_) | Expr::FString(_))
}

fn check_translation_required(ctx: &PythonContext, reporter: &mut Reporter) {
    let tl = translation_method(ctx);
    let in_tests = Path::new(ctx.file_path)
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == "tests");
    walk(ctx.parsed.suite(), |node, _| match node {
        Node::Expr(Expr::Call(call)) if !in_tests => {
            let Expr::Attribute(attr) = &*call.func else { return };
            if attr.attr.as_str() != "message_post" {
                return;
            }
            let positional = call.arguments.args.iter().map(|a| ("", a));
            let keywords = call
                .arguments
                .keywords
                .iter()
                .filter_map(|kw| kw.arg.as_ref().map(|a| (a.as_str(), &kw.value)));
            for (keyword, value) in positional.chain(keywords) {
                if !keyword.is_empty() && keyword != "subject" && keyword != "body" {
                    continue;
                }
                let quoted = match value {
                    v if is_string_like(v) => Some(as_string(ctx, v)),
                    Expr::BinOp(b) if matches!(b.op, Operator::Mod) && is_string_like(&b.left) => {
                        let translatable_right = match &*b.right {
                            Expr::Call(_) => true,
                            Expr::Tuple(t) => t.elts.iter().all(Expr::is_call_expr),
                            Expr::List(l) => l.elts.iter().all(Expr::is_call_expr),
                            _ => false,
                        };
                        (!translatable_right).then(|| as_string(ctx, &b.left))
                    }
                    Expr::Call(c) => match &*c.func {
                        Expr::Attribute(a) if a.attr.as_str() == "format" && is_string_like(&a.value) => {
                            Some(as_string(ctx, &a.value))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(quoted) = quoted {
                    let keyword = if keyword.is_empty() {
                        String::new()
                    } else {
                        format!("{keyword}=")
                    };
                    reporter.report(
                        &TRANSLATION_REQUIRED,
                        call.start(),
                        format!(
                            "String parameter on \"message_post\" requires translation. Use {keyword}{tl}({quoted})"
                        ),
                    );
                }
            }
        }
        Node::Stmt(Stmt::Raise(raise)) => {
            let Some(Expr::Call(exc)) = raise.exc.as_deref() else {
                return;
            };
            let Some(first) = exc.arguments.args.first() else {
                return;
            };
            let exception = func_name(&exc.func);
            let argument = match first {
                Expr::Call(c) if func_name(&c.func) == "format" => match &*c.func {
                    Expr::Attribute(a) => &*a.value,
                    _ => first,
                },
                Expr::BinOp(b) => &*b.left,
                _ => first,
            };
            if str_value(argument).is_some() && ODOO_EXCEPTIONS.contains(&exception) {
                reporter.report(
                    &TRANSLATION_REQUIRED,
                    raise.start(),
                    format!(
                        "String parameter on \"{exception}\" requires translation. Use {tl}({})",
                        as_string(ctx, argument)
                    ),
                );
            }
        }
        _ => {}
    });
}

/// `_(...)` and `_lt(...)` calls with at least one argument.
fn translation_calls<'a>(ctx: &PythonContext<'a>) -> Vec<&'a ExprCall> {
    let mut calls = Vec::new();
    walk(ctx.parsed.suite(), |node, _| {
        if let Node::Expr(Expr::Call(call)) = node {
            if !call.arguments.args.is_empty() && TRANSLATION_METHODS.contains(&func_name(&call.func)) {
                calls.push(call);
            }
        }
    });
    calls
}

fn check_contains_variable(ctx: &PythonContext, reporter: &mut Reporter) {
    for call in translation_calls(ctx) {
        let arg = &call.arguments.args[0];
        let suggestion = match arg {
            Expr::BinOp(b) if matches!(b.op, Operator::Mod) => {
                let (left, right) = (as_string(ctx, &b.left), source_of(ctx.source, &*b.right));
                Some((format!("{left} % {right}"), format!("_({left}) % {right}")))
            }
            Expr::Call(c) => match &*c.func {
                Expr::Attribute(a) if a.attr.as_str() == "format" && is_str_literal(&a.value) => {
                    let params: Vec<&str> = c
                        .arguments
                        .args
                        .iter()
                        .map(|p| source_of(ctx.source, p))
                        .chain(c.arguments.keywords.iter().map(|kw| source_of(ctx.source, kw)))
                        .collect();
                    Some((
                        source_of(ctx.source, arg).to_string(),
                        format!("_({}).format({})", as_string(ctx, &a.value), params.join(", ")),
                    ))
                }
                _ => None,
            },
            _ => None,
        };
        if let Some((wrong, right)) = suggestion {
            reporter.report(
                &TRANSLATION_CONTAINS_VARIABLE,
                call.start(),
                format!("Translatable term in \"{wrong}\" contains variables. Use {right} instead"),
            );
        }
    }
}

fn check_positional_used(ctx: &PythonContext, reporter: &mut Reporter) {
    for call in translation_calls(ctx) {
        let text = as_string(ctx, &call.arguments.args[0]);
        let printf = printf_positional_count(&text).unwrap_or(0);
        if printf >= 2 || format_positional_count(&text) >= 2 {
            reporter.report(
                &TRANSLATION_POSITIONAL_USED,
                call.start(),
                format!(
                    "Translation method _({text}) is using positional string printf formatting with multiple arguments. Use named placeholder `_(\"%(placeholder)s\")` instead."
                ),
            );
        }
    }
}

fn check_prefer_env(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, scopes| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        if call.arguments.args.is_empty() || !matches!(&*call.func, Expr::Name(n) if n.id.as_str() == "_") {
            return;
        }
        let violation = reporter.report(
            &PREFER_ENV_TRANSLATION,
            call.start(),
            "Better using self.env._ More info at https://github.com/odoo/odoo/pull/174844",
        );
        if self_is_a_record(ctx, scopes) {
            violation.fix = Some(Fix::safe(
                "Use `self.env._`",
                vec![Edit::replace(
                    call.func.start().to_usize(),
                    call.func.end().to_usize(),
                    "self.env._",
                )],
            ));
        }
    });
}

/// Whether `self` is a record where the scopes end: inside a method of an
/// Odoo model whose first parameter is `self`, not rebound by an inner
/// function or lambda.
fn self_is_a_record(ctx: &PythonContext, scopes: &[Scope]) -> bool {
    let Some(method) = scopes.windows(2).rposition(|pair| {
        matches!(pair, [Scope::Class(class), Scope::Function(_)] if ctx.semantic.odoo_model_kind(class).is_some())
    }) else {
        return false;
    };
    let Scope::Function(function) = scopes[method + 1] else {
        return false;
    };
    let first = function
        .parameters
        .posonlyargs
        .iter()
        .chain(&function.parameters.args)
        .next();
    let is_static = function
        .decorator_list
        .iter()
        .any(|d| matches!(&d.expression, Expr::Name(n) if n.id.as_str() == "staticmethod"));
    let rebound = scopes[method + 2..].iter().any(|scope| match scope {
        Scope::Function(f) => f.parameters.includes("self"),
        Scope::Lambda(l) => l.parameters.as_ref().is_some_and(|p| p.includes("self")),
        _ => false,
    });
    !is_static && !rebound && first.is_some_and(|p| p.parameter.name.as_str() == "self")
}

fn check_injection(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(attr) = &*call.func else { return };
        let Expr::Call(inner) = &*attr.value else { return };
        if attr.attr.as_str() == "format" && TRANSLATION_METHODS.contains(&func_name(&inner.func)) {
            reporter.report(
                &TRANSLATION_INJECTION,
                call.start(),
                "Do not use str.format on translation methods. Use placeholders instead. Reference: https://lucumr.pocoo.org/2016/12/29/careful-with-str-format/",
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::run_python_rule;

    fn codes(rule: &Rule, src: &str) -> usize {
        run_python_rule(rule, src).len()
    }

    #[test]
    fn logging_style() {
        let src = r#"
_("a %s" % x)
_("a" + x)
_("a" + "b")
_("{}".format(x))
_("{:.2f}".format(x))
_("a %s") % x
_("a {}").format(x)
_(f"a {x}")
_(f"a %s {x}")
_("a %y", x)
_("50%", x)
_("%s", x, y)
_("%s %s", x)
_("%(a)s", x)
_("plain")
_lt("a %s" % x)
"#;
        assert_eq!(codes(&TRANSLATION_NOT_LAZY, src), 3);
        assert_eq!(codes(&TRANSLATION_FORMAT_INTERPOLATION, src), 2);
        assert_eq!(codes(&TRANSLATION_FSTRING_INTERPOLATION, src), 1);
        let unsupported = run_python_rule(&TRANSLATION_UNSUPPORTED_FORMAT, src);
        assert_eq!(
            unsupported[0].message,
            "Unsupported odoo._ format character 'y' (0x79) at index 3"
        );
        assert_eq!(codes(&TRANSLATION_FORMAT_TRUNCATED, src), 1);
        assert_eq!(codes(&TRANSLATION_TOO_MANY_ARGS, src), 1);
        assert_eq!(codes(&TRANSLATION_TOO_FEW_ARGS, src), 1);
        assert_eq!(
            run_python_rule(&TRANSLATION_NOT_LAZY, src)[0].message,
            "Use lazy % or .format() or % formatting in odoo._ functions"
        );
    }

    #[test]
    fn required() {
        let src = "raise UserError('Nope')\nraise UserError(_('Ok'))\nraise ValueError('x')\nraise UserError('a %s' % x)\nrec.message_post(body='Hi')\nrec.message_post(body='Hi %s' % _('x'))\nrec.message_post(subject=f'S {x}', other='y')\n";
        let v: Vec<_> = run_python_rule(&TRANSLATION_REQUIRED, src)
            .into_iter()
            .map(|v| v.message)
            .collect();
        assert_eq!(
            v,
            vec![
                "String parameter on \"UserError\" requires translation. Use _('Nope')",
                "String parameter on \"UserError\" requires translation. Use _('a %s')",
                "String parameter on \"message_post\" requires translation. Use body=_('Hi')",
                "String parameter on \"message_post\" requires translation. Use subject=_(f'S {x}')",
            ]
        );
    }

    #[test]
    fn contains_variable() {
        let src = "_('a %s' % x)\n_('a {}'.format(x, y=1))\n_('a')\n";
        let v: Vec<_> = run_python_rule(&TRANSLATION_CONTAINS_VARIABLE, src)
            .into_iter()
            .map(|v| v.message)
            .collect();
        assert_eq!(
            v,
            vec![
                "Translatable term in \"'a %s' % x\" contains variables. Use _('a %s') % x instead",
                "Translatable term in \"'a {}'.format(x, y=1)\" contains variables. Use _('a {}').format(x, y=1) instead",
            ]
        );
    }

    #[test]
    fn multiline_arguments_count_as_one_line() {
        let src = "_('a {}, {b}'.format(\n    x, b=y))\n";
        assert_eq!(codes(&TRANSLATION_POSITIONAL_USED, src), 0);
        assert_eq!(single_line("f(\n    a,\n    b)"), "f(a,b)");
    }

    #[test]
    fn positional_and_others() {
        let src = "_('%s %s', a, b)\n_('%(a)s %(b)s', a=1, b=2)\n_('{} {}')\nself.env._('%s and %d', a, b)\n_('x').format(y)\n";
        assert_eq!(codes(&TRANSLATION_POSITIONAL_USED, src), 3);
        assert_eq!(codes(&PREFER_ENV_TRANSLATION, src), 4);
        assert_eq!(codes(&TRANSLATION_INJECTION, src), 1);
    }
}
