<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# view-spec-not-found (ODOO008)

An inheritance spec (`<xpath>`, `<field position=...>`) finds nothing in the view it extends.

## What it does

Builds the view a view inherits from as Odoo does: the base view, then the
extensions of the modules `depends` reaches, in Odoo's order (priority,
then load order). It then applies the view's own specs (`<xpath>`, or an
element such as `<field name="..." position="after">`) one by one, and
reports the first one that matches nothing.

## Why is this bad?

Odoo refuses the view: the module fails to install or update with "Element
... cannot be located in parent view". It typically happens after an
upgrade, when the parent view changed, or when the element comes from a
module that is not in `depends`.

## Limits

XPath outside the subset odoo-lint evaluates (`ancestor::` and other axes,
some functions) is not checked, nor anything after it in the same view. Like
[ODOO004](ODOO004.md), the check needs the dependencies, Odoo's included,
in `addons-path`.
