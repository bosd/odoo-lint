//! Changes of Odoo 20.0 (`odoo/upgrade_code/19.*` and `20.0-*`, and the
//! 19.0 -> 20.0 differences of views, schemas and models).

use super::{arch_elements, data_elements, record_field, view_archs};
use crate::checker::{ManifestContext, PythonContext, Reporter};
use crate::fix::{Edit, Fix};
use crate::odoo_version::OdooVersion;
use crate::rules::python::calls::searched_model;
use crate::rules::python::{classes, methods, source_of};
use crate::rules::xml::{at, child_field, delete_element, is, start_tag_end, XmlContext, XmlReporter};
use crate::rules::{Check, Rule};
use crate::semantic::func_name;
use crate::visit::{walk, Node};
use crate::xml::XmlFile;
use regex::Regex;
use roxmltree::Attribute;
use ruff_python_ast::{Expr, Stmt, StmtImportFrom};
use ruff_text_size::{Ranged, TextRange};
use std::collections::HashSet;
use std::sync::LazyLock;

const ODOO_20: Option<OdooVersion> = Some(OdooVersion::new(20, 0));

fn rename(range: TextRange, new: &str) -> Edit {
    Edit::replace(range.start().to_usize(), range.end().to_usize(), new)
}

fn rename_attribute(attribute: &Attribute, new: &str) -> Edit {
    let start = attribute.range().start;
    Edit::replace(start, start + attribute.name().len(), new)
}

fn delete_attribute(file: &XmlFile, attribute: &Attribute) -> Edit {
    let range = attribute.range();
    let start = file.source[..range.start]
        .trim_end_matches([' ', '\t', '\n', '\r'])
        .len();
    Edit::delete(start, range.end)
}

/// Rewrites `from module import a, b` when some names moved to other
/// modules: the names that stay keep the statement, the others get one
/// statement per new module. `None` when nothing moved or a name is gone.
fn split_import(
    source: &str,
    import: &StmtImportFrom,
    module: &str,
    moved: impl Fn(&str) -> Option<&'static str>,
) -> Option<Edit> {
    let mut stay = Vec::new();
    let mut targets: Vec<(&str, Vec<String>)> = Vec::new();
    for alias in &import.names {
        let text = source_of(source, alias).to_string();
        match moved(alias.name.as_str()) {
            Some(target) => match targets.iter_mut().find(|(t, _)| *t == target) {
                Some((_, names)) => names.push(text),
                None => targets.push((target, vec![text])),
            },
            None => stay.push(text),
        }
    }
    if targets.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    if !stay.is_empty() {
        lines.push(format!("from {module} import {}", stay.join(", ")));
    }
    for (target, names) in targets {
        lines.push(format!("from {target} import {}", names.join(", ")));
    }
    let start = import.start().to_usize();
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let indent = &source[line_start..start];
    if !indent.trim().is_empty() {
        return None;
    }
    Some(Edit::replace(
        start,
        import.end().to_usize(),
        lines.join(&format!("\n{indent}")),
    ))
}

fn import_module(import: &StmtImportFrom) -> Option<&str> {
    (import.level == 0).then_some(())?;
    import.module.as_ref().map(|m| m.as_str())
}

// --- XML ------------------------------------------------------------------

pub const XML_ACCESS_RECORDS: Rule = Rule {
    code: "U2002",
    name: "upgrade-ir-access-records",
    summary: "`ir.rule` or `ir.model.access` records, models replaced by `ir.access` in Odoo 20.0.",
    doc: r#"
## What it does

Reports `<record model="ir.rule">` and `<record model="ir.model.access">`
in data files.

## Why is this bad?

Odoo 20.0 merged access rights and record rules into one model,
`ir.access` (`operation`, `domain`, group): both old models are gone, so
the data file fails to load. Odoo's upgrade script `19.4-00-ir-access`
converts them; review its output, since a rule without a group became a
restriction for everyone.
"#,
    check: Check::Xml(check_access_records),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_access_records(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for record in data_elements(file, &["record"]) {
            let Some(model) = record
                .attribute("model")
                .filter(|m| matches!(*m, "ir.rule" | "ir.model.access"))
            else {
                continue;
            };
            reporter.report(
                &XML_ACCESS_RECORDS,
                file,
                at(file, record),
                format!("`{model}` was replaced by `ir.access` in Odoo 20.0"),
            );
        }
    }
}

pub const XML_T_ESC: Rule = Rule {
    code: "U2003",
    name: "upgrade-t-esc-t-raw",
    summary: "`t-esc` or `t-raw`, removed from QWeb in Odoo 20.0.",
    doc: r#"
## What it does

Reports `t-esc` and `t-raw` in templates, reports and views, and
`<attribute name="t-esc">`/`<attribute name="t-raw">` in inheritance.

## Why is this bad?

Odoo 20.0 dropped both directives: in templates and reports they are
unknown and output nothing (a warning in the log), in kanban views the view
fails to install. `t-out` replaces both.

## Fix safety

`t-esc` becomes `t-out`: safe, both escape. `t-raw` becomes `t-out`, an
unsafe fix: `t-out` escapes a value that is not marked safe (`Markup`), so
HTML built as a plain string shows as text.
"#,
    check: Check::Xml(check_t_esc),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_t_esc(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in arch_elements(file) {
            if is(node, "attribute") && matches!(node.attribute("name"), Some("t-esc" | "t-raw")) {
                let Some(name) = node.attributes().find(|a| a.name() == "name") else {
                    continue;
                };
                let old = name.value();
                let edit = Edit::replace(name.range_value().start, name.range_value().end, "t-out");
                reporter
                    .report(
                        &XML_T_ESC,
                        file,
                        at(file, node),
                        format!("`{old}` was removed in Odoo 20.0; use `t-out`"),
                    )
                    .fix = Some(if old == "t-esc" {
                    Fix::safe("Use `t-out`", vec![edit])
                } else {
                    Fix::unsafe_("Use `t-out`", vec![edit])
                });
                continue;
            }
            for attribute in node.attributes().filter(|a| matches!(a.name(), "t-esc" | "t-raw")) {
                let old = attribute.name();
                let fix = node.attribute("t-out").is_none().then(|| {
                    let edits = vec![rename_attribute(&attribute, "t-out")];
                    if old == "t-esc" {
                        Fix::safe("Use `t-out`", edits)
                    } else {
                        Fix::unsafe_("Use `t-out`", edits)
                    }
                });
                reporter
                    .report(
                        &XML_T_ESC,
                        file,
                        at(file, node),
                        format!("`{old}` was removed in Odoo 20.0; use `t-out`"),
                    )
                    .fix = fix;
            }
        }
    }
}

