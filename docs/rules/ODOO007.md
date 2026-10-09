<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# python-field-not-found (ODOO007)

Python code names a field the model does not have.

## What it does

Checks the field names and paths (`partner_id.country_id.code`) in model
classes against the models of the addons path, following relational fields
to their comodel:

- `@api.depends`, `@api.onchange` and `@api.constrains`;
- `related=`, `currency_field=` and the inverse field of a `One2many`;
- the field names of a field's `domain` (on its comodel);
- `_rec_name` and `_order`;
- `mapped()`, `filtered()` and `sorted()` with a string on `self`.

## Why is this bad?

Odoo refuses most of these when it loads the model ("Field x does not
exist", or an invalid `related` or `_order`), and the rest fail when the
code runs. A field defined in a module outside `depends` works only on a
database where that module happens to be installed: the message names it.

## Configuration

Like [ODOO004](ODOO004.md), the check needs the dependencies, Odoo's
included, in `addons-path`; a module whose dependencies cannot all be found
is not checked, and a path through a model the index does not know stops
there without a report.
