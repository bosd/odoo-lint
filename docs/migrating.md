# Migrating from OCA's tooling

odoo-lint implements the checks of pylint-odoo, `oca-checks-odoo-module` and
`oca-checks-po` under the same names and messages, and some of OCA's
generator hooks. This page maps an OCA-style `.pre-commit-config.yaml` and
`.pylintrc` to odoo-lint, and lists what differs.

## Hooks

| OCA hook                                              | odoo-lint                                               | Notes                                                                                                                                         |
| ----------------------------------------------------- | ------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `pylint_odoo` with `.pylintrc-mandatory`              | `odoo-lint`                                             | Every pylint-odoo check, same codes and names. See [rule selection](#rule-selection).                                                         |
| `pylint_odoo` with `.pylintrc` and `--exit-zero`      | `odoo-lint-advisory`                                    | Shows the findings, never fails the commit.                                                                                                   |
| `oca-checks-odoo-module`                              | `odoo-lint`                                             | `MOD0xx` and `XML0xx` rules, same check names.                                                                                                |
| `oca-checks-odoo-module-fixit`                        | `odoo-lint` with `args: [--fix]`                        | Its fixes are those of [C8116](rules/C8116.md) and of [MOD009](rules/MOD009.md) (opt-in; [W8113](rules/W8113.md) reports the same).           |
| `oca-checks-po`                                       | `odoo-lint`                                             | `PO001` to `PO007`. `--disable=po-pretty-format` is not needed: [PO007](rules/PO007.md) is opt-in.                                            |
| `oca-fix-manifest-website`                            | `odoo-lint` with `args: [--fix]`                        | Set the URL in [`rules.manifest-website`](configuration.md#rulesmanifest-website) ([MOD010](rules/MOD010.md)) instead of the hook's argument. |
| `oca-gen-external-dependencies`                       | `odoo-lint-requirements`                                | Same `requirements.txt`, byte for byte; see [requirements.txt](requirements-txt.md).                                                          |
| `prettier` with `@prettier/plugin-xml`                | `odoo-lint-format` for `.xml`, `.scss`, `.css`, `.less` | Keep prettier for the other file types. The first run re-indents some lines; see [Formatting XML](format.md).                                 |
| `oca-gen-addon-readme`                                | none                                                    | `odl readme` builds `README.md` from Markdown fragments in the layout of the OBS client template, not OCA's `README.rst`. Keep OCA's hook.    |
| `eslint`                                              | none                                                    | `odl check` lints JavaScript for Odoo-specific problems (OWL, upgrades), not eslint's rules. Keep eslint.                                     |
| `oca-update-pre-commit-excluded-addons`, `whool-init` | none                                                    | Keep them.                                                                                                                                    |
| `ruff`, `ruff-format`                                 | none                                                    | odoo-lint has no general Python style checks. Keep ruff.                                                                                      |

All hooks come from
[odoo-lint-pre-commit](https://github.com/bosd/odoo-lint-pre-commit); see
[Pre-commit, hk and CI](integrations.md#pre-commit). The `odoo-lint` hook
covers pylint-odoo, `oca-checks-odoo-module` and `oca-checks-po` in one run,
on `.py`, `.xml`, `.po`, `.pot` and `.js` files.

## `.pylintrc` to `[tool.odoo-lint]`

Settings go in `pyproject.toml` under `[tool.odoo-lint]`, or in an
`odoo-lint.toml` without the prefix; see [Configuration](configuration.md).

| `.pylintrc`                         | `[tool.odoo-lint]`                                                                            |
| ----------------------------------- | --------------------------------------------------------------------------------------------- |
| `enable=` (after `disable=all`)     | `select = [...]`                                                                              |
| `disable=`                          | `ignore = [...]`                                                                              |
| `valid-odoo-versions=18.0`          | `target-version = "18.0"`, and `rules.manifest-version-format.valid-odoo-versions = ["18.0"]` |
| `manifest-required-authors=`        | `rules.manifest-required-author.authors`                                                      |
| `manifest-required-keys=`           | `rules.manifest-required-key.keys`                                                            |
| `manifest-required-keys-app=`       | `rules.manifest-required-key-app.keys`                                                        |
| `manifest-deprecated-keys=`         | `rules.manifest-deprecated-key.keys`                                                          |
| `manifest-keys-values-true=`        | `rules.manifest-superfluous-key.keys-values-true`                                             |
| `manifest-version-format=`          | `rules.manifest-version-format.format`                                                        |
| `license-allowed=`                  | `rules.license-allowed.allowed`                                                               |
| `development-status-allowed=`       | `rules.development-status-allowed.allowed`                                                    |
| `category-allowed=`                 | `rules.category-allowed.allowed`                                                              |
| `category-allowed-app=`             | `rules.category-allowed-app.allowed`                                                          |
| `readme-template-url=`              | `rules.missing-readme.template-url`                                                           |
| `odoo-required-files=`              | `rules.missing-odoo-file.files`                                                               |
| `odoo-required-files-app=`          | `rules.missing-odoo-file-app.files`                                                           |
| `attribute-deprecated=`             | `rules.attribute-deprecated.attributes`                                                       |
| `cursor-expr=`                      | `rules.invalid-commit.cursor-expr`                                                            |
| `deprecated-field-parameters=`      | `rules.renamed-field-parameter.parameters`                                                    |
| `deprecated-odoo-model-methods=`    | `rules.deprecated-odoo-model-method.methods`                                                  |
| `external-request-timeout-methods=` | `rules.external-request-timeout.methods`                                                      |
| `method-required-super=`            | `rules.method-required-super.methods`                                                         |
| `prohibited-method-override=`       | `rules.prohibited-method-override.methods`                                                    |
| `no-missing-return=`                | `rules.missing-return.ignore-methods`                                                         |
| `odoo-exceptions=`                  | none                                                                                          |
| `msg-template=`, `output-format=`   | `odl check --output-format` (text, json, github, sarif, gitlab)                               |

Comma-separated values become TOML lists. Each rule's page lists its
options and defaults, which are pylint-odoo's except where noted under
[Differences](#differences). The `.pylintrc` of an OCA 18.0 repository
becomes:

```toml
[tool.odoo-lint]
target-version = "18.0"

[tool.odoo-lint.rules.manifest-required-key]
keys = ["license"]

[tool.odoo-lint.rules.manifest-deprecated-key]
keys = ["description", "active"]

[tool.odoo-lint.rules.license-allowed]
allowed = ["AGPL-3", "GPL-2", "GPL-2 or any later version", "GPL-3", "GPL-3 or any later version", "LGPL-3"]

[tool.odoo-lint.rules.manifest-version-format]
valid-odoo-versions = ["18.0"]
```

The required author (`Odoo Community Association (OCA)`) and the README
template URL are already the defaults.

## Rule selection

By default `select = ["ALL"]`: every rule except the
[opt-in ones](configuration.md#opt-in-rules). That is more than OCA runs.
odoo-lint adds rule families of its own:

| Codes             | What                                                                                                                               |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `ODOO0xx`         | Checks across modules (fields, XML ids, models, methods, widgets, OWL templates), unwanted and large files, missing `@api.depends` |
| `U1xxx`, `U2xxx`  | [Upgrade](upgrades.md) leftovers of older Odoo versions, per version                                                               |
| `PO1xx`           | [Translations](translations.md) Odoo cannot load or ignores                                                                        |
| `XML1xx`          | Bootstrap 4 classes, view extensions appended to a whole form                                                                      |
| `MOD010`–`MOD013` | Repository conventions: website, `readme/` fragments, icon, tests                                                                  |
| `OWL0xx`          | OWL templates                                                                                                                      |

For the checks OCA runs and no more, ignore those families:

```toml
[tool.odoo-lint]
ignore = ["ODOO", "OWL", "U", "PO1", "XML1", "MOD01"]
```

A copied `enable=` list works as `select`, as the codes and names are
pylint-odoo's. Names odoo-lint does not implement produce a warning, not an
error. OCA's `.pylintrc-mandatory` lists about 40 of them: pylint's own
checks (`eval-used`, `dangerous-default-value`, ...; ruff covers many of them) and
names pylint-odoo dropped or that moved to `oca-checks-odoo-module` under
another name (`duplicate-xml-record-id` is
[`xml-duplicate-record-id`](rules/XML005.md)). Drop them from the list.

To keep OCA's split between blocking and optional checks, put the blocking
list in `select` and run the advisory hook with `--select ALL`:

```yaml
- id: odoo-lint
- id: odoo-lint-advisory
  args: [--select, ALL]
```

## Differences

- **Version-dependent rules.** With `target-version` set, the rules that
  only apply to some Odoo versions follow it, as pylint-odoo's
  `valid-odoo-versions` does. Without it, they follow each module's
  manifest version. XML rules always follow the module's version, as
  `oca-checks-odoo-module` does. See
  [`target-version`](configuration.md#target-version).
- **`manifest-superfluous-key`** ([C8116](rules/C8116.md)) leaves
  `"installable": True` alone, as OCA writes it in every manifest.
  pylint-odoo's default is `keys-values-true = ["active", "installable"]`.
- **Suppression comments.** `# pylint: disable=...` and `# noqa` work in
  Python; `<!-- oca-hooks:disable=... -->` and `<!-- pylint:disable=... -->`
  turn checks off for a whole XML file. The `# lint-ignore` and
  `# oca-hooks:disable=...` comments of `oca-checks-odoo-module-fixit` in
  Python files are not read; use `# noqa: MOD009`. See
  [Suppressing a violation](configuration.md#suppressing-a-violation-in-the-code).
- **CSV files.** The `odoo-lint` hook's `files` pattern has no `.csv`, so
  a commit that only changes CSV files does not run it. The CSV checks
  ([MOD003](rules/MOD003.md), [MOD004](rules/MOD004.md)) run when another
  file of the module is staged, and on every `odl check` in CI.
- **Output.** The `text` format is pylint's default
  (`path:line:col: CODE: message (name)`, 0-based column); `msg-template`
  is not supported.
- **Formatting.** `odl format` re-indents lines indented by hand and drops
  blank lines right after an opening tag or before a closing one, where
  prettier keeps them. It does not break long selectors over lines. See
  [Formatting XML](format.md).
- **Parity.** The [parity page](parity.md) compares odoo-lint with each OCA
  tool on its own test repository, check by check. Please
  [report](https://github.com/bosd/odoo-lint/issues) any other difference.

## `.pre-commit-config.yaml`

Before, the OCA hooks that odoo-lint covers:

```yaml
repos:
  - repo: https://github.com/oca/maintainer-tools
    rev: bf9ecb9938b6a5deca0ff3d870fbd3f33341fded
    hooks:
      - id: oca-fix-manifest-website
        args: ["https://github.com/OCA/server-tools"]
      - id: oca-gen-addon-readme
        args:
          [
            --addons-dir=.,
            --branch=18.0,
            --org-name=OCA,
            --repo-name=server-tools,
          ]
      - id: oca-gen-external-dependencies
  - repo: https://github.com/OCA/odoo-pre-commit-hooks
    rev: v0.0.33
    hooks:
      - id: oca-checks-odoo-module
      - id: oca-checks-po
        args: [--disable=po-pretty-format]
  - repo: https://github.com/OCA/pylint-odoo
    rev: v9.1.3
    hooks:
      - id: pylint_odoo
        name: pylint with optional checks
        args: [--rcfile=.pylintrc, --exit-zero]
        verbose: true
      - id: pylint_odoo
        args: [--rcfile=.pylintrc-mandatory]
```

### Step 1: advisory

Add odoo-lint next to the existing hooks, without failing commits, and
compare its findings with pylint-odoo's for a while:

```yaml
- repo: https://github.com/bosd/odoo-lint-pre-commit
  rev: v0.1.0a17
  hooks:
    - id: odoo-lint-advisory
```

Add the `[tool.odoo-lint]` settings from above to `pyproject.toml`. Run
`odl check --diff` and `odl check --fix` to apply the safe fixes once over
the repository, and ignore or fix what is left.

### Step 2: enforcing

Replace the hooks odoo-lint covers:

```yaml
repos:
  - repo: https://github.com/oca/maintainer-tools
    rev: bf9ecb9938b6a5deca0ff3d870fbd3f33341fded
    hooks:
      - id: oca-gen-addon-readme
        args:
          [
            --addons-dir=.,
            --branch=18.0,
            --org-name=OCA,
            --repo-name=server-tools,
          ]
  - repo: https://github.com/bosd/odoo-lint-pre-commit
    rev: v0.1.0a17
    hooks:
      - id: odoo-lint
        args: [--fix]
      - id: odoo-lint-requirements
```

with, in `pyproject.toml`, the website `oca-fix-manifest-website` set:

```toml
[tool.odoo-lint.rules.manifest-website]
website = "https://github.com/OCA/server-tools"
```

Add `odoo-lint-format` too to replace prettier for XML and stylesheets, and
narrow prettier's `files` to the other types. In CI, `odl check` fails the
job on findings; see [Pre-commit, hk and CI](integrations.md#ci).
