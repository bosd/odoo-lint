<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# model-not-found (ODOO006)

A model that no module of `depends` defines.

## What it does

Reports models that no module the module's `depends` reach defines (with
`_name`): `_inherit` and `_inherits` parents in Python, and the models of
records, views, actions and `<function>`s in data files.

## Why is this bad?

Odoo refuses to load the module ("Model ... does not exist"). When another
module of the addons path defines the model, the message names it: a
missing dependency.

## Configuration

Like [ODOO004](ODOO004.md), the check needs the dependencies in
`addons-path`.