pub const XML_T_CALL_BODY: Rule = Rule {
    code: "U2004",
    name: "upgrade-t-call-body",
    summary: "`t-set` inside a `t-call`, no longer passed to the template in Odoo 20.0.",
    doc: r#"
## What it does

Reports `t-call` elements whose body sets values with `t-set`.

## Why is this bad?

Since Odoo 19.0 a `t-call` takes its parameters as attributes
(`<t t-call="x" title="record.name"/>`); 20.0 no longer passes the `t-set`s
of the body, without an error: the called template renders without them.

## Fix safety

Safe when the body is only `<t t-set="a" t-value="..."/>` elements: they
become attributes of the `t-call`, as Odoo's script `19.1-00-t-call` does.
A `t-set` with a body, other content, or an xpath on the call needs a
person.
"#,
    check: Check::Xml(check_t_call_body),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn t_call_fix(file: &XmlFile, node: roxmltree::Node) -> Option<Fix> {
    let mut attributes = Vec::new();
    for child in node.children() {
        if child.is_text() && child.text().is_some_and(|t| t.trim().is_empty()) {
            continue;
        }
        if child.is_comment() || !is(child, "t") || child.has_children() {
            return None;
        }
        let (Some(name), Some(value)) = (
            child.attribute("t-set"),
            child.attributes().find(|a| a.name() == "t-value"),
        ) else {
            return None;
        };
        if child.attributes().count() != 2 || name.starts_with("t-") || node.attribute(name).is_some() {
            return None;
        }
        let range = value.range_value();
        let quote = &file.source[range.start - 1..range.start];
        attributes.push(format!("{name}={quote}{}{quote}", &file.source[range]));
    }
    let tag_end = start_tag_end(file, node);
    let start = node.range().start;
    let head = file.source[start..tag_end - 1].trim_end();
    if head.ends_with('/') {
        return None;
    }
    Some(Fix::safe(
        "Pass the values as attributes",
        vec![Edit::replace(
            start,
            node.range().end,
            format!("{head} {}/>", attributes.join(" ")),
        )],
    ))
}

fn check_t_call_body(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in arch_elements(file) {
            if node.attribute("t-call").is_none() {
                continue;
            }
            let sets: Vec<roxmltree::Node> = node.children().filter(|c| c.attribute("t-set").is_some()).collect();
            if sets.is_empty() {
                continue;
            }
            let only_sets = node.children().all(|c| {
                c.attribute("t-set").is_some()
                    || c.is_comment()
                    || (c.is_text() && c.text().is_some_and(|t| t.trim().is_empty()))
            });
            if only_sets {
                reporter
                    .report(
                        &XML_T_CALL_BODY,
                        file,
                        at(file, node),
                        "The `t-set`s in a `t-call` body are not passed since Odoo 20.0; pass them as attributes",
                    )
                    .fix = t_call_fix(file, node);
                continue;
            }
            // A body with content: the `t-set`s it uses itself, or that a
            // nested `t-call` may read, are fine. The others were parameters.
            if node.descendants().skip(1).any(|d| d.attribute("t-call").is_some()) {
                continue;
            }
            let body = &file.source[node.range()];
            let parameters: Vec<&str> = sets
                .iter()
                .filter_map(|set| {
                    let name = set.attribute("t-set")?;
                    let pattern = Regex::new(&format!(r"\b{}\b", regex::escape(name))).ok()?;
                    let own = set.range().start - node.range().start..set.range().end - node.range().start;
                    let used = pattern.is_match(&body[..own.start]) || pattern.is_match(&body[own.end..]);
                    (!used).then_some(name)
                })
                .collect();
            if parameters.is_empty() {
                continue;
            }
            reporter.report(
                &XML_T_CALL_BODY,
                file,
                at(file, node),
                format!(
                    "`{}` set in a `t-call` body is not passed since Odoo 20.0; pass it as an attribute",
                    parameters.join("`, `")
                ),
            );
        }
    }
}

pub const XML_T_CALL_OPTIONS: Rule = Rule {
    code: "U2005",
    name: "upgrade-t-call-options",
    summary: "`t-call-options`, removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports the `t-call-options` alias.

## Why is this bad?

Odoo 20.0 removed it: the options are ignored. Use `t-options`.

## Fix safety

Safe: the attribute is renamed.
"#,
    check: Check::Xml(check_t_call_options),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_t_call_options(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in arch_elements(file) {
            let Some(attribute) = node.attributes().find(|a| a.name() == "t-call-options") else {
                continue;
            };
            let fix = node
                .attribute("t-options")
                .is_none()
                .then(|| Fix::safe("Use `t-options`", vec![rename_attribute(&attribute, "t-options")]));
            reporter
                .report(
                    &XML_T_CALL_OPTIONS,
                    file,
                    at(file, node),
                    "`t-call-options` is `t-options` since Odoo 20.0",
                )
                .fix = fix;
        }
    }
}

pub const XML_BASE64_FILE: Rule = Rule {
    code: "U2006",
    name: "upgrade-base64-file-field",
    summary: "`<field type=\"base64\" file=...>`, deprecated for `type=\"bytes\"` in Odoo 20.0.",
    doc: r#"
## What it does

Reports `type="base64"` on `<field>` elements that load a `file`.

## Why is this bad?

Binary fields hold bytes since Odoo 20.0; `type="base64"` is deprecated.

## Fix safety

Safe: the type becomes `bytes`, as Odoo's script `19.3-00-base64-in-xml`
does.
"#,
    check: Check::Xml(check_base64_file),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_base64_file(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for field in data_elements(file, &["field"]) {
            if field.attribute("file").is_none() {
                continue;
            }
            let Some(kind) = field.attributes().find(|a| a.name() == "type" && a.value() == "base64") else {
                continue;
            };
            reporter
                .report(
                    &XML_BASE64_FILE,
                    file,
                    at(file, field),
                    "`type=\"base64\"` is `type=\"bytes\"` since Odoo 20.0",
                )
                .fix = Some(Fix::safe(
                "Use `type=\"bytes\"`",
                vec![Edit::replace(kind.range_value().start, kind.range_value().end, "bytes")],
            ));
        }
    }
}

pub const XML_ATTACHMENT_DATAS: Rule = Rule {
    code: "U2007",
    name: "upgrade-attachment-datas-xml",
    summary: "An `ir.attachment` record sets `datas`, removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports `<field name="datas">` in `ir.attachment` records.

## Why is this bad?

Odoo 20.0 removed `datas`: the value is dropped with a warning and the
attachment is created **empty**. Set `raw`, with `type="bytes"` for a file.
"#,
    check: Check::Xml(check_attachment_datas),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_attachment_datas(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for field in file
            .elements()
            .filter(|f| record_field(*f, &["ir.attachment"], "datas"))
        {
            reporter.report(
                &XML_ATTACHMENT_DATAS,
                file,
                at(file, field),
                "`ir.attachment.datas` was removed in Odoo 20.0 (the attachment is created empty); set `raw`",
            );
        }
    }
}

pub const XML_REPORT_FILE: Rule = Rule {
    code: "U2008",
    name: "upgrade-report-file",
    summary: "A report sets `report_file`, removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports `<field name="report_file">` in `ir.actions.report` records.

## Why is this bad?

Odoo 20.0 removed the field: the data file fails to load ("Invalid field").

## Fix safety

Safe: the field is removed.
"#,
    check: Check::Xml(check_report_file),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_report_file(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for field in file
            .elements()
            .filter(|f| record_field(*f, &["ir.actions.report"], "report_file"))
        {
            reporter
                .report(
                    &XML_REPORT_FILE,
                    file,
                    at(file, field),
                    "`report_file` was removed in Odoo 20.0",
                )
                .fix = Some(Fix::safe("Remove the field", vec![delete_element(file, field)]));
        }
    }
}

