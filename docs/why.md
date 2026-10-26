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

odoo-lint is in alpha: its releases are `0.1.0` pre-releases. It implements
every check of pylint-odoo, `oca-checks-odoo-module` and `oca-checks-po`,
and the [parity page](parity.md) shows, check by check, that it reports
exactly what their own test suites expect. It can replace their pre-commit
hooks; [Migrating from OCA's tooling](migrating.md) lists what changes.
Please report any difference you see on your own code.

In OCA repositories ruff runs alongside both; it covers general Python style,
not the Odoo checks, so odoo-lint replaces pylint-odoo, not ruff.

Skipping type inference is a deliberate trade-off. Checks that need
knowledge across modules, such as whether a field in a view exists on its
model or an XML id is defined in a dependency, use a lightweight index of
the modules' dependencies on the [addons path](configuration.md#addons-path),
not inference, so they stay fast. Without an addons path they stay silent
rather than guess.

Measured timings are on the [benchmarks](benchmarks.md) page; planned work
is on the [roadmap](roadmap.md).
