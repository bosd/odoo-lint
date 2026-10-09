<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# xml-id-not-found (ODOO005)

A reference to an XML id that does not exist, comes from outside `depends`, or is loaded later.

## What it does

Checks the XML ids a module refers to: `ref="..."`, `parent` and `action`
of menus, `inherit_id`, `groups`, `t-call`, `%(...)d`, `ref('...')` in
`eval`, the `:id` columns of CSV files, records that override another
module's (`id="base.main_company"`), and `env.ref()` and `has_group()` in
Python.

It reports an id that does not exist, one from a module that `depends` does
not reach, and one of the module itself that is defined in a later data
file (or further down, or only in demo data).

## Why is this bad?

Odoo resolves the reference while it loads the record: the module fails to
install with "External ID not found". Outside `depends`, it works only on a
database where the other module happens to be installed. In Python, the
error comes when the code runs.

## Configuration

Like [ODOO004](ODOO004.md), the check needs the dependencies, Odoo's
included, in `addons-path`; a module whose dependencies cannot all be found
is not checked. `env.ref(..., raise_if_not_found=False)` is not reported.
