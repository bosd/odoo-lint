# Roadmap

Ideas that are planned but not built yet. Suggestions are welcome as
[issues](https://github.com/bosd/odoo-lint/issues).

## A model of the whole addons path

The index of models, fields and XML ids across the addons path is there
(see `addons-path`), with its checks (ODOO004-ODOO009) and go to definition
in the language server. Next on top of it: find references (where a field
or an XML id is used) and completion of model, field and XML id names.

## Editor extensions

`odl server` works in every editor with a language server client; VS Code
and Zed have extensions. PyCharm (through LSP4IJ) needs a small extension of
its own to start it.

## Ferris' clean sweep

An opt-in `odl check --crab`: when a codebase is clean, the crab rolls its
lint roller over the terminal. Only on a terminal, never in CI or JSON
output.
