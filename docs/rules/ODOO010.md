<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# field-method-not-found (ODOO010)

A field names a method (`compute=`, `inverse=`, ...) that its model does not have.

## What it does

Reports the methods named by string in a field definition, `compute`,
`inverse`, `search`, `group_expand` and a `selection` method, that the
model does not have, as the modules the module's `depends` reach define it,
Odoo's own model methods included.

## Why is this bad?

The field fails as soon as Odoo computes, writes or searches it: an
`AttributeError` at install for a stored computed field with records, else
when it is used. It typically happens after a method was renamed or moved
to a module that is not in `depends`, or when a field was copied from
another model without its method.

## Configuration

Like [ODOO004](ODOO004.md), the check needs the code of the dependencies,
Odoo's included, in `addons-path`. A model with methods odoo-lint cannot
see (a Python mixin outside the addons, a parent outside the closure) is not
checked, nor is an abstract model: it may leave the methods to the models
that inherit it.
