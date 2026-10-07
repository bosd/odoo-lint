# Usage

## `odl check`

Lint an addons directory or a single file.

```bash
odl check [PATH] [--version VERSION] [--config FILE]
```

`PATH`
: Directory or file to lint. Defaults to the current directory.

`-v`, `--version VERSION`
: Target Odoo version, for example `17.0`. Overrides `target-version` from the
  configuration. Defaults to `17.0`.

`--config FILE`
: Use this `odoo-lint.toml` or `pyproject.toml` instead of searching for one.
  See [configuration](configuration.md).

Each violation is printed as `path:line: [CODE] message`.

### Exit codes

| Code | Meaning                         |
| ---- | ------------------------------- |
| `0`  | No violations found             |
| `1`  | One or more violations found    |
| `2`  | Invalid configuration or usage  |

## `odl rule`

```bash
odl rule            # list all rules
odl rule ODOO001    # show the documentation of one rule
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
