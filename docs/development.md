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

## Architecture

```text
odl check ─> settings ─> linter ─> rules ─> fixer ─> output
               │           │        │
            config.rs   checker.rs index/ (checks across modules)
```

- `src/main.rs`: the command line (clap). Each subcommand calls into the
  library (`src/lib.rs`).
- `src/config.rs` reads `odoo-lint.toml` or `[tool.odoo-lint]`;
  `src/settings.rs` adds the command line options and decides which rules
  run (`select`, `ignore`; `OPT_IN` is in `src/rules/mod.rs`).
- `src/linter.rs` collects the files (`.py`, `.xml`, `.js`, `.po`, `.pot`),
  finds the module of each one, parses it and runs the enabled rules, in
  parallel (rayon). Suppression comments are applied here
  (`src/suppression.rs`).
- `src/checker.rs` is what a rule sees (`PythonContext`, `ManifestContext`,
  ...) and the `Reporter` it reports with; `src/diagnostics.rs` holds the
  `Violation`.
- `src/fixer.rs` applies the fixes attached to violations, in passes, in
  memory (`src/sources.rs`), until nothing is left; `--diff` shows the
  result instead of writing it.
- `src/output.rs` prints the violations as text, JSON, GitHub annotations,
  SARIF or a GitLab report.
- `src/index/` is a model of the addons path: per module, its
  dependencies, models, fields and XML ids, read on first use. Rules that
  look across modules (most `ODOO0xx` rules) ask it what a module's
  `depends` reach. `mod.rs` parses and caches a module, `python.rs` reads
  its model classes, `data.rs` its data files (XML ids, access rights,
  views) and `scope.rs` answers for everything its `depends` reach.
- `src/server.rs` (`odl server`), `src/mcp.rs` (`odl mcp`) and
  `src/hook.rs` (`odl hook`) wrap the linter for editors and AI agents;
  `definition.rs`, `completion.rs` and `usages.rs` add go to definition,
  completion and references to the server.
- Subcommands that write files: `src/upgrade.rs` (`upgrade-check`),
  `src/format.rs` (`format`), `src/readme.rs` and `src/readme_obs.rs`
  (`readme`), `src/requirements.rs` (`requirements`), `src/badge.rs`
  (`badge`) and `src/pot/` (`pot`, hidden). `src/po/` is a port of polib's
  PO parser and writer, used by the PO rules and `pot`.

## Adding a rule

Rules live in `src/rules/`, grouped by what they check:

| Where                                                           | What                                                                 |
| --------------------------------------------------------------- | -------------------------------------------------------------------- |
| `manifest/`                                                     | manifest keys, values and files (C81xx, ...)                         |
| `python/`                                                       | Python checks ported from pylint-odoo                                |
| `xml.rs`                                                        | XML data files (XML0xx from `oca-checks-odoo-module`, XML1xx)        |
| `module.rs`, `module_conventions.rs`                            | module folders, CSV and conventions (MOD0xx, ODOO002, ODOO003)       |
| `po.rs`, `po_odoo.rs`, `po_fixes.rs`, `po_languages.rs`         | translation files (PO0xx, PO1xx)                                     |
| `views.rs`, `records.rs`, `references.rs`, `web_assets.rs`, ... | checks across modules (ODOO0xx)                                      |
| `owl.rs`                                                        | OWL templates (OWL0xx)                                               |
| `upgrade/`                                                      | upgrade rules per Odoo version (Uxxxx), with data in `upgrade/data/` |

1. Define a `pub const` of type `Rule` (`src/rules/mod.rs`): `code`, `name`,
   `summary`, `doc` (Markdown: _What it does_, _Why is this bad?_, an
   example, and _Fix safety_ when it has a fix), `check` and optionally
   `min_odoo` and `max_odoo`. `check` says when it runs:

   | `Check` variant | Runs                                                        |
   | --------------- | ----------------------------------------------------------- |
   | `Python`        | on the syntax tree of every Python file                     |
   | `Manifest`      | once per module, on its parsed manifest                     |
   | `Po`            | on every `.po` and `.pot` file                              |
   | `Xml`           | once per module, on the XML files its manifest loads        |
   | `Module`        | once per module, on its folder and manifest                 |
   | `Js`            | on every JavaScript file of `static/src` and `static/tests` |
   | `PythonXml`     | a `Python` and an `Xml` check in one rule                   |

