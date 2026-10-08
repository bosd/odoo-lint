//! Rules on `.po` and `.pot` files, ported from OCA's `oca-checks-po`
//! (odoo-pre-commit-hooks) with its check names and messages. The checks
//! have no codes there; odoo-lint numbers them `PO###`.

use crate::checker::Reporter;
use crate::po::pyformat::{percent_format, str_format, PercentArgs, PyError, Value};
use crate::po::{PoEntry, PoError, PoFile};
use crate::rules::{Check, Rule};
use crate::settings::Settings;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// What a PO rule sees.
pub struct PoContext<'a> {
    pub file_path: &'a str,
    /// The file contents with newlines normalised to `\n`.
    pub source: &'a str,
    pub po: Result<&'a PoFile, &'a PoError>,
    /// Name of the folder the file is in, e.g. `i18n` or `i18n_extra`.
    pub data_section: &'a str,
    pub settings: &'a Settings,
}

impl PoContext<'_> {
    fn entries(&self) -> impl Iterator<Item = &PoEntry> {
        self.po
            .ok()
            .into_iter()
            .flat_map(|po| po.entries.iter())
            .filter(|e| !e.obsolete)
    }
}

pub const PO_SYNTAX_ERROR: Rule = Rule {
    code: "PO001",
    name: "po-syntax-error",
    summary: "A PO file cannot be parsed.",
    doc: r#"
## What it does

Reports `.po` and `.pot` files that are not valid gettext files, or not UTF-8.
The message says what is wrong and where.

## Why is this bad?

Odoo fails to load the translations of the module, or skips them.
"#,
    check: Check::Po(check_syntax),
    min_odoo: None,
    max_odoo: None,
};

pub const PO_REQUIRES_MODULE: Rule = Rule {
    code: "PO002",
    name: "po-requires-module",
    summary: "A translation entry lacks its `#. module:` comment.",
    doc: r#"
## What it does

Checks that every entry has the `#. module: <module>` comment Odoo writes
when it exports translations.

## Why is this bad?

Odoo uses the comment to know which module a translation belongs to; entries
without it are not imported.
"#,
    check: Check::Po(check_requires_module),
    min_odoo: None,
    max_odoo: None,
};

pub const PO_PYTHON_PARSE_PRINTF: Rule = Rule {
    code: "PO003",
    name: "po-python-parse-printf",
    summary: "A translation does not match the `%` placeholders of its source.",
    doc: r#"
## What it does

For entries flagged `python-format`, formats the translation with the
arguments the source text expects (`%s`, `%d`, `%(name)s`) and reports the
error Python raises, such as a missing or extra placeholder.

## Why is this bad?

The translated message raises an exception at run time, in that language
only, often in an error message that then hides the real error.

## Example

```po
#, python-format
msgid "Order %s confirmed"
msgstr "Bestelling bevestigd"
```
"#,
    check: Check::Po(check_parse_printf),
    min_odoo: None,
    max_odoo: None,
};

pub const PO_PYTHON_PARSE_FORMAT: Rule = Rule {
    code: "PO004",
    name: "po-python-parse-format",
    summary: "A translation does not match the `{}` placeholders of its source.",
    doc: r#"
## What it does

For entries flagged `python-format`, formats the translation with
`str.format` and the arguments the source text expects (`{}`, `{0}`,
`{name}`) and reports the error Python raises.

## Why is this bad?

The translated message raises an exception at run time, in that language
only.
"#,
    check: Check::Po(check_parse_format),
    min_odoo: None,
    max_odoo: None,
};

pub const PO_DUPLICATE_MESSAGE_DEFINITION: Rule = Rule {
    code: "PO005",
    name: "po-duplicate-message-definition",
    summary: "The same `msgid` is translated more than once.",
    doc: r#"
## What it does

Reports `msgid`s that appear in more than one entry of a file. Files in an
`i18n_extra` folder are skipped.

## Why is this bad?

Odoo exports translations by `msgid` and keeps only one of them, so one of the
translations is lost on the next export. Use the `i18n_extra` folder for
translations that must differ.
"#,
    check: Check::Po(check_duplicate_messages),
    min_odoo: None,
    max_odoo: None,
};

