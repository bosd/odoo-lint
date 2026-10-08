# Editors

`odl server` is a language server: editors show odoo-lint's findings while
you type, with a link to each rule's page, offer its fixes as quick fixes,
and a _Fix all_ source action (`source.fixAll.odoo-lint`) for the safe
ones. Unsaved changes are linted as they are; files outside an Odoo addon
are left alone, so the server can run for every Python project.

It needs `odl` on PATH, or in the project's virtual environment:
`uv tool install odoo-linter`.

## Zed

The extension in
[`integrations/zed`](https://github.com/bosd/odoo-lint/tree/main/integrations/zed)
starts `odl server` for Python files. It uses, in order, the binary set in
the settings, `odl` on the project's PATH, or the latest release from PyPI,
which it downloads.

Until it is in Zed's extension registry, install it from a checkout of this
repository: run _zed: install dev extension_ from the command palette and
pick the `integrations/zed` folder (this needs Rust installed through
rustup).

To use another binary:

```json
{
  "lsp": {
    "odoo-lint": {
      "binary": { "path": "/path/to/odl", "arguments": ["server"] }
    }
  }
}
```

## Neovim

Neovim 0.11 and later:

```lua
vim.lsp.config("odoo_lint", {
  cmd = { "odl", "server" },
  filetypes = { "python", "po" },
  root_markers = { "pyproject.toml", "odoo-lint.toml", ".git" },
})
vim.lsp.enable("odoo_lint")
```

## Helix

In `languages.toml`:

```toml
[language-server.odoo-lint]
command = "odl"
args = ["server"]

[[language]]
name = "python"
language-servers = ["pyright", "odoo-lint"]
```

Keep the servers you already use in the list.

## OpenCode

OpenCode feeds diagnostics back to the model. In `opencode.json`:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "lsp": {
    "odoo-lint": {
      "command": ["odl", "server"],
      "extensions": [".py", ".po", ".pot"]
    }
  }
}
```

## Other editors

Any editor with a generic LSP client can run `odl server` over stdin and
stdout. It supports full document sync, `textDocument/publishDiagnostics`
and `textDocument/codeAction`, and the UTF-8, UTF-16 and UTF-32 position
encodings.
