# odoo-lint for VS Code

Lints Odoo addons as you type with [odoo-lint](https://github.com/bosd/odoo-lint)
(`odl`): pylint-odoo's checks, OCA's manifest, XML and translation (`.po`/`.pot`)
checks, translation files Odoo cannot load, and leftovers of Odoo upgrades.

- Findings in Python, XML and translation files, with a link to each rule's page
- Quick fixes, and **Fix all** (`source.fixAll.odoo-lint`) for the safe ones
- Unsaved changes are linted as they are; files outside an Odoo addon are left alone

It works in VS Code and in editors based on it that install extensions from
Open VSX.

## The `odl` it runs

In this order:

1. `odoo-lint.path` from the settings
2. `odl` in the workspace's `.venv`, then on `PATH`
3. the latest release from PyPI, downloaded once and checked against PyPI's
   SHA-256 (turn this off with `odoo-lint.download`)

To install it yourself: `uv tool install odoo-linter`.

## Configuration

odoo-lint reads `[tool.odoo-lint]` from `pyproject.toml`, or `odoo-lint.toml`;
see [Configuration](https://odoo-lint.readthedocs.io/en/latest/configuration.html).

To fix on save:

```json
"editor.codeActionsOnSave": {
  "source.fixAll.odoo-lint": "explicit"
}
```

The commands **odoo-lint: Restart Server** and **odoo-lint: Show Logs** are in
the command palette.

## License

MIT
