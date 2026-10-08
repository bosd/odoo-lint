<p align="center">
  <img src="https://raw.githubusercontent.com/bosd/odoo-lint/main/docs/_static/logo.svg" alt="odoo-lint logo" width="160">
</p>

<h1 align="center">odoo-lint</h1>

<p align="center">
  <a href="https://pypi.org/project/odoo-linter/"><img src="https://img.shields.io/pypi/v/odoo-linter.svg" alt="PyPI"></a>
  <a href="https://crates.io/crates/odoo-lint"><img src="https://img.shields.io/crates/v/odoo-lint.svg" alt="crates.io"></a>
  <a href="https://odoo-lint.readthedocs.io/"><img src="https://img.shields.io/readthedocs/odoo-lint/latest.svg" alt="Documentation"></a>
  <a href="https://github.com/bosd/odoo-lint/actions/workflows/tests.yml"><img src="https://github.com/bosd/odoo-lint/actions/workflows/tests.yml/badge.svg" alt="Tests"></a>
  <a href="https://github.com/bosd/odoo-lint/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
</p>

<!-- start-docs -->

Blazing fast Rust-native linter for Odoo modules. The command is called `odl`.

## Installation

```bash
uv tool install odoo-linter   # or: pipx install odoo-linter
cargo install odoo-lint       # from crates.io
```

The PyPI package is called `odoo-linter`, because `odoo-lint` is too similar
to an existing PyPI project. On crates.io it is `odoo-lint`.

## Quick start

```bash
odl check path/to/addons   # lint a directory
odl rule                   # list all rules
odl rule C8101             # explain a rule
```

Rules ported from pylint-odoo keep its codes, names and messages, and
`# pylint: disable=` comments keep working. Company-specific rules are
configured in your `pyproject.toml`:

```toml
[tool.odoo-lint]
target-version = "17.0"

[tool.odoo-lint.rules.manifest-required-author.mapping]
"acme_*" = "Acme Corp"
```

See the [documentation](https://odoo-lint.readthedocs.io/) for all commands,
configuration options and rules.

<!-- github-only -->

## Development

```bash
uv sync --all-groups   # builds odl into .venv via maturin
uv run nox             # pre-commit, fmt, clippy, cargo tests, CLI tests, docs
```

See the [development guide](https://odoo-lint.readthedocs.io/en/latest/development.html).

## License

MIT
