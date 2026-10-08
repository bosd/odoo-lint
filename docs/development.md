# Development

Rust builds the `odl` binary, [maturin](https://www.maturin.rs) packages it
as a wheel and [uv](https://docs.astral.sh/uv/) manages the virtual
environment. You need a Rust toolchain (see `rust-version` in `Cargo.toml`)
and uv.

```bash
uv sync --all-groups   # builds odl into .venv via maturin
uv run nox             # pre-commit, fmt, clippy, cargo tests, CLI tests
uv run nox -s clippy   # a single session
uv run nox -s docs     # live-reloading documentation
```

## Adding a rule

1. Add `src/rules/<code>_<name>.rs` with a `pub const RULE: Rule` holding the
   code, name, summary, Markdown documentation, the check function
   (`Check::Python` or `Check::Manifest`) and optionally the Odoo versions it
   applies to.
2. Register it in `ALL` in `src/rules/mod.rs`, sorted by code. The linter
   runs it for every Python file or once per module manifest.
3. Regenerate the documentation pages:

   ```bash
   UPDATE_DOCS=1 cargo test --test generated_docs
   ```

   CI fails when `docs/rules/` is out of date.

### Codes, names and messages

- Rules ported from [pylint-odoo](https://github.com/OCA/pylint-odoo) keep its
  message id as code, its symbolic name and its message text, so existing
  `# pylint: disable=` comments, configs and forum answers keep applying.
- pylint-odoo is AGPL-3.0 and odoo-lint is MIT: rules are reimplemented from
  their documented behaviour, never translated from the pylint-odoo source,
  and pylint-odoo's test fixtures are not copied into this repository.
- Rules of odoo-lint's own use `ODOO###` codes.

### Parity with OCA's linters

`uv run nox -s parity` lints the test repositories of pylint-odoo and of
`oca-checks-po` (odoo-pre-commit-hooks), fetched at pinned commits into
`.cache/` and not vendored, and compares the number of messages per check
with what their test suites expect. It fails when a
check odoo-lint implements reports a different number, so a ported check is
only done when it matches. After adding or changing a rule, update the
[parity page](parity.md) and the README badge with:

```bash
uv run nox -s parity -- --write
```

## Releasing

1. Bump `version` in `Cargo.toml` and run `uv lock`.
2. Publish the GitHub release drafted by Release Drafter, with tag
   `v<version>`.

The release workflow checks that the tag matches `Cargo.toml`, builds wheels
for Linux, macOS and Windows, signs them with Sigstore, attaches the
signatures to the GitHub release, and publishes to PyPI (`odoo-linter`) and
crates.io (`odoo-lint`) with trusted publishing.

### Python and polib compatibility

The PO checks emulate Python's `%` and `str.format` errors and use a port of
polib's PO writer. `tests/python_compat.rs` checks both against the real
implementations, using cases generated with CPython and polib:

```bash
uv run --no-project --with polib==1.2.0 python scripts/gen_python_compat_cases.py
```