2. Register it in `ALL` in `src/rules/mod.rs`. `ALL` is sorted by code; a
   test fails otherwise. If `ALL` should not select it, add its code to
   `OPT_IN` in `src/rules/mod.rs` and to the table in
   [Configuration](configuration.md#opt-in-rules); a test checks the two
   agree. A check across modules goes in `ACROSS_MODULES` next to it.
3. Options go in `RulesConfig` in `src/config.rs` (and in
   `NO_RULES_CONFIG` next to it), under the rule's name, with
   pylint-odoo's option names and defaults for ported rules.
4. Regenerate the documentation pages:

   ```bash
   UPDATE_DOCS=1 cargo test --test generated_docs
   ```

   CI fails when `docs/rules/` is out of date. Do not edit those pages by
   hand.

5. Add tests. Small checks have unit tests next to the rule
   (`checker::run_python_rule` and `run_manifest_rule`). Rules that need a
   module build one in a temporary folder (`tempfile::tempdir()`), write
   its manifest and files, and lint it with `odoo_lint::linter::lint_paths`;
   see the files in `tests/`, one per area.

### Fixes

A fix is a list of `Edit`s (byte ranges of the file, or of another file)
attached to the violation:

```rust
reporter.report(&RULE, offset, message).fix =
    Some(Fix::safe("Use `self.env.cr`", vec![Edit::replace(start, end, "self.env.cr")]));
```

Choose the `Applicability` (`src/fix.rs`) as Ruff does: `Fix::safe` when
the result behaves the same or only formatting changes, `Fix::unsafe_` when
it is probably right but needs review, or loses something (a comment, a
translation). `--fix` applies only safe fixes. Say which it is in the rule's
_Fix safety_ section, and test the fix with `fixer::fix_paths`
(`tests/fix.rs`).

### Rules across modules

A rule that needs to know what another module defines (a field of an
inherited model, an XML id from a dependency) uses `src/index/`:
`index::closure(module, addons_path)` gives the modules its `depends`
reach. When a dependency is missing from the addons path, the closure is
`None` and the rule reports nothing rather than guess.

### Upgrade rules

The rename and removal data of the upgrade rules (`U1590` and later) in
`src/rules/upgrade/data/` is generated from OpenUpgrade by
`scripts/gen_upgrade_data.py`; do not edit it by hand.

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
odoo-pre-commit-hooks (`oca-checks-po` and `oca-checks-odoo-module`),
fetched at pinned commits into
`.cache/` and not vendored, and compares the number of messages per check
with what their test suites expect. It fails when a
check odoo-lint implements reports a different number, so a ported check is
only done when it matches. After adding or changing a rule, update the
[parity page](parity.md) and the README badge with:

```bash
uv run nox -s parity -- --write
```

## Releasing

1. Bump `version` in `Cargo.toml` and run `uv lock`. Use the same version in
   the Claude Code plugin (`plugin.json`) and the dsh bundle (`package.json`),
   and pin it in their `bin/odl` (`odoo-linter==0.1.0a5` for
   `0.1.0-alpha.5`); `tests/integrations.rs` checks this.
2. Wait for the push of step 1 to put the release assets into the draft
   (the _Put the assets into the draft release_ job, about ten minutes),
   then publish the GitHub release drafted by Release Drafter, with tag
   `v<version>`.
3. Once the release is on PyPI, tag the Claude Code plugin and push the tag:
   `claude plugin tag --push integrations/claude-code` creates
   `odoo-lint--v<version>`. Submit that tag as an update in Anthropic's
   developer portal ([claude.ai/directory/manage](https://claude.ai/directory/manage)).
   A tag does not move, so later merges to `main` do not invalidate the
   submission.

Releases are immutable once published, so their assets are added before:
while the version in `Cargo.toml` has no release yet, every push to `main`
builds `odl` for each platform and puts it into the draft release, as
`odl-<target>.tar.gz` (`.zip` on Windows; `scripts/release_asset.py` has the
list). The Zed extension downloads these. When the release is published, a job
reports any asset that is missing.

The release workflow checks that the tag matches `Cargo.toml`, builds wheels
for Linux, macOS and Windows, signs them with Sigstore, attaches the
signatures to the GitHub release, and publishes to PyPI (`odoo-linter`) and
crates.io (`odoo-lint`) with trusted publishing. It then publishes
`server.json` to the MCP Registry, with the version from `Cargo.toml`, and
the dsh bundle to npm (`dsh-odoo-lint`), also with trusted publishing.

The VS Code extension (`integrations/vscode`) has a version of its own in its
`package.json`, as the marketplaces do not take prerelease tags. The release
workflow publishes it to Open VSX (trusted publishing, environment
`open-vsx`) and the Visual Studio Marketplace (secret `VSCE_PAT`) and skips a version that is already there, so
bump it when the extension changes. To publish it between releases, run the
Release workflow by hand (_Run workflow_ on GitHub).

### Python and polib compatibility

The PO checks emulate Python's `%` and `str.format` errors and use a port of
polib's PO writer. `tests/python_compat.rs` checks both against the real
implementations, using cases generated with CPython and polib:

```bash
uv run --no-project --with polib==1.2.0 python scripts/gen_python_compat_cases.py
```
