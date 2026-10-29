# Getting started

From installing `odl` to running it on every commit, in CI and in your
editor. Each step links to the page with the details.

## 1. Install

```bash
uv tool install odoo-linter   # or: pipx install odoo-linter
cargo install odoo-lint       # from crates.io
```

The PyPI package is `odoo-linter`, the command is `odl`. The wheels hold one
binary without Python dependencies, so it does not matter which Python your
project uses.

## 2. Lint a repository

In the root of an addons repository:

```bash
odl check
```

```text
./acme_sale/__manifest__.py:1:0: C8112: Missing ./README.rst file. Template here: https://github.com/OCA/maintainer-tools/blob/master/template/module/README.rst (missing-readme)
./acme_sale/__manifest__.py:4:4: C8101: One of the following authors must be present in manifest: 'Odoo Community Association (OCA)' (manifest-required-author)
./acme_sale/models/sale_order.py:9:8: ODOO001: Compute method '_compute_margin_total' (field(s): margin_total) is missing @api.depends (missing-depends)
Found 3 violation(s).
```

Each line ends with the rule's name; `odl rule ODOO001` (or
`odl rule missing-depends`) explains it. The defaults are OCA's: the OCA as
required author, OCA's README template. Step 3 changes them.

Some violations can be fixed automatically:

```bash
odl check --diff   # show the safe fixes
odl check --fix    # apply them
```

See [Usage](usage.md) for every option and [Fixes](usage.md#fixes) for safe
and unsafe fixes.

## 3. Configure

Put the settings in `pyproject.toml` (or in an `odoo-lint.toml`):

```toml
[tool.odoo-lint]
target-version = "18.0"
ignore = ["missing-readme"]

[tool.odoo-lint.rules.manifest-required-author]
authors = ["Acme Corp"]
```

- `target-version`: the Odoo version of the branch.
- `select` and `ignore`: which rules run, by code, name or code prefix.
- `addons-path`: where Odoo and your dependencies are, for the checks across
  modules (fields in views, XML ids, models).

See [Configuration](configuration.md) for every key. Coming from OCA's
pre-commit hooks and a `.pylintrc`, see
[Migrating from OCA's tooling](migrating.md).

## 4. Run it on every commit and in CI

With [pre-commit](https://pre-commit.com), start with the advisory hook: it
shows the findings but never fails the commit.

```yaml
repos:
  - repo: https://github.com/bosd/odoo-lint-pre-commit
    rev: v0.1.0a18
    hooks:
      - id: odoo-lint-advisory
```

Once the repository is clean, switch to `id: odoo-lint`, which fails the
commit. For hk, GitHub Actions (annotations or code scanning), GitLab and
Forgejo, see [Pre-commit, hk and CI](integrations.md).

## 5. In your editor

`odl server` is a language server: findings while you type, quick fixes,
go to definition and completion for models, fields and XML ids. Install
the odoo-lint extension for VS Code (Marketplace or Open VSX) or Zed, or
configure `odl server` in Neovim, Helix or any LSP client; see
[Editors](editors.md).

For AI coding agents there is an MCP server and a post-edit hook; see
[AI coding agents](ai.md).

## Next

- [Upgrades](upgrades.md): what your modules need for the next Odoo version.
- [Translations](translations.md): why a `.po` file does not load.
- [Rules](rules/index.md): every rule, with examples.