pub const XML_FONT_AWESOME: Rule = Rule {
    code: "U2009",
    name: "upgrade-font-awesome",
    summary: "A Font Awesome icon, which Odoo 20.0 no longer loads.",
    doc: r#"
## What it does

Reports `fa`/`fa-*` classes and `icon="fa-..."` in views and templates.

## Why is this bad?

Odoo 20.0 removed Font Awesome: `fa` classes render nothing, and a button
`icon="fa-..."` shows its name as text. Use Odoo's icons instead:
`<i class="oi" data-icon="shopping_cart"/>`, `<button icon="shopping_cart">`.
"#,
    check: Check::Xml(check_font_awesome),
    min_odoo: ODOO_20,
    max_odoo: None,
};

static FA_CLASS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|[\s'])fa(-[a-z0-9-]+)?($|[\s'])").unwrap());

fn check_font_awesome(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in arch_elements(file) {
            let icon = node.attribute("icon").is_some_and(|i| i.starts_with("fa-"));
            let class = node
                .attributes()
                .filter(|a| matches!(a.name(), "class" | "t-attf-class" | "t-att-class"))
                .any(|a| FA_CLASS.is_match(a.value()));
            if icon || class {
                reporter.report(
                    &XML_FONT_AWESOME,
                    file,
                    at(file, node),
                    "Font Awesome is not loaded since Odoo 20.0; use Odoo's icons (`data-icon`)",
                );
            }
        }
    }
}

pub const XML_FILTER_DATE_ATTRIBUTES: Rule = Rule {
    code: "U2010",
    name: "upgrade-filter-date-range",
    summary: "A search filter uses `start_month`/`end_month`/`start_year`/`end_year`, removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports `start_month`, `end_month`, `start_year` and `end_year` on search
view filters.

## Why is this bad?

Odoo 20.0 removed them from the search view schema: the view fails to
install.

## Fix safety

Safe: the attributes are removed, as Odoo's script
`20.0-00-search-date-filters` does.
"#,
    check: Check::Xml(check_filter_date_attributes),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_filter_date_attributes(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for (_, arch) in view_archs(file) {
            for node in arch.descendants().filter(|n| is(*n, "filter")) {
                let edits: Vec<Edit> = node
                    .attributes()
                    .filter(|a| matches!(a.name(), "start_month" | "end_month" | "start_year" | "end_year"))
                    .map(|a| delete_attribute(file, &a))
                    .collect();
                if edits.is_empty() {
                    continue;
                }
                reporter
                    .report(
                        &XML_FILTER_DATE_ATTRIBUTES,
                        file,
                        at(file, node),
                        "Filter date range attributes were removed in Odoo 20.0",
                    )
                    .fix = Some(Fix::safe("Remove the attributes", edits));
            }
        }
    }
}

pub const XML_CALENDAR_DATE_DELAY: Rule = Rule {
    code: "U2011",
    name: "upgrade-calendar-date-delay",
    summary: "A calendar view uses `date_delay`, removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports `date_delay` on `<calendar>` views.

## Why is this bad?

Odoo 20.0 removed it: the view fails to validate. Use `date_stop`, if need
be with a computed end date.
"#,
    check: Check::Xml(check_calendar_date_delay),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_calendar_date_delay(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for (_, arch) in view_archs(file) {
            for node in arch
                .descendants()
                .filter(|n| is(*n, "calendar") && n.attribute("date_delay").is_some())
            {
                reporter.report(
                    &XML_CALENDAR_DATE_DELAY,
                    file,
                    at(file, node),
                    "`date_delay` was removed from calendar views in Odoo 20.0; use `date_stop`",
                );
            }
        }
    }
}

/// `res.partner.bank` fields renamed in 20.0.
fn bank_rename(name: &str) -> Option<&'static str> {
    match name {
        "acc_number" => Some("account_number"),
        "acc_holder_name" => Some("holder_name"),
        "acc_type" => Some("account_type"),
        "sanitized_acc_number" => Some("sanitized_account_number"),
        _ => None,
    }
}

static BANK_FIELD_NAMES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(acc_number|acc_holder_name|acc_type|sanitized_acc_number)\b").unwrap());

pub const XML_BANK_FIELDS: Rule = Rule {
    code: "U2012",
    name: "upgrade-bank-account-fields",
    summary: "A `res.partner.bank` view or record uses a field renamed in Odoo 20.0.",
    doc: r#"
## What it does

Reports `acc_number`, `acc_holder_name`, `acc_type` and
`sanitized_acc_number` in views of `res.partner.bank` (fields and xpaths)
and in its records.

## Why is this bad?

Odoo 20.0 renamed them `account_number`, `holder_name`, `account_type` and
`sanitized_account_number`: the view or data file fails to load.

## Fix safety

Safe: the names are replaced.
"#,
    check: Check::Xml(check_bank_fields),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_bank_fields(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for record in data_elements(file, &["record"]).filter(|r| r.attribute("model") == Some("res.partner.bank")) {
            for field in record.children().filter(|f| is(*f, "field")) {
                let Some(name) = field.attributes().find(|a| a.name() == "name") else {
                    continue;
                };
                let Some(new) = bank_rename(name.value()) else { continue };
                reporter
                    .report(
                        &XML_BANK_FIELDS,
                        file,
                        at(file, field),
                        format!("`{}` is `{new}` since Odoo 20.0", name.value()),
                    )
                    .fix = Some(Fix::safe(
                    format!("Use `{new}`"),
                    vec![Edit::replace(name.range_value().start, name.range_value().end, new)],
                ));
            }
        }
        for (record, arch) in view_archs(file) {
            let model = child_field(record, "model").and_then(|m| m.text()).map(str::trim);
            if model != Some("res.partner.bank") {
                continue;
            }
            for node in arch.descendants().filter(|n| n.is_element()) {
                let attribute = if is(node, "field") {
                    node.attributes()
                        .find(|a| a.name() == "name" && bank_rename(a.value()).is_some())
                } else if is(node, "xpath") {
                    node.attributes()
                        .find(|a| a.name() == "expr" && BANK_FIELD_NAMES.is_match(a.value()))
                } else {
                    None
                };
                let Some(attribute) = attribute else { continue };
                let fix = file.source.get(attribute.range_value()).map(|text| {
                    let new = BANK_FIELD_NAMES.replace_all(text, |c: &regex::Captures| {
                        bank_rename(&c[1]).unwrap_or_default().to_string()
                    });
                    Fix::safe(
                        "Use the new field names",
                        vec![Edit::replace(
                            attribute.range_value().start,
                            attribute.range_value().end,
                            new.into_owned(),
                        )],
                    )
                });
                reporter
                    .report(
                        &XML_BANK_FIELDS,
                        file,
                        at(file, node),
                        "Bank account fields were renamed in Odoo 20.0",
                    )
                    .fix = fix;
            }
        }
    }
}

