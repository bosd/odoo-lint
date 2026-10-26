# Roadmap

Ideas that are planned but not built yet. Suggestions are welcome as
[issues](https://github.com/bosd/odoo-lint/issues).

## Editor extensions

`odl server` works in every editor with a language server client; VS Code
and Zed have extensions. PyCharm (through LSP4IJ) needs a small extension of
its own to start it.

## Translation templates

`odl pot` writes a module's `i18n/<module>.pot` from its source, the way
Odoo's export writes it, without a database. It is in the binary but hidden
from `odl --help` and not documented yet; `scripts/pot_parity.py` compares
its output with the templates OCA exports from Odoo.

## Ferris' clean sweep

An opt-in `odl check --crab`: when a codebase is clean, the crab rolls its
lint roller over the terminal. Only on a terminal, never in CI or JSON
output.
