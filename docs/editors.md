# Editors

`odl server` is a language server: editors show odoo-lint's findings while
you type, with a link to each rule's page, offer its fixes as quick fixes,
and a _Fix all_ source action (`source.fixAll.odoo-lint`) for the safe
ones. Unsaved changes are linted as they are; files outside an Odoo addon
are left alone, so the server can run for every Python project.

It needs `odl` on PATH, or in the project's virtual environment:
`uv tool install odoo-linter`.

## Go to definition

_Go to definition_ jumps from what a module refers to, to where it is
defined:

- **XML ids**: `ref="..."`, `inherit_id`, the `parent` and `action` of
  menus, `groups`, `t-call`, `%(...)d` and `ref('...')` in XML, and
  `env.ref("...")` and `has_group("...")` in Python, to the record;
- **models**: `model="..."`, `<field name="model">`, `_name`, `_inherit`,
  `_inherits`, `env["..."]` and the comodel of a relational field, to the
  model's class;
- **fields**: `<field name="...">` in views (through embedded lists and
  forms to their comodel) and data records, each part of a path in
  `@api.depends`, `related=` and `mapped()`, and attribute access in Python
  (`order.partner_id.country_id`), to the field's definition;
- **methods**: calls on records (`order.action_confirm()`,
  `super().write(vals)`), the methods a field names (`compute=`,
  `inverse=`, `search=`, `selection=`, `group_expand=`) and
  `<button type="object" name="...">`, to the method's definition: in the
  class that defines the model, else in an extension or a parent model. On
  an override (`def action_confirm`), it goes to the method it overrides.
  Methods of Odoo's `BaseModel` (`write`, `create`, ...) have no definition
  in an addon.

For attribute access, odoo-lint reads the model of the records from the
code: `self` in a model's method, `self.env["sale.order"]`, `env.user` and
`env.company`, a variable assigned or looped from those, the parameter of a
lambda in `filtered()`, `sorted()` or `mapped()`, a comprehension, and the
recordset methods that keep the model (`search`, `browse`, `sudo`,
`with_context`, `filtered`, ...) or follow a path (`mapped("partner_id")`).
A parameter of a plain function has no known model.

## Completion

In the same places, completion offers the names that fit, while you type:
the XML ids of the module and its dependencies (the module's own without
its name, as it writes them), the models they define, and the fields of
the model at hand, with their type and comodel. In a path such as
`related="partner_id.co"` it offers the fields of the model the path has
reached; after `order.` in Python, the fields and methods of the records
`order` holds (with the module that defines each method, `BaseModel` for
Odoo's own); in a view, the fields of the embedded list or form the cursor
is in.

## Find references

_Find references_ lists where the XML id, model or field at the cursor is
used: in views and data files (`ref`, `inherit_id`, `groups`, `model`,
`<field name>`), in the strings of Python (`env.ref`, `env["..."]`,
`related=`, `@api.depends`, `mapped()`, comodels), and in attribute access
(`order.partner_id`), and for methods their calls, overrides and buttons.
It starts from a use or from the definition in Python, looks in the modules next to the open one and on the addons path,
and keeps only the mentions whose definition is the same: `name` on
`sale.order` is not `name` on `res.partner`. Finding every use of
`sale.order`'s `partner_id` in all of Odoo 18.0 takes about four seconds.

All three look in the module's dependencies, Odoo's included, as listed in
[`addons-path`](configuration.md#addons-path); the modules next to the
open one are found without it.

## Zed

The extension in
[`integrations/zed`](https://github.com/bosd/odoo-lint/tree/main/integrations/zed)
starts `odl server` for Python, XML and PO files. Zed knows XML and PO
files through extensions of their own: install `XML` and `PO` from the
registry too, or odoo-lint only sees the Python files. It uses, in order,
the binary set in the settings, `odl` on the project's PATH, or `odl` from
the latest GitHub release of odoo-lint, which it downloads.

Install it from Zed's extension registry: open _zed: extensions_ from the
command palette and search for `odoo-lint`. To try a version that is not
released yet, install it from a checkout of this repository instead: run
_zed: install dev extension_ and pick the `integrations/zed` folder (this
needs Rust installed through rustup).

To use another binary, give its arguments too: with a `path`, Zed uses the
`arguments` from the settings, and without `server` the server does not
start:

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
      "extensions": [".py", ".po", ".pot", ".xml"]
    }
  }
}
```

## Other editors

Any editor with a generic LSP client can run `odl server` over stdin and
stdout. It supports full document sync, `textDocument/publishDiagnostics`
and `textDocument/codeAction`, and the UTF-8, UTF-16 and UTF-32 position
encodings.