pub const XML_PARTNER_FIELDS: Rule = Rule {
    code: "U2013",
    name: "upgrade-partner-company-fields",
    summary:
        "A partner or company view uses `company_type`, `company_name` or `company_registry`, removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports `company_type` and `company_name` in `res.partner` views and
`company_registry` in `res.partner` and `res.company` views.

## Why is this bad?

Odoo 20.0 removed them (`is_company` is now computed): the view fails to
install.
"#,
    check: Check::Xml(check_partner_fields),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_partner_fields(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for (record, arch) in view_archs(file) {
            let model = child_field(record, "model")
                .and_then(|m| m.text())
                .map(str::trim)
                .unwrap_or_default();
            let removed: &[&str] = match model {
                "res.partner" => &["company_type", "company_name", "company_registry"],
                "res.company" => &["company_registry"],
                _ => continue,
            };
            for node in arch.descendants().filter(|n| is(*n, "field")) {
                let Some(name) = node.attribute("name").filter(|n| removed.contains(n)) else {
                    continue;
                };
                // Only fields of the view's model, not of a sub-view.
                if node
                    .ancestors()
                    .skip(1)
                    .take_while(|a| *a != arch)
                    .any(|a| is(a, "field"))
                {
                    continue;
                }
                reporter.report(
                    &XML_PARTNER_FIELDS,
                    file,
                    at(file, node),
                    format!("`{model}.{name}` was removed in Odoo 20.0"),
                );
            }
        }
    }
}

pub const XML_WIDGET_RENAMES: Rule = Rule {
    code: "U2014",
    name: "upgrade-widget-renames",
    summary: "`widget=\"remaining_days\"`, renamed `relative_date` in Odoo 20.0.",
    doc: r#"
## What it does

Reports the `remaining_days` widget.

## Why is this bad?

Odoo 20.0 renamed it `relative_date`; an unknown widget falls back to the
default one with a "Missing widget" warning.

## Fix safety

Safe: the widget is renamed.
"#,
    check: Check::Xml(check_widget_renames),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_widget_renames(ctx: &XmlContext, reporter: &mut XmlReporter) {
    for file in ctx.files {
        for node in arch_elements(file) {
            let Some(widget) = node
                .attributes()
                .find(|a| a.name() == "widget" && a.value() == "remaining_days")
            else {
                continue;
            };
            reporter
                .report(
                    &XML_WIDGET_RENAMES,
                    file,
                    at(file, node),
                    "The `remaining_days` widget is `relative_date` since Odoo 20.0",
                )
                .fix = Some(Fix::safe(
                "Use `relative_date`",
                vec![Edit::replace(
                    widget.range_value().start,
                    widget.range_value().end,
                    "relative_date",
                )],
            ));
        }
    }
}

// --- Manifest -------------------------------------------------------------

pub const MANIFEST_ACCESS_CSV: Rule = Rule {
    code: "U2001",
    name: "upgrade-ir-model-access-csv",
    summary: "The manifest loads `ir.model.access.csv`, a model replaced by `ir.access` in Odoo 20.0.",
    doc: r#"
## What it does

Reports `ir.model.access.csv` files in the manifest's `data`.

## Why is this bad?

Odoo 20.0 replaced `ir.model.access` and `ir.rule` by `ir.access`, loaded
from `ir.access.csv` (`id,name,model_id,group_id/id,operation,domain`): the
old file fails to load. Odoo's upgrade script `19.4-00-ir-access` converts
the files; review the result, since a row without a group means a
restriction for everyone.
"#,
    check: Check::Manifest(check_access_csv),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_access_csv(ctx: &ManifestContext, reporter: &mut Reporter) {
    let Some(Expr::List(data)) = ctx.manifest.get("data") else {
        return;
    };
    for item in &data.elts {
        let Some(path) = item.as_string_literal_expr().map(|s| s.value.to_str()) else {
            continue;
        };
        if path.rsplit('/').next() == Some("ir.model.access.csv") {
            reporter.report(
                &MANIFEST_ACCESS_CSV,
                item.start(),
                "`ir.model.access.csv` is replaced by `ir.access.csv` in Odoo 20.0",
            );
        }
    }
}

pub const MANIFEST_JQUERY: Rule = Rule {
    code: "U2015",
    name: "upgrade-jquery-bundle",
    summary: "The manifest includes `web._assets_jquery`, removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports `web._assets_jquery` in the manifest's `assets`.

## Why is this bad?

Odoo 20.0 no longer ships jQuery: the bundle is gone and code using `$`
fails at runtime. Port the code to plain JavaScript or ship the library.
"#,
    check: Check::Manifest(check_jquery),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_jquery(ctx: &ManifestContext, reporter: &mut Reporter) {
    let Some(assets) = ctx.manifest.get("assets") else {
        return;
    };
    walk_strings(assets, &mut |literal, value| {
        if value == "web._assets_jquery" {
            reporter.report(
                &MANIFEST_JQUERY,
                literal.start(),
                "`web._assets_jquery` was removed in Odoo 20.0",
            );
        }
    });
}

/// Every string literal in `expr`.
fn walk_strings<'a>(expr: &'a Expr, f: &mut impl FnMut(&'a Expr, &'a str)) {
    match expr {
        Expr::StringLiteral(s) => f(expr, s.value.to_str()),
        Expr::List(l) => l.elts.iter().for_each(|e| walk_strings(e, f)),
        Expr::Tuple(t) => t.elts.iter().for_each(|e| walk_strings(e, f)),
        Expr::Dict(d) => d.items.iter().for_each(|i| walk_strings(&i.value, f)),
        _ => {}
    }
}

pub const MANIFEST_INIT_XML: Rule = Rule {
    code: "U2016",
    name: "upgrade-manifest-init-xml",
    summary: "The manifest lists files under `init_xml`, ignored since Odoo 20.0.",
    doc: r#"
## What it does

Reports the manifest key `init_xml`.

## Why is this bad?

Odoo 20.0 only loads `data` and `demo`: the files under `init_xml` are
silently never loaded. Move them to `data`.
"#,
    check: Check::Manifest(check_init_xml),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_init_xml(ctx: &ManifestContext, reporter: &mut Reporter) {
    if let Some((key, _)) = ctx.manifest.entry("init_xml") {
        reporter.report(
            &MANIFEST_INIT_XML,
            key.start(),
            "`init_xml` is ignored since Odoo 20.0; move its files to `data`",
        );
    }
}

// --- Python ---------------------------------------------------------------

pub const CONFIG_PARAMETER: Rule = Rule {
    code: "U2017",
    name: "upgrade-config-parameter",
    summary: "`ir.config_parameter` `get_param`/`set_param`, removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports `get_param` and `set_param` on `ir.config_parameter`.

## Why is this bad?

Odoo 20.0 replaced them by typed methods, without a deprecation period:
`get_str`, `get_int`, `get_float`, `get_bool` and `set_str`, `set_int`,
`set_float`, `set_bool`. The old call raises `AttributeError`.

## Fix safety

Unsafe: `get_param(k)` becomes `get_str(k)` (a missing key gives `''`
instead of `False`, both falsy), `int(get_param(k, d))` becomes
`get_int(k, d)` and `float(...)` `get_float(...)`, and `set_param` becomes
`set_str` (which stores `str(value)`; `False`/`None` deletes the key).
"#,
    check: Check::Python(check_config_parameter),
    min_odoo: ODOO_20,
    max_odoo: None,
};

/// Whether a `get_param`/`set_param` call is on `ir.config_parameter`:
/// `env["ir.config_parameter"]...`, or a variable assigned from it in the
/// file (`ICP = env["ir.config_parameter"].sudo()`). Others, such as
/// `email.message.Message.get_param`, are not.
fn is_config_parameter(func: &Expr, variables: &HashSet<String>) -> bool {
    if searched_model(func) == Some("ir.config_parameter") {
        return true;
    }
    let Expr::Attribute(attribute) = func else { return false };
    let mut receiver = &*attribute.value;
    loop {
        receiver = match receiver {
            Expr::Name(name) => return variables.contains(name.id.as_str()),
            Expr::Call(call) => &call.func,
            Expr::Attribute(attribute) => &attribute.value,
            _ => return false,
        };
    }
}

/// Variables assigned from an expression that mentions `ir.config_parameter`.
fn config_parameter_variables(ctx: &PythonContext) -> HashSet<String> {
    let mut variables = HashSet::new();
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Stmt(Stmt::Assign(assign)) = node else { return };
        if !source_of(ctx.source, &*assign.value).contains("ir.config_parameter") {
            return;
        }
        for target in &assign.targets {
            if let Expr::Name(name) = target {
                variables.insert(name.id.to_string());
            }
        }
    });
    variables
}

fn check_config_parameter(ctx: &PythonContext, reporter: &mut Reporter) {
    let variables = config_parameter_variables(ctx);
    let parameter_call = |expr: &Expr| -> Option<(String, String)> {
        let Expr::Call(call) = expr else { return None };
        let Expr::Attribute(attribute) = &*call.func else {
            return None;
        };
        if attribute.attr.as_str() != "get_param" || !is_config_parameter(&call.func, &variables) {
            return None;
        }
        Some((
            source_of(ctx.source, &*attribute.value).to_string(),
            source_of(ctx.source, &call.arguments).to_string(),
        ))
    };
    // `int(...get_param(...))` first, so its inner call is not reported twice.
    let mut wrapped = HashSet::new();
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(outer)) = node else { return };
        let Expr::Name(name) = &*outer.func else { return };
        let typed = match name.id.as_str() {
            "int" => "get_int",
            "float" => "get_float",
            _ => return,
        };
        let [inner] = outer.arguments.args.as_ref() else { return };
        if !outer.arguments.keywords.is_empty() {
            return;
        }
        let Some((receiver, mut arguments)) = parameter_call(inner) else {
            return;
        };
        wrapped.insert(inner.start());
        // A numeric default written as a string becomes a number.
        if let Expr::Call(call) = inner {
            if let [key, Expr::StringLiteral(default)] = call.arguments.args.as_ref() {
                let text = default.value.to_str().trim();
                let numeric = if typed == "get_int" {
                    text.parse::<i64>().is_ok()
                } else {
                    text.parse::<f64>().is_ok()
                };
                if numeric && call.arguments.keywords.is_empty() {
                    arguments = format!("({}, {text})", source_of(ctx.source, key));
                }
            }
        }
        reporter
            .report(
                &CONFIG_PARAMETER,
                inner.start(),
                format!("`get_param` was removed in Odoo 20.0; use `{typed}`"),
            )
            .fix = Some(Fix::unsafe_(format!("Use `{typed}`"), {
            // The `int(...)` parentheses held a multi-line receiver together.
            let call = format!("{receiver}.{typed}{arguments}");
            let call = if receiver.contains('\n') {
                format!("({call})")
            } else {
                call
            };
            vec![rename(outer.range(), &call)]
        }));
    });
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(attribute) = &*call.func else {
            return;
        };
        let new = match attribute.attr.as_str() {
            "get_param" => "get_str",
            "set_param" => "set_str",
            _ => return,
        };
        if wrapped.contains(&call.start()) || !is_config_parameter(&call.func, &variables) {
            return;
        }
        reporter
            .report(
                &CONFIG_PARAMETER,
                call.start(),
                format!(
                    "`{}` was removed in Odoo 20.0; use `{new}` or another typed method",
                    attribute.attr
                ),
            )
            .fix = Some(Fix::unsafe_(
            format!("Use `{new}`"),
            vec![rename(attribute.attr.range(), new)],
        ));
    });
}

