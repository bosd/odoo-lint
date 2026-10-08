# AI coding agents

AI coding agents write a lot of Odoo code, and they repeat old patterns:
`name_get`, `self._cr`, `_()` instead of `self.env._()`, translation files
Odoo cannot load. odoo-lint gives them the same feedback a reviewer would,
right after every edit.

Two building blocks work with any agent:

`odl mcp`
: A [Model Context Protocol](https://modelcontextprotocol.io) server on
stdin/stdout with four tools: `check` (lint), `fix` (apply fixes, or show
them as a diff with `dry_run`), `upgrade_check` (what modules need for a
newer Odoo version) and `rule` (explain a code).

`odl hook`
: Lints the file the agent just edited, for a post-edit hook. It reads a
Claude Code hook event on stdin and prints the findings as context for the
model. Files outside an Odoo addon are ignored.

Both need `odl`: `uv tool install odoo-linter` (or `pip install
odoo-linter`). The plugins below also find it in the project's `.venv`, or
run the latest release through `uvx`.

From 0.1.0-alpha.5 on, the MCP server is in the
[MCP Registry](https://registry.modelcontextprotocol.io) as
`io.github.bosd/odoo-lint`, for clients that install servers from there.
They run it as `uvx odoo-linter mcp`.

## Claude Code

The plugin bundles the MCP server, the post-edit hook and a skill that tells
Claude when to use them and how translations get loaded:

```text
/plugin marketplace add bosd/odoo-lint
/plugin install odoo-lint@odoo-lint
```

Or from a shell: `claude plugin marketplace add bosd/odoo-lint` and
`claude plugin install odoo-lint@odoo-lint`. The plugin is in
[`integrations/claude-code`](https://github.com/bosd/odoo-lint/tree/main/integrations/claude-code).

## DeepSeek Harness (dsh)

[`integrations/dsh`](https://github.com/bosd/odoo-lint/tree/main/integrations/dsh)
is a dsh bundle with the same three parts: the MCP server, the hook (through
dsh's Claude Code hooks bridge) and the skill, published on npm as
`dsh-odoo-lint`. Install it into a profile:

```bash
dsh plugin --profile tui add dsh-odoo-lint
dsh --profile tui --dump-config   # shows the odoo-lint rows
```

To try changes that are not published yet, add `./odoo-lint/integrations/dsh`
from a checkout of this repository instead.

## OpenCode

Register the MCP server in `opencode.json`:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "odoo-lint": { "type": "local", "command": ["odl", "mcp"] }
  }
}
```

For linting after every edit, copy
[`integrations/opencode/odoo-lint.js`](https://github.com/bosd/odoo-lint/blob/main/integrations/opencode/odoo-lint.js)
to `.opencode/plugins/` (or `~/.config/opencode/plugins/`): it appends the
findings to the result of the edit, so the model sees them. Copy the skill
from `integrations/claude-code/skills/odoo-lint/` to `.opencode/skills/` as
well.

## Other agents

Any MCP client can run the server. The configuration is usually a variant
of:

```json
{
  "mcpServers": {
    "odoo-lint": { "command": "odl", "args": ["mcp"] }
  }
}
```

Agents with Claude Code compatible hooks can run `odl hook` after their edit
tools. It reads the path from `tool_input.file_path` (or `filePath`, or
`path`) and answers with `hookSpecificOutput.additionalContext`.
