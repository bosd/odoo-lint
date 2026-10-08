<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# view-field-not-found (ODOO004)

A view uses a field that does not exist on its model.

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
