<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# button-method-not-found (ODOO011)

A view's `<button type="object">` calls a method its model does not have.

## What it does

Reports `<button type="object" name="...">` in views whose method the
view's model does not have, as the modules the module's `depends` reach
define it. Buttons in an embedded list or form are checked against that
field's comodel.

## Why is this bad?

Odoo installs the view, but the button fails when clicked: "The method ...
does not exist on the model ...". It typically happens after a method was
renamed, or when it comes from a module that is not in `depends`.

## Configuration

Like [ODOO004](ODOO004.md), the check needs the code of the dependencies,
Odoo's included, in `addons-path`. A model with methods odoo-lint cannot
see is not checked.