pub const PO_DUPLICATE_MODEL_DEFINITION: Rule = Rule {
    code: "PO006",
    name: "po-duplicate-model-definition",
    summary: "The same `model:` reference is translated more than once.",
    doc: r#"
## What it does

Reports `#: model:...` references, such as a field description, that appear
in more than one entry.

## Why is this bad?

Only one of the translations is loaded; which one depends on the order of
the file.
"#,
    check: Check::Po(check_duplicate_models),
    min_odoo: None,
    max_odoo: None,
};

pub const PO_PRETTY_FORMAT: Rule = Rule {
    code: "PO007",
    name: "po-pretty-format",
    summary: "A PO file is not formatted the way Odoo exports it.",
    doc: r#"
## What it does

Checks that the file is formatted as Odoo (and polib) write it:

1. entries sorted by `msgid`;
2. lines wrapped at 78 columns;
3. no `msgstr` identical to its `msgid` (outside `i18n_extra`).

## Why is this bad?

Every export by Odoo or Weblate reformats the file, which turns small
translation changes into large, hard to review diffs.
"#,
    check: Check::Po(check_pretty_format),
    min_odoo: None,
    max_odoo: None,
};

fn check_syntax(ctx: &PoContext, reporter: &mut Reporter) {
    if let Err(error) = ctx.po {
        reporter.report_line(&PO_SYNTAX_ERROR, error.line.max(1), &error.message);
    }
}

static MODULE_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(module[s]?): (\w+)").unwrap());

fn check_requires_module(ctx: &PoContext, reporter: &mut Reporter) {
    for entry in ctx.entries() {
        if !MODULE_COMMENT.is_match(&entry.comment) {
            reporter.report_line(
                &PO_REQUIRES_MODULE,
                entry.linenum.max(1),
                "Translation entry requires comment `#. module: MODULE`",
            );
        }
    }
}

static PRINTF_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"%((?P<boost_ord>\d+)%|(?:(?P<ord>\d+)\$|\((?P<key>\w+)\))?(?P<fullvar>[+#-]*(?:\d+)?(?:\.\d+)?(hh\|h\|l\|ll)?(?P<type>[\w@])))",
    )
    .unwrap()
});

/// OCA's `_get_printf_str_args_kwargs`: dummy arguments for a `%` string.
fn printf_args(source: &str) -> Option<PercentArgs> {
    let text = source.replace("%%", "");
    let mut args = Vec::new();
    let mut kwargs = BTreeMap::new();
    for line in crate::po::splitlines(&text, false) {
        for caps in PRINTF_PATTERN.captures_iter(line) {
            let value = if caps.name("type").map(|t| t.as_str()) == Some("s") {
                Value::Str
            } else {
                Value::Int(0)
            };
            match caps.name("key") {
                Some(key) => {
                    kwargs.insert(key.as_str().to_string(), value);
                }
                None => args.push(value),
            }
        }
    }
    if !args.is_empty() {
        Some(PercentArgs::Tuple(args))
    } else if !kwargs.is_empty() {
        Some(PercentArgs::Dict(kwargs))
    } else {
        None
    }
}

/// OCA's `_get_format_str_args_kwargs`: dummy arguments for a `str.format`
/// string (faithful to its accumulation over lines).
fn format_args(source: &str) -> (Vec<Value>, BTreeMap<String, Value>) {
    let mut placeholders: Vec<String> = Vec::new();
    let mut numbers: Vec<usize> = Vec::new();
    let mut kwargs = BTreeMap::new();
    for line in crate::po::splitlines(source, false) {
        let Some(fields) = crate::rules::python::format_strings::parse_format_fields(line) else {
            continue;
        };
        placeholders.extend(fields.into_iter().map(|f| f.name));
        for placeholder in &placeholders {
            if placeholder.is_empty() {
                numbers.push(0);
            } else if placeholder.chars().all(|c| c.is_ascii_digit()) {
                numbers.push(placeholder.parse::<usize>().unwrap_or(0) + 1);
            } else {
                kwargs.insert(placeholder.clone(), Value::Int(0));
            }
        }
    }
    let count = match numbers.iter().max() {
        None => 0,
        Some(0) => numbers.len(),
        Some(max) => *max,
    };
    ((0..count as i64).map(Value::Int).collect(), kwargs)
}

