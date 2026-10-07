# Configuration

`odl` reads its settings from the nearest configuration file, searching from
the linted path upwards:

1. `odoo-lint.toml`, with the settings at the top level;
2. `pyproject.toml`, with the settings under `[tool.odoo-lint]`.

In each directory `odoo-lint.toml` wins over `pyproject.toml`. A
`pyproject.toml` without a `[tool.odoo-lint]` table is skipped. Use
`odl check --config FILE` to skip the search.

Unknown keys are an error, so a typo such as `manifest_author` instead of
`manifest-author` is reported instead of silently ignored.

## Reference

```toml
[tool.odoo-lint]
# Odoo version to lint for; `odl check --version` overrides it.
target-version = "17.0"

[tool.odoo-lint.rules.manifest-author]
default = "Odoo Community Association (OCA)"

[tool.odoo-lint.rules.manifest-author.mapping]
"acme_*" = "Acme Corp"
```

The same file as `odoo-lint.toml`:

```toml
target-version = "17.0"

[rules.manifest-author]
default = "Odoo Community Association (OCA)"

[rules.manifest-author.mapping]
"acme_*" = "Acme Corp"
```

### `target-version`

Odoo version the code targets, as a string such as `"16.0"`. Default: `"17.0"`.

### `rules.manifest-author`

Settings for [ODOO010](rules/ODOO010.md).

`default`
: Author expected when no mapping matches. Default:
  `"Odoo Community Association (OCA)"`.

`mapping`
: Table of module folder name or `prefix*` pattern to expected author. An
  exact name wins over a pattern; among patterns the longest prefix wins.
