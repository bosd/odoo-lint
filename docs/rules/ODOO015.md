<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# view-widget-not-found (ODOO015)

A view uses a `widget` that no JavaScript of the module or its dependencies registers.

Applies to Odoo 16.0 and later.

## What it does

Reports `<field widget="...">` and `<widget name="...">` in views when no
module the view's module can see registers that name in the `fields` or
`view_widgets` registry of its JavaScript
(`registry.category("fields").add("name", ...)`, also for one view type,
`"list.name"`).

## Why is this bad?

The view loads, but the web client fails where it renders the field:
"Missing widget: ..." for every user who opens it. Usually a dependency is
missing from the manifest, or the widget was renamed in a newer Odoo.

## Limitations

Runs with an [addons path](../configuration.md#addons-path) that includes
Odoo's sources with their `static` folders, or in an Odoo source tree.
Search views are skipped: their `widget` names are not registry keys.