enum ParseError {
    Printf(PyError),
    Format(PyError),
}

/// OCA's check of a `python-format` entry: the translation must format with
/// the arguments of the source, first with `%`, then with `str.format`.
fn parse_error(entry: &PoEntry) -> Option<ParseError> {
    if entry.msgstr.is_empty() || !entry.flags.iter().any(|f| f == "python-format") {
        return None;
    }
    if let Some(args) = printf_args(&entry.msgid) {
        if percent_format(&entry.msgid, &args).is_ok() {
            if let Err(error) = percent_format(&entry.msgstr, &args) {
                return Some(ParseError::Printf(error));
            }
        }
    }
    let (args, kwargs) = format_args(&entry.msgid);
    if (args.is_empty() && kwargs.is_empty()) || str_format(&entry.msgid, &args, &kwargs).is_err() {
        return None;
    }
    str_format(&entry.msgstr, &args, &kwargs).err().map(ParseError::Format)
}

fn check_parse_printf(ctx: &PoContext, reporter: &mut Reporter) {
    for entry in ctx.entries() {
        if let Some(ParseError::Printf(error)) = parse_error(entry) {
            reporter.report_line(
                &PO_PYTHON_PARSE_PRINTF,
                entry.msgid_line(),
                format!(
                    "Translation string couldn't be parsed correctly using str%variables {}",
                    error.repr()
                ),
            );
        }
    }
}

fn check_parse_format(ctx: &PoContext, reporter: &mut Reporter) {
    for entry in ctx.entries() {
        if let Some(ParseError::Format(error)) = parse_error(entry) {
            reporter.report_line(
                &PO_PYTHON_PARSE_FORMAT,
                entry.msgid_line(),
                format!(
                    "Translation string couldn't be parsed correctly using str.format {}",
                    error.repr()
                ),
            );
        }
    }
}

/// Groups entries by `key`, in order of first appearance.
fn group_by<'a, K: PartialEq>(entries: impl Iterator<Item = (K, &'a PoEntry)>) -> Vec<(K, Vec<&'a PoEntry>)> {
    let mut groups: Vec<(K, Vec<&PoEntry>)> = Vec::new();
    for (key, entry) in entries {
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.push(entry),
            None => groups.push((key, vec![entry])),
        }
    }
    groups
}