pub const ATTACHMENT_DATAS: Rule = Rule {
    code: "U2018",
    name: "upgrade-attachment-datas",
    summary: "`ir.attachment` `datas`, removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports the `"datas"` key in values and the `.datas` attribute.

## Why is this bad?

Odoo 20.0 removed `ir.attachment.datas`: written values are dropped with a
warning, so the attachment is created **empty**, and reading it fails. Use
`raw` (bytes).

## Fix safety

Safe for `"datas": base64.b64encode(content)`, which becomes
`"raw": content`. Others need a person (`att.datas` →
`att.raw.to_base64()` where base64 is really needed).
"#,
    check: Check::Python(check_attachment_datas_python),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_attachment_datas_python(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| match node {
        Node::Expr(Expr::Dict(dict)) => {
            for item in &dict.items {
                let Some(key) = item.key.as_ref() else { continue };
                if key.as_string_literal_expr().is_none_or(|k| k.value.to_str() != "datas") {
                    continue;
                }
                let fix = match &item.value {
                    Expr::Call(call)
                        if matches!(&*call.func, Expr::Attribute(a) if a.attr.as_str() == "b64encode")
                            && call.arguments.args.len() == 1
                            && call.arguments.keywords.is_empty() =>
                    {
                        let quote = source_of(ctx.source, key).chars().next().unwrap_or('"');
                        Some(Fix::safe(
                            "Use `raw`",
                            vec![
                                rename(key.range(), &format!("{quote}raw{quote}")),
                                rename(item.value.range(), source_of(ctx.source, &call.arguments.args[0])),
                            ],
                        ))
                    }
                    _ => None,
                };
                reporter
                    .report(
                        &ATTACHMENT_DATAS,
                        key.start(),
                        "`ir.attachment.datas` was removed in Odoo 20.0; use `raw`",
                    )
                    .fix = fix;
            }
        }
        Node::Expr(Expr::Attribute(attribute)) if attribute.attr.as_str() == "datas" => {
            reporter.report(
                &ATTACHMENT_DATAS,
                attribute.attr.start(),
                "`ir.attachment.datas` was removed in Odoo 20.0; use `raw`",
            );
        }
        _ => {}
    });
}

pub const ACCESS_MODELS: Rule = Rule {
    code: "U2019",
    name: "upgrade-ir-access-models",
    summary: "Python code uses `ir.model.access` or `ir.rule`, replaced by `ir.access` in Odoo 20.0.",
    doc: r#"
## What it does

Reports the model names `ir.model.access` and `ir.rule` in Python.

## Why is this bad?

Odoo 20.0 replaced both by `ir.access`: `env["ir.rule"]` raises
`KeyError`. To check access, use `records.has_access(operation)` and
`check_access(operation)`.
"#,
    check: Check::Python(check_access_models),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_access_models(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(expr @ Expr::StringLiteral(literal)) = node else {
            return;
        };
        let model = literal.value.to_str();
        if matches!(model, "ir.model.access" | "ir.rule") {
            reporter.report(
                &ACCESS_MODELS,
                expr.start(),
                format!("`{model}` was replaced by `ir.access` in Odoo 20.0"),
            );
        }
    });
}

