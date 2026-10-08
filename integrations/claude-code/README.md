# odoo-lint for Claude Code

Lints and fixes Odoo addons with [odoo-lint](https://github.com/bosd/odoo-lint)
(`odl`): pylint-odoo's checks, OCA's manifest, XML and PO checks, translation
files Odoo cannot load, and upgrade checks for newer Odoo versions.

The plugin has three parts:

- **MCP server** (`odl mcp`) with the tools `check`, `fix` (or a diff with
  `dry_run`), `upgrade_check` and `rule` (explain a code).
- **Hook**: after Claude edits, writes or creates a file in an Odoo addon,
  it lints that file and gives Claude the findings. Other files are left
  alone.
- **Skill**: tells Claude when to use the tools, how to treat findings and
  fixes, and how Odoo loads translations.

## Install

```text
/plugin marketplace add bosd/odoo-lint
/plugin install odoo-lint@odoo-lint
```

The plugin runs `odl` from the project's `.venv`, then from PATH, and
otherwise the odoo-lint release with the plugin's version, from PyPI through
`uvx`. Install it with
`uv tool install odoo-linter` or `pip install odoo-linter`.

## Configuration

`odl` reads `[tool.odoo-lint]` from `pyproject.toml`, or `odoo-lint.toml`;
see [Configuration](https://odoo-lint.readthedocs.io/en/latest/configuration.html).
The plugin has no settings of its own.

## License

MIT