fn other_lines(entries: &[&PoEntry]) -> String {
    entries[1..]
        .iter()
        .map(|e| e.msgid_line().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_duplicate_messages(ctx: &PoContext, reporter: &mut Reporter) {
    if ctx.data_section == "i18n_extra" {
        return;
    }
    for (msgid, entries) in group_by(ctx.entries().map(|e| (e.msgid.as_str(), e))) {
        if entries.len() < 2 {
            continue;
        }
        let head: String = msgid.chars().take(40).filter(|c| *c != '\n' && *c != '\t').collect();
        let mut short = head.trim().to_string();
        if msgid.chars().count() > 40 {
            short.push_str("...");
        }
        reporter.report_line(
            &PO_DUPLICATE_MESSAGE_DEFINITION,
            entries[0].msgid_line(),
            format!(
                "Duplicate PO message definition `{short}` in lines {}. Odoo exports these items by msgid and delete one of them. Use the `i18n_extra` folder instead of `i18n` to ignore this message.",
                other_lines(&entries)
            ),
        );
    }
}

fn check_duplicate_models(ctx: &PoContext, reporter: &mut Reporter) {
    let references = ctx.entries().flat_map(|entry| {
        entry
            .occurrences
            .iter()
            .filter(|(path, _)| path.starts_with("model:"))
            .map(move |(path, _)| (path.as_str(), entry))
    });
    for (model, entries) in group_by(references) {
        if entries.len() < 2 {
            continue;
        }
        reporter.report_line(
            &PO_DUPLICATE_MODEL_DEFINITION,
            entries[0].msgid_line(),
            format!(
                "Translation for {model} has been defined more than once in line(s) {}",
                other_lines(&entries)
            ),
        );
    }
}

/// The file as OCA's `po-pretty-format` wants it.
pub fn pretty_format(po: &PoFile, data_section: &str) -> String {
    let mut po = po.clone();
    po.entries.sort_by(|a, b| a.msgid.cmp(&b.msgid));
    if data_section != "i18n_extra" {
        for entry in &mut po.entries {
            if entry.msgid == entry.msgstr {
                entry.msgstr.clear();
            }
        }
    }
    po.to_po_string()
}

fn check_pretty_format(ctx: &PoContext, reporter: &mut Reporter) {
    let Ok(po) = ctx.po else { return };
    if pretty_format(po, ctx.data_section) != ctx.source {
        reporter.report_line(&PO_PRETTY_FORMAT, 1, "Wrong formatting");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Violation;
    use crate::linter::lint_po_source;

    fn run(rule: &Rule, source: &str) -> Vec<Violation> {
        lint_po_source("i18n/nl.po", source, &[rule], &Settings::default())
    }

    const HEADER: &str = "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\n";

    #[test]
    fn syntax_error() {
        let v = run(&PO_SYNTAX_ERROR, "msgid \"a\"\nmsgstr \"b\" \"c\"\n");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 2);
    }

    #[test]
    fn requires_module() {
        let src = format!("{HEADER}#. module: acme\nmsgid \"A\"\nmsgstr \"\"\n\nmsgid \"B\"\nmsgstr \"\"\n");
        let v = run(&PO_REQUIRES_MODULE, &src);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 9);
    }

    #[test]
    fn parse_errors() {
        let src = format!(
            "{HEADER}#, python-format\nmsgid \"Order %s\"\nmsgstr \"Bestelling\"\n\n#, python-format\nmsgid \"{{}} and {{}}\"\nmsgstr \"{{}} en {{}} en {{}}\"\n\n#, python-format\nmsgid \"%d items\"\nmsgstr \"%s artikelen\"\n"
        );
        let printf: Vec<_> = run(&PO_PYTHON_PARSE_PRINTF, &src)
            .into_iter()
            .map(|v| (v.line, v.message))
            .collect();
        assert_eq!(
            printf,
            vec![(
                6,
                "Translation string couldn't be parsed correctly using str%variables TypeError('not all arguments converted during string formatting')".to_string()
            )]
        );
        let format: Vec<_> = run(&PO_PYTHON_PARSE_FORMAT, &src)
            .into_iter()
            .map(|v| v.message)
            .collect();
        assert_eq!(
            format,
            vec!["Translation string couldn't be parsed correctly using str.format IndexError('Replacement index 2 out of range for positional args tuple')"]
        );
    }

    #[test]
    fn duplicates() {
        let src = format!(
            "{HEADER}#: model:ir.model.fields,field_description:m.f\nmsgid \"A\"\nmsgstr \"\"\n\n#: model:ir.model.fields,field_description:m.f\nmsgid \"A\"\nmsgstr \"\"\n"
        );
        let messages = run(&PO_DUPLICATE_MESSAGE_DEFINITION, &src);
        assert_eq!(messages.len(), 1);
        assert!(messages[0]
            .message
            .starts_with("Duplicate PO message definition `A` in lines 10."));
        let models = run(&PO_DUPLICATE_MODEL_DEFINITION, &src);
        assert_eq!(
            models[0].message,
            "Translation for model:ir.model.fields,field_description:m.f has been defined more than once in line(s) 10"
        );
    }

    #[test]
    fn pretty_format_check() {
        let pretty = "#\nmsgid \"\"\nmsgstr \"\"\n\nmsgid \"A\"\nmsgstr \"a\"\n\nmsgid \"B\"\nmsgstr \"\"\n";
        assert!(run(&PO_PRETTY_FORMAT, pretty).is_empty());
        let unsorted = "#\nmsgid \"\"\nmsgstr \"\"\n\nmsgid \"B\"\nmsgstr \"\"\n\nmsgid \"A\"\nmsgstr \"a\"\n";
        assert_eq!(run(&PO_PRETTY_FORMAT, unsorted).len(), 1);
        let same = "#\nmsgid \"\"\nmsgstr \"\"\n\nmsgid \"A\"\nmsgstr \"A\"\n";
        assert_eq!(run(&PO_PRETTY_FORMAT, same).len(), 1);
    }
}