pub const REMOVED_ACCESS_METHODS: Rule = Rule {
    code: "U2020",
    name: "upgrade-removed-access-methods",
    summary: "An access or recursion method removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports calls and overrides of `check_access_rights`, `check_access_rule`,
`_filter_access_rules`, `_filter_access_rules_python`,
`check_field_access_rights`, `_check_recursion`, `_check_m2m_recursion` and
`toggle_active`.

## Why is this bad?

Deprecated in 18.0 and removed in 20.0: calls raise `AttributeError` and
overrides are never called. Use `check_access`/`has_access`/
`_filtered_access`, `check_field_access`, `_has_cycle` and
`action_archive`/`action_unarchive`.

## Fix safety

Safe for `x._check_recursion()` and `x._check_m2m_recursion(field)`, which
become `not x._has_cycle()` and `not x._has_cycle(field)`.
"#,
    check: Check::Python(check_removed_access_methods),
    min_odoo: ODOO_20,
    max_odoo: None,
};

const REMOVED_ACCESS: &[&str] = &[
    "check_access_rights",
    "check_access_rule",
    "_filter_access_rules",
    "_filter_access_rules_python",
    "check_field_access_rights",
    "_check_recursion",
    "_check_m2m_recursion",
    "toggle_active",
];

fn check_removed_access_methods(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        if ctx.semantic.odoo_model_kind(class).is_none() {
            continue;
        }
        for method in methods(class).filter(|m| REMOVED_ACCESS.contains(&m.name.as_str())) {
            reporter.report(
                &REMOVED_ACCESS_METHODS,
                method.name.start(),
                format!(
                    "`{}` was removed in Odoo 20.0; this override is never called",
                    method.name
                ),
            );
        }
    }
    // `not x._check_recursion()` first: it becomes `x._has_cycle()`.
    let mut negated = HashSet::new();
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::UnaryOp(unary)) = node else { return };
        if !matches!(unary.op, ruff_python_ast::UnaryOp::Not) {
            return;
        }
        if let Some((call, edit)) = has_cycle(ctx.source, &unary.operand, unary.range(), false) {
            negated.insert(call.start());
            reporter
                .report(
                    &REMOVED_ACCESS_METHODS,
                    call.start(),
                    format!("`{}()` was removed in Odoo 20.0", func_name(&call.func)),
                )
                .fix = Some(Fix::safe("Use `_has_cycle`", vec![edit]));
        }
    });
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(expr @ Expr::Call(call)) = node else {
            return;
        };
        let Expr::Attribute(attribute) = &*call.func else {
            return;
        };
        let name = attribute.attr.as_str();
        if !REMOVED_ACCESS.contains(&name) || negated.contains(&call.start()) {
            return;
        }
        let fix =
            has_cycle(ctx.source, expr, call.range(), true).map(|(_, edit)| Fix::safe("Use `_has_cycle`", vec![edit]));
        reporter
            .report(
                &REMOVED_ACCESS_METHODS,
                call.start(),
                format!("`{name}()` was removed in Odoo 20.0"),
            )
            .fix = fix;
    });
}

/// `x._check_recursion()` or `x._check_m2m_recursion(field)` as `_has_cycle`,
/// replacing `range`; `negate` because `_has_cycle` is the opposite test.
fn has_cycle<'a>(
    source: &str,
    expr: &'a Expr,
    range: TextRange,
    negate: bool,
) -> Option<(&'a ruff_python_ast::ExprCall, Edit)> {
    let Expr::Call(call) = expr else { return None };
    let Expr::Attribute(attribute) = &*call.func else {
        return None;
    };
    let arguments = &call.arguments;
    let argument = match attribute.attr.as_str() {
        "_check_recursion" if arguments.is_empty() => String::new(),
        "_check_m2m_recursion" if arguments.args.len() == 1 && arguments.keywords.is_empty() => {
            source_of(source, &arguments.args[0]).to_string()
        }
        _ => return None,
    };
    let receiver = source_of(source, &*attribute.value);
    let not = if negate { "not " } else { "" };
    Some((call, rename(range, &format!("{not}{receiver}._has_cycle({argument})"))))
}

pub const READ_GROUP_SIGNATURE: Rule = Rule {
    code: "U2021",
    name: "upgrade-read-group-signature",
    summary: "`read_group` called with the 19.0 signature (`lazy`, `orderby`, `fields`).",
    doc: r#"
## What it does

Reports `read_group(...)` calls with `lazy=`, `orderby=` or `fields=`.

## Why is this bad?

Odoo 20.0 gave `read_group` the signature of `_read_group`:
`read_group(domain, groupby, aggregates, having, offset, limit, order)`,
returning tuples with records instead of dicts. The old call fails or its
result is misread; `formatted_read_group` returns the old dicts.
"#,
    check: Check::Python(check_read_group_signature),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_read_group_signature(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        if !matches!(&*call.func, Expr::Attribute(a) if a.attr.as_str() == "read_group") {
            return;
        }
        let old = call.arguments.keywords.iter().any(|k| {
            k.arg
                .as_ref()
                .is_some_and(|a| matches!(a.as_str(), "lazy" | "orderby" | "fields"))
        });
        if old {
            reporter.report(
                &READ_GROUP_SIGNATURE,
                call.start(),
                "`read_group` takes `(domain, groupby, aggregates, …)` and returns tuples since Odoo 20.0",
            );
        }
    });
}

pub const ORMCACHE_INVALIDATION: Rule = Rule {
    code: "U2022",
    name: "upgrade-registry-clear-cache",
    summary: "`registry.clear_cache()`, removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports `registry.clear_cache(...)`, `pool.clear_cache(...)` and
`clear_all_caches()`.

## Why is this bad?

Odoo 20.0 moved the ormcache to the transaction: the call raises
`AttributeError`. Use `env.transaction.invalidate_ormcache(name)`.

## Fix safety

Safe with at most one cache name, as Odoo's script
`19.4-00-ormcache-on-transaction` does: `x.env.registry.clear_cache(n)`
becomes `x.env.transaction.invalidate_ormcache(n)`, and
`x.pool.clear_cache(n)` `x.env.transaction.invalidate_ormcache(n)`.
Several names need one call each.
"#,
    check: Check::Python(check_ormcache_invalidation),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_ormcache_invalidation(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Call(call)) = node else { return };
        let Expr::Attribute(method) = &*call.func else { return };
        let Expr::Attribute(holder) = &*method.value else {
            return;
        };
        let holder_name = holder.attr.as_str();
        if !matches!(holder_name, "registry" | "pool")
            || !matches!(method.attr.as_str(), "clear_cache" | "clear_all_caches")
        {
            return;
        }
        let one_name = call.arguments.keywords.is_empty() && call.arguments.args.len() <= 1;
        let via_env = holder_name == "pool" || matches!(&*holder.value, Expr::Attribute(a) if a.attr.as_str() == "env");
        let fix = (method.attr.as_str() == "clear_cache" && one_name && via_env).then(|| {
            let new = if holder_name == "pool" {
                "env.transaction.invalidate_ormcache"
            } else {
                "transaction.invalidate_ormcache"
            };
            let range = TextRange::new(holder.attr.start(), method.attr.end());
            Fix::safe("Use `transaction.invalidate_ormcache`", vec![rename(range, new)])
        });
        reporter
            .report(
                &ORMCACHE_INVALIDATION,
                call.start(),
                format!(
                    "`{holder_name}.{}()` was removed in Odoo 20.0; use `env.transaction.invalidate_ormcache()`",
                    method.attr
                ),
            )
            .fix = fix;
    });
}

