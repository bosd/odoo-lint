# odoo-lint for DeepSeek Harness

Lints and fixes Odoo addons with [odoo-lint](https://github.com/bosd/odoo-lint)
(`odl`) in [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) (dsh): pylint-odoo's
checks, OCA's manifest, XML and PO checks, translation files Odoo cannot load,
and upgrade checks for newer Odoo versions.

The bundle has three parts:

- **MCP server** (`odl mcp`) with the tools `check`, `fix` (or a diff with
  `dry_run`), `upgrade_check` and `rule` (explain a code).
- **Hook**, through dsh's Claude Code hooks bridge: after the agent edits a
  file in an Odoo addon, it lints that file and gives the agent the findings.
- **Skill**: when to use the tools, how to treat findings and fixes, and how
  Odoo loads translations.

## Install

```bash
dsh plugin --profile tui add dsh-odoo-lint
dsh --profile tui --dump-config   # shows the odoo-lint rows
```

The bundle runs `odl` from the project's `.venv`, then from PATH, and
otherwise the odoo-lint release with the plugin's version, from PyPI through
`uvx`. Install it with
`uv tool install odoo-linter` or `pip install odoo-linter`.

## License

MIT
