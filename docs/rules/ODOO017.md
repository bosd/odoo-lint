<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# owl-template-reference-not-found (ODOO017)

An OWL template inherits from or calls a template no module it can see defines.

Applies to Odoo 16.0 and later.

## What it does

Reports `t-inherit="name"` and `t-call="name"` in the OWL templates of the
module's assets when no module it can see defines a template `name`.

## Why is this bad?

An extension of a missing template fails to load the assets bundle; a
`t-call` of one fails where it renders. Usually the parent template was
renamed in a newer Odoo, or a dependency is missing from the manifest.
