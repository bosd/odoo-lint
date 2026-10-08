<p align="center">
  <img src="https://raw.githubusercontent.com/bosd/odoo-lint/main/docs/_static/logo.svg" alt="odoo-lint logo" width="160">
</p>

<h1 align="center">odoo-lint</h1>

<p align="center">
  <a href="https://pypi.org/project/odoo-linter/"><img src="https://img.shields.io/pypi/v/odoo-linter?style=flat-square&logo=pypi&logoColor=white&label=PyPI&color=ef662f" alt="PyPI"></a>
  <a href="https://crates.io/crates/odoo-lint"><img src="https://img.shields.io/crates/v/odoo-lint?style=flat-square&logo=rust&logoColor=white&label=crates.io&color=ef662f" alt="crates.io"></a>
  <a href="https://github.com/bosd/odoo-lint/actions/workflows/tests.yml"><img src="https://img.shields.io/github/actions/workflow/status/bosd/odoo-lint/tests.yml?branch=main&style=flat-square&logo=githubactions&logoColor=white&label=tests" alt="Tests"></a>
  <a href="https://odoo-lint.readthedocs.io/en/latest/parity.html"><img src="https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fbosd%2Fodoo-lint%2Fmain%2Fdocs%2F_static%2Fparity.json&style=flat-square" alt="OCA parity"></a>
  <a href="https://odoo-lint.readthedocs.io/"><img src="https://img.shields.io/readthedocs/odoo-lint?style=flat-square&logo=readthedocs&logoColor=white&label=docs" alt="Documentation"></a>
  <a href="https://github.com/bosd/odoo-lint/blob/main/LICENSE"><img src="https://img.shields.io/github/license/bosd/odoo-lint?style=flat-square&color=7e5a8c" alt="License: MIT"></a>
</p>

<!-- start-docs -->

Blazing fast Rust-native linter for Odoo modules. The command is called `odl`.

It checks Python files, manifests, XML and CSV data files and translation
(`.po`) files, with the checks of OCA's pylint-odoo, `oca-checks-odoo-module`
and `oca-checks-po`: same names, same messages, same results on their own
test suites. It also warns about files that do not belong in a module, such
as packages or database dumps committed by accident.

<p align="center">
  <img alt="Bar chart: linting OCA/sale-workflow takes 0.10s with odoo-lint and 25.8s with pylint-odoo" src="https://raw.githubusercontent.com/bosd/odoo-lint/main/docs/_static/benchmark.svg" width="640">
</p>

<p align="center"><i>Linting all of OCA/sale-workflow 18.0, as OCA's pre-commit hook runs pylint-odoo. See <a href="https://odoo-lint.readthedocs.io/en/latest/benchmarks.html">benchmarks</a>.</i></p>

- ⚡ **250x faster** than pylint with pylint-odoo, and still 30x faster on a
  single core
- ⚖️ **Drop-in for pylint-odoo and OCA's pre-commit checks**: same codes,
  names and messages, 95/95 checks matching on their own test suites (Python,
  manifests, XML, `.po`), and `# pylint: disable=` comments keep working
- 🔧 **Automatic fixes**, safe and unsafe as in Ruff, for Python, manifests,
  XML and `.po` files (`odl check --fix`, `--diff` to preview)
- ⬆️ **Upgrade checks**: leftovers of older Odoo versions, such as Bootstrap 4
  classes that do nothing since Odoo 15.0, with fixes
- 🌍 **Translation checks that predict Odoo's loader**: why Odoo logs
  "malformed po file", and why a translation does not show up
- 📦 **Installable with pip, uv or pipx**: one binary in prebuilt wheels for
  Linux, macOS and Windows, without Python dependencies
- 🛠️ **Configured in `pyproject.toml`**, including company-specific author
  rules
- 📋 **Output for CI**: GitHub annotations, GitLab Code Quality, SARIF and
  JSON
- 🤖 **For AI coding agents**: an MCP server, and plugins for Claude Code,
  DeepSeek Harness and OpenCode that lint every file the agent edits
- 🪝 **Git hooks**: pre-commit and hk

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
odl check --fix            # apply the safe fixes (--diff to preview)
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

For AI coding agents there is an MCP server (`odl mcp`) and a post-edit hook,
packaged as plugins for Claude Code (`/plugin marketplace add bosd/odoo-lint`),
DeepSeek Harness and OpenCode.

<!-- mcp-name: io.github.bosd/odoo-lint -->

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
