# Configuration

`odl` reads its settings from the nearest configuration file, searching from
the first linted path upwards:

1. `odoo-lint.toml`, with the settings at the top level;
2. `pyproject.toml`, with the settings under `[tool.odoo-lint]`.

In each directory `odoo-lint.toml` wins over `pyproject.toml`. A
`pyproject.toml` without a `[tool.odoo-lint]` table is skipped. Use
`odl check --config FILE` to skip the search.

Unknown keys are an error, so a typo such as `manifest_author` instead of
`manifest-required-author` is reported instead of silently ignored.

## Reference

```toml
[tool.odoo-lint]
# Odoo version to lint for; `odl check --version` overrides it.
target-version = "17.0"
# Rules to run and to skip, by code, name or code prefix.
select = ["ALL"]
ignore = ["ODOO001"]
# Extra paths to skip, relative to this file.
exclude = ["setup", "addons/legacy_*"]
# Where the dependencies of your modules are, Odoo's included.
addons-path = ["../odoo/odoo/addons", "../odoo/addons", "../oca/*"]

[tool.odoo-lint.per-file-ignores]
"*/tests/*" = ["missing-depends"]

[tool.odoo-lint.rules.manifest-required-author]
authors = ["Odoo Community Association (OCA)"]

[tool.odoo-lint.rules.manifest-required-author.mapping]
"acme_*" = "Acme Corp"
```

The same file as `odoo-lint.toml`:

```toml
target-version = "17.0"
select = ["ALL"]

[per-file-ignores]
"*/tests/*" = ["missing-depends"]

[rules.manifest-required-author]
authors = ["Odoo Community Association (OCA)"]

[rules.manifest-required-author.mapping]
"acme_*" = "Acme Corp"
```

### `target-version`

Odoo version the code targets, as a string such as `"16.0"`. Rules that only
apply to some Odoo versions are skipped for other versions. Default: `"17.0"`.

### `select` and `ignore`

Which rules run. Each entry is one of:

- `ALL`, every rule except the opt-in ones ([MOD008](rules/MOD008.md),
  which removes copyright headers);
- a code such as `C8101`, or a name such as `manifest-required-author`;
- a code prefix such as `C81`, `E` or `ODOO`.

A rule runs when it matches an entry in `select` and none in `ignore`.
`--select` on the command line replaces `select`; `--ignore` adds to `ignore`.
Default: `select = ["ALL"]`, no `ignore`.

Codes and names of rules ported from pylint-odoo are pylint-odoo's, so a list
copied from a `.pylintrc` works. Entries for pylint-odoo checks that odoo-lint
does not implement yet produce a warning, not an error.

### `exclude`

Glob patterns of files and directories to skip, relative to the directory of
the configuration file. A pattern without `/` also matches a bare file or
directory name anywhere. Directories such as `.git`, `.venv`, `node_modules`
and `__pycache__` are always skipped.

### `addons-path`

Folders with addons, as in Odoo's `addons_path`, relative to this file; a
glob such as `../oca/*` adds every folder it matches. `odl check
--addons-path` replaces it, relative to the working directory.

Checks across modules use it to look up what a module's `depends` reach:
[ODOO004](rules/ODOO004.md) for fields in views,
[ODOO005](rules/ODOO005.md) for XML ids and [ODOO006](rules/ODOO006.md)
for models. The modules next to
the checked module are found without it. A module whose dependencies cannot
all be found is not checked by these rules, so without `addons-path` they
stay silent rather than guess.

Only the dependencies of the checked modules are read: with all of Odoo 18.0
in the path, checking every core module takes about two seconds.

### `per-file-ignores`

A table of glob pattern to rules that are ignored in matching files, with the
same pattern and rule syntax as above.

### `rules.manifest-required-author`

Settings for [C8101](rules/C8101.md).

`authors`
: Authors of which at least one must be in the manifest; a string or a list.
Default: `"Odoo Community Association (OCA)"`.

`mapping`
: Table of module folder name or `prefix*` pattern to required author(s). An
exact name wins over a pattern; among patterns the longest prefix wins.

### `manifest-defaults`

Values that `odl check --fix` fills in when a manifest lacks a required key
(C8102, C8119) or its author (C8101), per module pattern:

```toml
[tool.odoo-lint.manifest-defaults]
"*" = { license = "AGPL-3" }
"acme_*" = { license = "LGPL-3", author = "Acme Corp", website = "https://acme.example" }
```

An exact module name wins over the longest matching `prefix*` pattern, which
wins over `*`, key by key. Values are strings, numbers, booleans or lists of
them. Without a configured value there is no fix: odoo-lint does not guess a
license or an author.

### Other rule options

Rules ported from pylint-odoo take the same options, under
`[tool.odoo-lint.rules.<rule name>]`, with the same defaults. For example:

```toml
[tool.odoo-lint.rules.license-allowed]
allowed = ["AGPL-3", "LGPL-3"]

[tool.odoo-lint.rules.manifest-version-format]
valid-odoo-versions = ["17.0"]
```

Each rule's page under [rules](rules/index.md) lists its options:
[C8102](rules/C8102.md), [C8103](rules/C8103.md), [C8105](rules/C8105.md),
[C8106](rules/C8106.md), [C8111](rules/C8111.md), [C8112](rules/C8112.md),
[C8114](rules/C8114.md), [C8115](rules/C8115.md), [C8116](rules/C8116.md),
[C8117](rules/C8117.md), [C8118](rules/C8118.md) and [C8119](rules/C8119.md).

## Suppressing a violation in the code

Both Ruff and pylint comments work, so existing pylint-odoo suppressions keep
working:

| Comment                                   | Scope                                |
| ----------------------------------------- | ------------------------------------ |
| `# noqa`                                  | every rule, on this line             |
| `# noqa: C8101, print-used`               | these rules, on this line            |
| `# pylint: disable=print-used` after code | this line                            |
| `# pylint: disable=print-used` on its own | until the end of the enclosing block |
| `# pylint: disable=...` at column 0       | until the end of the file            |
| `# pylint: disable-next=print-used`       | the next line                        |
| `# pylint: enable=print-used`             | ends an earlier `disable`            |

`all` instead of a rule list disables every rule.
