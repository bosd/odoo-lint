# Roadmap

Ideas that are planned but not built yet. Suggestions are welcome as
[issues](https://github.com/bosd/odoo-lint/issues).

## XPaths against the views they inherit

An `<xpath>` that matches in 17.0 can match nothing in 18.0 when Odoo
changes the view it inherits. Checking that needs the views of the target
version, from an Odoo source checkout (`--odoo-src`), and an XPath engine.

## A model of the whole addons path

The index of models, fields and XML ids across the addons path is there
(see `addons-path`), with checks of fields in views (ODOO004), XML ids
(ODOO005) and models (ODOO006). Next on top of it: go to definition for
models, fields and XML ids in the language server, and fields used in
Python (`depends`, domains, `mapped`) that do not exist.

## Editor extensions

`odl server` works in every editor with a language server client; Zed and
VS Code have extensions. PyCharm (through LSP4IJ) needs a small extension of
its own to start it.

## Ferris' clean sweep

An opt-in `odl check --crab`: when a codebase is clean, the crab rolls its
lint roller over the terminal. Only on a terminal, never in CI or JSON
output.
