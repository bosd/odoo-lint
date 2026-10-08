# Why odoo-lint?

```{image} _static/banner.svg
:alt: odoo-lint banner
:align: center
```

## The problem: linting Odoo code is slow

[pylint-odoo](https://github.com/OCA/pylint-odoo) is the standard linter for
Odoo modules. The OCA runs it in the pre-commit configuration and CI of almost
every repository, and it encodes years of community knowledge about what goes
wrong in Odoo code.

It is also slow, and that is a consequence of how it is built rather than of
the checks themselves:

- **It runs on pylint and astroid.** Before any Odoo check runs, astroid builds
  an inference model of every module and follows imports to resolve names and
  types. That is powerful, but the cost grows with the size of the codebase,
  and most Odoo checks never need it.
- **It is Python.** Even with `--jobs`, every worker pays the start-up and
  inference cost again, and a single module is processed by a single core.
- **It needs a matching Python environment.** pylint, astroid and pylint-odoo
  have to be pinned to versions that support the Python version of the
  project. Upgrading one of them regularly means fixing the others.

On a large addons repository a full run takes minutes. The result is familiar:
pre-commit hooks get skipped with `--no-verify`, linting moves to CI only, and
developers wait for feedback they could have had while typing.

## The approach: Ruff, but for Odoo

The Python ecosystem solved the same problem once already. Ruff replaced
flake8, isort and friends by reimplementing their checks in Rust, and made
linting fast enough to run on every save. odoo-lint applies that idea to Odoo:

- **Written in Rust, shipped as one binary.** `odl` has no runtime
  dependencies and does not care which Python version your project uses.
  Install it with `uv tool install odoo-linter`, `pipx` or `cargo`.
- **Parsed with Ruff's parser.** Python files are parsed with
  [`ruff_python_parser`](https://github.com/astral-sh/ruff), the same parser
  that powers Ruff.
- **No type inference.** Rules work on the syntax tree and target concrete
  Odoo patterns, such as a `compute=` method without `@api.depends`. Whatever
  can be decided from the source is decided from the source.
- **Every file in parallel.** Files are linted concurrently on all CPU cores.
- **Company rules without plugins.** Organisation-specific requirements, like
  the expected manifest author per module prefix, are plain configuration in
  `pyproject.toml` instead of Python plugin code. See
  [configuration](configuration.md).
- **Explains itself.** `odl rule ODOO001` prints what a rule checks, why it
  matters and how to fix it, and the same text is on the [rules](rules/index.md)
  pages.

## Status and trade-offs

odoo-lint is young, but it implements all of pylint-odoo's checks: the
[parity page](parity.md) shows, check by check, that odoo-lint reports exactly
what pylint-odoo's own test suite expects on its test repository. It can
replace the pylint-odoo pre-commit hook; please report any difference you see
on your own code.

In OCA repositories ruff runs alongside both; it covers general Python style,
not the Odoo checks, so odoo-lint replaces pylint-odoo, not ruff.

Skipping type inference is a deliberate trade-off. A few checks need knowledge
across modules, such as which model a field is inherited from. Those will be
built on a lightweight index of the addons being linted, not on full
inference, so they stay fast.

Support for XML views, `ir.model.access.csv` and more manifest checks is on
the roadmap.

## Benchmarks

Speed is the reason this project exists, so it has to be measured, not
claimed. A benchmark section comparing `odl` and pylint-odoo on real OCA
repositories, with the exact commands and versions used, will be added here.
