# Contributing

Bug reports, false positives and differences with pylint-odoo or OCA's
pre-commit hooks are welcome as
[issues](https://github.com/bosd/odoo-lint/issues). Include the `odl`
version (`odl --version`), the command and a minimal module that shows the
problem.

To work on the code:

```bash
uv sync --all-groups   # builds odl into .venv via maturin
uv run nox             # pre-commit, fmt, clippy, cargo tests, CLI tests, docs
```

The [development guide](docs/development.md)
([online](https://odoo-lint.readthedocs.io/en/latest/development.html))
describes the architecture, how to add a rule and its tests, the parity
check against OCA's linters and the release process.

Rules ported from pylint-odoo are reimplemented from their documented
behaviour: pylint-odoo is AGPL-3.0 and odoo-lint is MIT, so do not copy its
source or test fixtures.
