# Roadmap

Ideas that are planned but not built yet. Suggestions are welcome as
[issues](https://github.com/bosd/odoo-lint/issues).

## XPaths against the views they inherit

An `<xpath>` that matches in 17.0 can match nothing in 18.0 when Odoo
changes the view it inherits. Checking that needs the views of the target
version, from an Odoo source checkout (`--odoo-src`), and an XPath engine.

## A model of the whole addons path

The index of models and fields across the addons path is there (see
`addons-path`), with fields in views as its first check (ODOO004). Next on
top of it: XML ids (`ref()`, `inherit_id`, `t-call`) that do not exist or
come from a module outside `depends`, models used without the module that
defines them, and go to definition for models, fields and XML ids in the
language server.

## Editor extensions

`odl server` works in every editor with a language server client; Zed has
an extension. VS Code and PyCharm (through LSP4IJ) need small extensions of
their own to start it.

## Ferris' clean sweep

An opt-in `odl check --crab`: when a codebase is clean, the crab rolls its
lint roller over the terminal. Only on a terminal, never in CI or JSON
output.
