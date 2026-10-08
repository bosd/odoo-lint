# Roadmap

Ideas that are planned but not built yet. Suggestions are welcome as
[issues](https://github.com/bosd/odoo-lint/issues).

## Upgrade rules for every version step

`odl upgrade-check` reports what the existing rules know about versions.
Next are rules for the breaking changes of each step from 16.0 to 20.0,
each with a fix where one is possible: `attrs` and `states` in 17.0,
`<tree>` to `<list>` and `group_operator` to `aggregator` in 18.0,
`_sql_constraints` and JSON-RPC routes in 19.0, and so on.

## XPaths against the views they inherit

An `<xpath>` that matches in 17.0 can match nothing in 18.0 when Odoo
changes the view it inherits. Checking that needs the views of the target
version, from an Odoo source checkout (`--odoo-src`), and an XPath engine.

## A model of the whole addons path

An index of the models, fields, XML ids and views of Odoo and all addons,
built in a fraction of a second. On top of it: views that use fields that do
not exist, `depends` that are missing, go to definition for XML ids and
models in the language server.

## Editor extensions

`odl server` works in every editor with a language server client; Zed has
an extension. VS Code and PyCharm (through LSP4IJ) need small extensions of
their own to start it.

## Ferris' clean sweep

An opt-in `odl check --crab`: when a codebase is clean, the crab rolls its
lint roller over the terminal. Only on a terminal, never in CI or JSON
output.

## Badges

A README badge for the share of clean modules, and one for whether a
repository is ready for the next Odoo version.
