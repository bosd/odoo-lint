# Roadmap

Ideas that are planned but not built yet. Suggestions are welcome as
[issues](https://github.com/bosd/odoo-lint/issues).

## Editor extensions

`odl server` works in every editor with a language server client; VS Code
and Zed have extensions. PyCharm (through LSP4IJ) needs a small extension of
its own to start it.

## Ferris' clean sweep

An opt-in `odl check --crab`: when a codebase is clean, the crab rolls its
lint roller over the terminal. Only on a terminal, never in CI or JSON
output.
