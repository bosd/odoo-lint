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

Odoo version the code targets, as a string such as `"16.0"`. Default: the
newest `Framework :: Odoo :: X` classifier in the `[project]` table of the
nearest `pyproject.toml` (even without a `[tool.odoo-lint]` table), or
`"17.0"`. A version from the classifiers is a default only: unlike
`target-version`, it does not win over the modules' own versions.

Rules that only apply to some Odoo versions (the upgrade rules `U1801`,
...) follow `target-version` when it is set, here or with `odl check --version`, as
pylint-odoo's `valid-odoo-versions` does. Without it, they follow each
module's own version, from its manifest (`19.0.1.0.0` is 19.0), and the
default for modules whose manifest version has no Odoo series (`1.0.0`).
XML rules always follow the module's version, as oca-checks-odoo-module
does.

### `select` and `ignore`

Which rules run. Each entry is one of:

- `ALL`, every rule except the [opt-in ones](#opt-in-rules);
- a code such as `C8101`, or a name such as `manifest-required-author`;
- a code prefix such as `C81`, `E` or `ODOO`.

A rule runs when it matches an entry in `select` and none in `ignore`.
`--select` on the command line replaces `select`; `--ignore` adds to `ignore`.
Default: `select = ["ALL"]`, no `ignore`.

Codes and names of rules ported from pylint-odoo are pylint-odoo's, so a list
copied from a `.pylintrc` works. Entries for pylint-odoo checks that odoo-lint
does not implement yet produce a warning, not an error.

### Opt-in rules

`ALL` leaves these out; select them by code or name. A code prefix such as
`XML` or `ODOO` selects them too.

| Rule                        | Why it is opt-in                                                                                 |
| --------------------------- | ------------------------------------------------------------------------------------------------ |
| [MOD008](rules/MOD008.md)   | Many projects require the copyright headers it reports (and removes)                             |
| [MOD009](rules/MOD009.md)   | [W8113](rules/W8113.md) reports the same redundant `string`                                      |
| [MOD011](rules/MOD011.md)   | Requires `readme/` fragments, a repository convention                                            |
| [MOD013](rules/MOD013.md)   | Requires tests in every module with models, a repository convention                              |
| [ODOO012](rules/ODOO012.md) | Suggests widgets, a style review rather than an error                                            |
| [PO007](rules/PO007.md)     | Style; every OCA repository disables it                                                          |
| [PO106](rules/PO106.md)     | Builds each module's template; complete only with the dependencies on the addons path            |
| [U1923](rules/U1923.md)     | Moves `SUPERUSER_ID` imports to `odoo.api`; `upgrade-check` runs it only when `select` names it  |
| [U2035](rules/U2035.md)     | Turns `_rec_names_search` lists into tuples; `upgrade-check` runs it only when `select` names it |
| [XML002](rules/XML002.md)   | Style; newer than the hooks OCA runs                                                             |
| [XML003](rules/XML003.md)   | Style; newer than the hooks OCA runs                                                             |
| [XML009](rules/XML009.md)   | Style; newer than the hooks OCA runs                                                             |
| [XML024](rules/XML024.md)   | Style; newer than the hooks OCA runs                                                             |
| [XML103](rules/XML103.md)   | Reports view extensions appended to a whole form, a review rule                                  |

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
[ODOO005](rules/ODOO005.md) for XML ids, [ODOO006](rules/ODOO006.md)
for models, [ODOO007](rules/ODOO007.md) for fields named in Python and
[ODOO008](rules/ODOO008.md) for inheritance specs against the views they
extend, [ODOO009](rules/ODOO009.md) for the fields of data records, and
[ODOO010](rules/ODOO010.md) and [ODOO011](rules/ODOO011.md) for the
methods that fields and buttons name, and [ODOO013](rules/ODOO013.md) for
fields whose comodel only administrators may read. The modules next to the checked module are found without it. A module whose dependencies cannot
all be found is not checked by these rules, so without `addons-path` they
stay silent rather than guess.

Only the dependencies of the checked modules are read: with all of Odoo 18.0
in the path, checking every core module takes about three seconds.

Before an upgrade, point `addons-path` at the Odoo of the target version:
ODOO008 then shows the `<xpath>`s that match nothing in its views.

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

### `format`

Line width and indentation of `odl format` ([Formatting XML](format.md)):

```toml
[tool.odoo-lint.format]
line-width = 88
indent-width = 4
```

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

### `rules.manifest-website`

The website every manifest must have; [MOD010](rules/MOD010.md) reports
another or a missing one, and `odl check --fix` sets it, as OCA's
`oca-fix-manifest-website` hook does:

```toml
[tool.odoo-lint.rules.manifest-website]
website = "https://www.example.com"
```

Without it the rule does nothing.

### `rules.po-translation-missing`

The languages every module with translatable terms must have a `.po` file
for; [PO105](rules/PO105.md) reports a missing file, and a file that leaves
terms of the module's `.pot` untranslated:

```toml
[tool.odoo-lint.rules.po-translation-missing]
languages = ["nl", "es"]
```

Without it the rule does nothing.

### `rules.module-icon`

The icon of the modules, and the outdated icons it replaces;
[MOD012](rules/MOD012.md) reports a module without an icon or with an
outdated one, and `odl check --fix` copies the configured icon. Other icons
(a module's own app icon) are left alone:

```toml
[tool.odoo-lint.rules.module-icon]
icon = "tools/icons/company.png"
replace = ["oca-2015", "oca-2025-01"]

[tool.odoo-lint.rules.module-icon.paths]
"modules/client/**" = "tools/icons/client.png"
```

`replace` takes `oca-2015` (OCA's grey cube), `oca-2025-01` (its first new
logo), `sha256:<hex>` or a file; `paths` gives the modules under a glob their
own icon (the longest matching glob wins). Paths are relative to the
configuration file. Without the table the rule does nothing.

### `rules.missing-depends`

[ODOO001](rules/ODOO001.md) reports stored computes without
`@api.depends`. Non-stored computes without dependencies are a deliberate
pattern (counts, SQL queries); report them too with:

```toml
[tool.odoo-lint.rules.missing-depends]
non-stored = true
```

### `readme`

The layout of the READMEs `odl readme` writes: `style`, `org`, `repo`,
`branch` and `remove-rst`. See [The `obs` style](readme.md#the-obs-style).

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

One default differs: [C8116](rules/C8116.md) leaves `"installable": True`
alone, as OCA writes it in every manifest. pylint-odoo's behaviour is
`keys-values-true = ["active", "installable"]`.

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

`all` instead of a rule list disables every rule. After a string that spans
several lines, `# noqa` on its last line also covers the line it starts on,
as in Ruff.

In XML and `.po` files:

| Comment                                    | Scope                                                |
| ------------------------------------------ | ---------------------------------------------------- |
| `<!-- noqa -->` after an element           | every rule, on this line                             |
| `<!-- noqa: XML002, ODOO006 -->` after one | these rules, on this line                            |
| `<!-- noqa: ... -->` on its own line       | that line and the next element's line                |
| `<!-- oca-hooks:disable=... -->`           | the whole file, as in OCA's hooks                    |
| `# noqa: PO005` in a `.po` entry           | that entry; `msgmerge` and `odl pot --merge` keep it |

The `.po` comment is a translator comment (`# `), not an extracted one
(`#. `), which a template update would replace.
