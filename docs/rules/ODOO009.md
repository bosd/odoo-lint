<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# record-field-not-found (ODOO009)

A data record sets a field that does not exist on its model.

## What it does

Reports `<field name="...">` in the `<record>`s of data and demo files, and
the columns of CSV data files, whose field the model does not have, as the
modules the module's `depends` reach define it.

## Why is this bad?

Odoo refuses the record: the module fails to install or update ("Invalid
field 'x' on model 'y'"). It typically happens after an upgrade, when Odoo
removed or renamed a field (`numbercall` on `ir.cron` in 18.0, `category_id`
on `res.groups` in 19.0), or when the field comes from a module that is not
in `depends`.

## Configuration

Like [ODOO004](ODOO004.md), the check needs the code of the dependencies,
Odoo's included, in `addons-path`; a module whose dependencies cannot all be
found is not checked. Fields whose name starts with `x_` (custom fields) are
not checked.