pub const ORMCACHE_IMPORT: Rule = Rule {
    code: "U2023",
    name: "upgrade-ormcache-import",
    summary: "`ormcache` imported from `odoo.tools`, deprecated in Odoo 20.0.",
    doc: r#"
## What it does

Reports `ormcache` imported from `odoo.tools` or `odoo.tools.cache`.

## Why is this bad?

Odoo 20.0 moved it to `odoo.api` (`@api.ormcache`); the old import warns.

## Fix safety

Safe: the name is imported from `odoo.api`.
"#,
    check: Check::Python(check_ormcache_import),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_ormcache_import(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Stmt(Stmt::ImportFrom(import)) = node else {
            return;
        };
        let Some(module) = import_module(import).filter(|m| matches!(*m, "odoo.tools" | "odoo.tools.cache")) else {
            return;
        };
        if !import.names.iter().any(|a| a.name.as_str() == "ormcache") {
            return;
        }
        reporter
            .report(
                &ORMCACHE_IMPORT,
                import.start(),
                "Import `ormcache` from `odoo.api` since Odoo 20.0",
            )
            .fix = split_import(ctx.source, import, module, |name| {
            (name == "ormcache").then_some("odoo.api")
        })
        .map(|edit| Fix::safe("Import from `odoo.api`", vec![edit]));
    });
}

/// Where the names `odoo.http` no longer exports went in 20.0.
fn http_move(name: &str) -> Option<&'static str> {
    Some(match name {
        "content_disposition" | "Stream" | "STATIC_CACHE" | "STATIC_CACHE_LONG" => "odoo.http.stream",
        "serialize_exception" => "odoo.http.dispatcher",
        "Session" | "SessionExpiredException" | "get_default_session" => "odoo.http.session",
        "db_list" | "db_filter" | "dispatch_rpc" | "root" | "Application" => "odoo.http.router",
        "Request" => "odoo.http.requestlib",
        "GeoIP" => "odoo.http.geoip",
        _ => return None,
    })
}

pub const HTTP_IMPORTS: Rule = Rule {
    code: "U2024",
    name: "upgrade-http-imports",
    summary: "A name imported from `odoo.http` that moved to a submodule in Odoo 20.0.",
    doc: r#"
## What it does

Reports `from odoo.http import ...` of names that moved: `content_disposition`
and `Stream` to `odoo.http.stream`, `serialize_exception` to
`odoo.http.dispatcher`, `Session` to `odoo.http.session`, `db_list`,
`db_filter`, `dispatch_rpc` and `root` to `odoo.http.router`, `Request` to
`odoo.http.requestlib`, `GeoIP` to `odoo.http.geoip`.

## Why is this bad?

`odoo.http` became a package in Odoo 20.0 that exports only `request`,
`Response`, `Controller` and `route`: the import fails.

## Fix safety

Safe: the moved names are imported from their new module.
"#,
    check: Check::Python(check_http_imports),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_http_imports(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Stmt(Stmt::ImportFrom(import)) = node else {
            return;
        };
        if import_module(import) != Some("odoo.http")
            || !import.names.iter().any(|a| http_move(a.name.as_str()).is_some())
        {
            return;
        }
        reporter
            .report(
                &HTTP_IMPORTS,
                import.start(),
                "Names moved out of `odoo.http` in Odoo 20.0",
            )
            .fix = split_import(ctx.source, import, "odoo.http", http_move)
            .map(|edit| Fix::safe("Import from the new modules", vec![edit]));
    });
}

/// Where names `odoo.tools` no longer exports went in 20.0.
fn tools_move(name: &str) -> Option<&'static str> {
    Some(match name {
        "Query" => "odoo.orm.query",
        "create_index" | "index_exists" | "make_index_name" | "escape_psql" | "reverse_order" | "make_identifier" => {
            "odoo.tools.sql"
        }
        "mod10r" | "street_split" => "odoo.tools.business_data",
        "convert_xml_import" | "convert_csv_import" => "odoo.tools.convert",
        _ => return None,
    })
}

const TOOLS_REMOVED: &[&str] = &["pycompat", "populate", "ustr", "get_encodings"];

pub const TOOLS_IMPORTS: Rule = Rule {
    code: "U2025",
    name: "upgrade-tools-imports",
    summary: "A name imported from `odoo.tools` that moved or was removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports imports from `odoo.tools` of names that moved (`Query` to
`odoo.orm.query`, the index and SQL helpers to `odoo.tools.sql`, `mod10r` and
`street_split` to `odoo.tools.business_data`, `convert_xml_import` and
`convert_csv_import` to `odoo.tools.convert`) or were removed (`pycompat`,
`populate`, `ustr`, `get_encodings`), and imports from the removed modules
`odoo.tools.query`, `odoo.tools.pycompat` and `odoo.tools.populate`.

## Why is this bad?

The import fails on Odoo 20.0.

## Fix safety

Safe for moved names: they are imported from their new module.
`odoo.tools.query` becomes `odoo.orm.query`. Removed names need a person.
"#,
    check: Check::Python(check_tools_imports),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_tools_imports(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Stmt(Stmt::ImportFrom(import)) = node else {
            return;
        };
        let Some(module) = import_module(import) else { return };
        match module {
            "odoo.tools" => {
                let removed = import.names.iter().any(|a| TOOLS_REMOVED.contains(&a.name.as_str()));
                if !removed && !import.names.iter().any(|a| tools_move(a.name.as_str()).is_some()) {
                    return;
                }
                let fix = (!removed)
                    .then(|| split_import(ctx.source, import, "odoo.tools", tools_move))
                    .flatten()
                    .map(|edit| Fix::safe("Import from the new modules", vec![edit]));
                reporter
                    .report(
                        &TOOLS_IMPORTS,
                        import.start(),
                        "Names moved or removed from `odoo.tools` in Odoo 20.0",
                    )
                    .fix = fix;
            }
            "odoo.tools.query" => {
                let range = import.module.as_ref().expect("has a module").range();
                reporter
                    .report(
                        &TOOLS_IMPORTS,
                        import.start(),
                        "`odoo.tools.query` is `odoo.orm.query` since Odoo 20.0",
                    )
                    .fix = Some(Fix::safe(
                    "Import from `odoo.orm.query`",
                    vec![rename(range, "odoo.orm.query")],
                ));
            }
            "odoo.tools.pycompat" | "odoo.tools.populate" => {
                reporter.report(
                    &TOOLS_IMPORTS,
                    import.start(),
                    format!("`{module}` was removed in Odoo 20.0"),
                );
            }
            _ => {}
        }
    });
}

