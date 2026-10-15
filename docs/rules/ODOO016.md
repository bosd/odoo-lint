<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# owl-template-not-found (ODOO016)

A component's `static template` names an OWL template no module it can see defines.

Applies to Odoo 16.0 and later.

## What it does

Reports `static template = "name"` in a component class when no `t-name`
in the `static/src` templates of the module or its dependencies defines
that name.

## Why is this bad?

OWL fails to render the component: "Missing template: ...". Usually a
typo, a template that was renamed, or a dependency missing from the
manifest.

## Limitations

Runs with an addons path that includes Odoo's sources with their `static`
folders, or in an Odoo source tree.
