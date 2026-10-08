---
name: odoo-lint
description: Lint and fix Odoo addons with odoo-lint (`odl`). Use when writing or reviewing Odoo module code, manifests, XML views and data, or .po/.pot translation files; when pylint-odoo, oca-checks-po, oca-checks-odoo-module or pre-commit report codes like C8101, W8161, E8103, PO001 or xml-duplicate-record-id; or when Odoo logs "malformed po file" or translations do not show up.
---

# odoo-lint

odoo-lint (`odl`) checks Odoo addons: Python code, `__manifest__.py`, XML
data files and translation files. Codes, names and messages are pylint-odoo's
(`C8xxx`, `W8xxx`, `E8xxx`, `R8xxx`); OCA's PO and XML checks keep their names
(`PO001`-`PO007`, `XML001`-`XML024`). `PO1xx` rules predict what Odoo's
translation loader does; `XML1xx` rules find leftovers of version upgrades,
such as Bootstrap 4 classes in Odoo 15.0 and later.

## Tools

The `odoo-lint` MCP server has three tools. Without it, use the CLI.

| Task              | MCP tool                                    | CLI                                      |
| ----------------- | ------------------------------------------- | ---------------------------------------- |
| Lint              | `check` (`paths`, `select`, `odoo_version`) | `odl check <paths>`                      |
| Preview fixes     | `fix` with `dry_run: true`                  | `odl check --diff <paths>`               |
| Apply safe fixes  | `fix`                                       | `odl check --fix <paths>`                |
| Also unsafe fixes | `fix` with `unsafe: true`                   | `odl check --fix --unsafe-fixes <paths>` |
| Explain a code    | `rule` (`rule: "W8161"`)                    | `odl rule W8161`                         |

The project's configuration (`[tool.odoo-lint]` in `pyproject.toml`, or
`odoo-lint.toml`) sets the Odoo version and rules; pass `odoo_version` only
when there is none. After every edit of an addon file a hook lints it and
reports what it finds.

## Working with findings

- Fix what you introduced. Leave unrelated existing findings alone unless
  asked; mention them instead.
- Apply safe fixes with the tool rather than by hand. Unsafe fixes can change
  behaviour (e.g. adding `return` before `super()`): look at the dry-run diff
  and apply only what is right.
- Read the rule (`rule` tool) before working around a finding you do not
  understand.
- Suppress only with a reason, as in pylint:
  `# pylint: disable=sql-injection` on the line, or in the configuration.

## Translations (.po/.pot)

When translations do not show up or Odoo logs "malformed po file", lint the
module's `i18n/` folder with `select: ["PO"]`:

- `PO101`: the msgid is not in the `.pot`, so Odoo drops the translation.
  Regenerate the `.pot` from a running Odoo when possible (export the module
  without a language); the unsafe fix copies the msgid into the `.pot`.
- `PO102`: a reference Odoo cannot read; `code:` references need a line
  number (`:0`).
- `PO103`: the file name is not a language code, so Odoo never loads it.
- `PO104`: fuzzy translations are loaded as if reviewed.
- Even a correct file needs a reload: code translations (`_()`) need a
  server restart; model and view translations need
  `odoo-bin -u <module> --i18n-overwrite`.

Never edit `msgid`s in a `.po` file: they must match the source text.