pub const BANK_FIELDS: Rule = Rule {
    code: "U2026",
    name: "upgrade-bank-account-fields-python",
    summary: "Python code uses a `res.partner.bank` field renamed in Odoo 20.0.",
    doc: r#"
## What it does

Reports `acc_number`, `acc_holder_name`, `acc_type` and
`sanitized_acc_number` as attributes and string literals, and calls to
`retrieve_acc_type`.

## Why is this bad?

Odoo 20.0 renamed them `account_number`, `holder_name`, `account_type`,
`sanitized_account_number` and `retrieve_account_type`, without aliases.

## Fix safety

Unsafe: the names are replaced, which is wrong for another model with a
field of the same name.
"#,
    check: Check::Python(check_bank_fields_python),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_bank_fields_python(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(expr) = node else { return };
        let (old, range, new) = match expr {
            Expr::Attribute(attribute) => {
                let old = attribute.attr.as_str();
                let new = if old == "retrieve_acc_type" {
                    Some("retrieve_account_type")
                } else {
                    bank_rename(old)
                };
                let Some(new) = new else { return };
                (old.to_string(), attribute.attr.range(), new.to_string())
            }
            Expr::StringLiteral(literal) if !literal.value.is_implicit_concatenated() => {
                let old = literal.value.to_str();
                let Some(new) = bank_rename(old) else { return };
                let text = source_of(ctx.source, expr);
                (old.to_string(), expr.range(), text.replacen(old, new, 1))
            }
            _ => return,
        };
        let shown = new.trim_matches(['"', '\'']).to_string();
        reporter
            .report(
                &BANK_FIELDS,
                range.start(),
                format!("`{old}` is `{shown}` since Odoo 20.0"),
            )
            .fix = Some(Fix::unsafe_(format!("Use `{shown}`"), vec![rename(range, &new)]));
    });
}

pub const PARTNER_FIELDS: Rule = Rule {
    code: "U2027",
    name: "upgrade-partner-company-fields-python",
    summary: "Python code uses `company_type`, `company_registry` or `create_company`, removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports `company_type` and `company_registry` as attributes and string
literals, and calls to `create_company()`.

## Why is this bad?

Odoo 20.0 removed them from partners (and `company_registry` from
companies): `is_company` is computed from the VAT and the commercial
partner. Reads and writes fail.
"#,
    check: Check::Python(check_partner_fields_python),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_partner_fields_python(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(expr) = node else { return };
        let name = match expr {
            Expr::Attribute(attribute)
                if matches!(
                    attribute.attr.as_str(),
                    "company_type" | "company_registry" | "create_company"
                ) =>
            {
                attribute.attr.as_str()
            }
            Expr::StringLiteral(literal) if matches!(literal.value.to_str(), "company_type" | "company_registry") => {
                literal.value.to_str()
            }
            _ => return,
        };
        reporter.report(
            &PARTNER_FIELDS,
            expr.start(),
            format!("`{name}` was removed in Odoo 20.0"),
        );
    });
}

pub const SELF_FIELDS: Rule = Rule {
    code: "U2028",
    name: "upgrade-self-writeable-fields",
    summary: "`SELF_READABLE_FIELDS`/`SELF_WRITEABLE_FIELDS`, replaced in Odoo 20.0.",
    doc: r#"
## What it does

Reports `SELF_READABLE_FIELDS` and `SELF_WRITEABLE_FIELDS` overrides.

## Why is this bad?

Odoo 20.0 replaced the properties by the field parameter
`user_writeable=True`: the override is never read, so users can no longer
change their own preferences in the new fields.
"#,
    check: Check::Python(check_self_fields),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_self_fields(ctx: &PythonContext, reporter: &mut Reporter) {
    for class in classes(ctx.parsed.suite()) {
        for method in
            methods(class).filter(|m| matches!(m.name.as_str(), "SELF_READABLE_FIELDS" | "SELF_WRITEABLE_FIELDS"))
        {
            reporter.report(
                &SELF_FIELDS,
                method.name.start(),
                format!(
                    "`{}` is not read since Odoo 20.0; set `user_writeable=True` on the fields",
                    method.name
                ),
            );
        }
    }
}

pub const INHERIT_READ: Rule = Rule {
    code: "U2029",
    name: "upgrade-inherit-read",
    summary: "Code reads `._inherit` at runtime, which raises in Odoo 20.0.",
    doc: r#"
## What it does

Reports reads of `x._inherit`, such as `"mail.thread" in self._inherit`.

## Why is this bad?

Odoo 20.0 raises `AttributeError` on runtime reads of `_inherit` (the
class attribute in a class body is fine). Test the class instead:
`isinstance(self, self.env.registry["mail.thread"])`.
"#,
    check: Check::Python(check_inherit_read),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_inherit_read(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| {
        let Node::Expr(Expr::Attribute(attribute)) = node else {
            return;
        };
        if attribute.attr.as_str() == "_inherit" && attribute.ctx.is_load() {
            reporter.report(
                &INHERIT_READ,
                attribute.start(),
                "Reading `_inherit` raises since Odoo 20.0; use `isinstance`",
            );
        }
    });
}

pub const TEST_CLASSES: Rule = Rule {
    code: "U2030",
    name: "upgrade-test-classes",
    summary: "`SingleTransactionCase` or `odoo.tests.common.Form`, removed in Odoo 20.0.",
    doc: r#"
## What it does

Reports `SingleTransactionCase` and `Form` imported from
`odoo.tests.common`.

## Why is this bad?

Odoo 20.0 removed `SingleTransactionCase` (use `TransactionCase`, with
`setUpClass` for shared data) and no longer exports `Form` from
`odoo.tests.common`.

## Fix safety

Safe for `Form`: it is imported from `odoo.tests`.
"#,
    check: Check::Python(check_test_classes),
    min_odoo: ODOO_20,
    max_odoo: None,
};

fn check_test_classes(ctx: &PythonContext, reporter: &mut Reporter) {
    walk(ctx.parsed.suite(), |node, _| match node {
        Node::Stmt(Stmt::ImportFrom(import)) if import_module(import) == Some("odoo.tests.common") => {
            if import.names.iter().any(|a| a.name.as_str() == "Form") {
                reporter
                    .report(
                        &TEST_CLASSES,
                        import.start(),
                        "Import `Form` from `odoo.tests` since Odoo 20.0",
                    )
                    .fix = split_import(ctx.source, import, "odoo.tests.common", |n| {
                    (n == "Form").then_some("odoo.tests")
                })
                .map(|edit| Fix::safe("Import from `odoo.tests`", vec![edit]));
            }
        }
        Node::Expr(Expr::Name(name)) if name.id.as_str() == "SingleTransactionCase" => {
            reporter.report(
                &TEST_CLASSES,
                name.start(),
                "`SingleTransactionCase` was removed in Odoo 20.0; use `TransactionCase`",
            );
        }
        Node::Expr(Expr::Attribute(attribute)) if attribute.attr.as_str() == "SingleTransactionCase" => {
            reporter.report(
                &TEST_CLASSES,
                attribute.start(),
                "`SingleTransactionCase` was removed in Odoo 20.0; use `TransactionCase`",
            );
        }
        _ => {}
    });
}
