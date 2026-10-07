# odoo-lint

Blazing fast Rust-native linter for Odoo modules. The CLI is called `odl`.

## Installation

```bash
uv tool install odoo-lint   # or: pipx install odoo-lint
cargo install odoo-lint     # from crates.io
```

## Usage

```bash
odl check path/to/addons            # target Odoo version from config, default 17.0
odl check . --version 18.0          # override the target version
odl check . --config odoo-lint.toml # use an explicit config file
```

Exit codes: `0` no violations, `1` violations found, `2` configuration error.

## Rules

| Code      | Description                                                        |
| --------- | ------------------------------------------------------------------ |
| `ODOO001` | Compute method referenced by `compute=` lacks `@api.depends`       |
| `ODOO010` | `__manifest__.py` author does not match the configured author      |

## Configuration

`odl` looks for `odoo-lint.toml` or a `pyproject.toml` with a
`[tool.odoo-lint]` table, starting at the linted path and walking up.

```toml
[tool.odoo-lint]
target-version = "17.0"

[tool.odoo-lint.rules.manifest-author]
# Used when no pattern matches
default = "Odoo Community Association (OCA)"

# Company-specific overrides by module folder name. An exact name wins,
# otherwise the longest matching `prefix*` pattern.
[tool.odoo-lint.rules.manifest-author.mapping]
"mijnbedrijf_*" = "MijnBedrijf B.V."
"acme_*" = "Acme Corp"
```

In `odoo-lint.toml` the same keys live at the top level (`[rules.manifest-author]`).

## Development

Rust builds the binary, [maturin](https://www.maturin.rs) packages it as a
wheel and [uv](https://docs.astral.sh/uv/) manages the virtual environment.

```bash
uv sync --all-groups   # builds `odl` into .venv via maturin
uv run nox             # pre-commit, fmt, clippy, cargo tests, CLI tests
uv run nox -s clippy   # a single session
```

## License

MIT
