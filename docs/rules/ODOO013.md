<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# view-field-restricted-comodel (ODOO013)

A view shows a one2many or many2many field whose comodel only administrators may read, without `groups`.

## What it does

Reports a one2many or many2many field in a view when every internal user
may read the view's model, but only administrators may read the field's
comodel: every
access right (`ir.model.access`) that grants reading it is for a group that
implies Access Rights (`base.group_erp_manager`), such as Settings. A
typical case is a many2many to `ir.model.fields` shown as tags.

The field is left alone when it is restricted with `groups`: on the field in
the view, on an element around it, or in its definition in Python.

## Why is this bad?

The web client reads the comodel's records (their `display_name`, or the
columns of the embedded list) as the user who opens the view. Any user outside the privileged group gets "Access
Error: You are not allowed to access ..." on opening the record. Nothing
shows it earlier: the module installs, and the administrator who tests it
may read everything.

`invisible` does not help: the field is still read. Only `groups` takes it
out of the view for those users.

## Configuration

The access rights come from the modules `depends` reaches, so a module that
tightens them is taken into account; the check needs the dependencies,
Odoo's included, in `addons-path`. Models a project restricts in another
way can be added:

```toml
[tool.odoo-lint.rules.view-field-restricted-comodel]
models = ["ir.config_parameter"]
```

## Limits

A many2one is not reported: Odoo reads its `display_name` as superuser.
A field with `groups` counts as restricted, whichever groups they are.
