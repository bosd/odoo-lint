# Usage

## `odl check`

Lint addons directories or single files.

```bash
odl check [PATHS]... [--version VERSION] [--config FILE]
          [--select RULES] [--ignore RULES] [--output-format FORMAT]
```

`PATHS`
: Directories or files to lint. Defaults to the current directory. The first
path is where the [configuration](configuration.md) search starts.

`-v`, `--version`, `--odoo-version VERSION`
: Target Odoo version, for example `17.0`. Overrides `target-version` from the
configuration. Defaults to `17.0`.

`--config FILE`
: Use this `odoo-lint.toml` or `pyproject.toml` instead of searching for one.

`--select RULES`
: Comma-separated codes, names or code prefixes to run; replaces `select`
from the configuration.

`--ignore RULES`
: Comma-separated rules to skip; added to `ignore` from the configuration.

`--output-format FORMAT`
: `text` (default), `json` or `github`.

### Output formats

`text`
: pylint's default format, so existing tooling keeps working. The column is
0-based, as in pylint:

```text
addons/acme_sale/__manifest__.py:3:4: C8101: One of the following authors must be present in manifest: 'Acme Corp' (manifest-required-author)
```

`json`
: An array of objects with `file_path`, `line`, `column` (both 1-based),
`code`, `name` and `message`.

`github`
: GitHub Actions annotations, shown inline on pull requests. `E` and `F`
codes are errors, everything else is a warning.

```yaml
- run: odl check --output-format github
```

### Exit codes

| Code | Meaning                        |
| ---- | ------------------------------ |
| `0`  | No violations found            |
| `1`  | One or more violations found   |
| `2`  | Invalid configuration or usage |

## `odl rule`

```bash
odl rule                            # list all rules
odl rule C8101                      # explain a rule by code
odl rule manifest-required-author   # or by name
```

The output is the same as the pages under [rules](rules/index.md).

## In pre-commit

```yaml
repos:
  - repo: local
    hooks:
      - id: odoo-lint
        name: odoo-lint
        entry: odl check
        language: python
        additional_dependencies: [odoo-linter]
        pass_filenames: false
```
