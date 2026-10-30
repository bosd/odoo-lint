# Translations not showing up?

Odoo's translation loader (`odoo/tools/translate.py`) silently skips more
than you would expect. This page explains why a translation does not appear,
which odoo-lint rule detects it, and how to reload translations. It is based
on the Odoo 18.0 and 19.0 sources.

## How Odoo reads a `.po` file

For a language such as `nl_BE`, Odoo reads `i18n/nl.po`, `i18n/nl_BE.po` and
the same names in `i18n_extra/`, in that order: the more specific file wins.

1. **Merge with the template.** When the module has an `i18n/<module>.pot`,
   Odoo first merges the `.po` file with it, like `msgmerge`. Entries whose
   `msgid` is not in the template become obsolete and are skipped, and the
   `#.` comments and `#:` references are taken from the template.
2. **Module comment.** Every entry needs `#. module: <module>`; without it
   reading the file fails.
3. **References.** `#: model:...` and `#: model_terms:...` entries translate
   records and views; `#: code:...` entries translate `_()` in Python and
   JavaScript. Other references are ignored with an error in the log.
4. **Empty and fuzzy.** Empty translations are skipped. Fuzzy ones are _not_:
   Odoo loads them as if they were reviewed.

| Symptom                                                   | Cause                                                          | Rule                                             |
| --------------------------------------------------------- | -------------------------------------------------------------- | ------------------------------------------------ |
| New translation never shows                               | `msgid` missing from the outdated `.pot`                       | [PO101](rules/PO101.md)                          |
| `malformed po file: unknown occurrence` in the log        | reference Odoo cannot read, often in the `.pot`                | [PO102](rules/PO102.md)                          |
| Code translations of a module all missing in one language | `code:` reference without line number, or missing `#. module:` | [PO102](rules/PO102.md), [PO002](rules/PO002.md) |
| A whole language file is ignored                          | file not named after a language code                           | [PO103](rules/PO103.md)                          |
| Unreviewed text shown to users                            | `#, fuzzy` translation                                         | [PO104](rules/PO104.md)                          |
| Translation file fails to load                            | syntax error                                                   | [PO001](rules/PO001.md)                          |
| One of two translations lost on the next export           | duplicate `msgid` or `model:` reference                        | [PO005](rules/PO005.md), [PO006](rules/PO006.md) |

## Fixing translation files

`odl check --fix` repairs what can be repaired mechanically: it adds missing
`#. module:` comments, adds `:0` to `code:` references, merges identical
duplicates and sorts the file the way OCA's `po-pretty-format` does. Review
with `--diff` first.

`--unsafe-fixes` also copies translations whose `msgid` is missing from the
`.pot` into the template, untranslated and with only the references Odoo can
read. Entries that only have references Odoo cannot read, or that label a
field the template already labels differently, are left alone: they are
translations of old texts. Regenerating the `.pot` (below) is the better
fix: it keeps those translations when their text is still in the code.

## Generating the template: `odl pot`

`odl pot` writes `i18n/<module>.pot` the way Odoo's export does, without a
database: the terms of the code (`_()`, `_lt()`, `_t()`, QWeb templates,
spreadsheet dashboards), of the models (descriptions, field labels and help,
selections, constraint messages) and of the data files (records, menus,
views, mail templates, CSV rows). `--merge` then brings the module's `.po`
files in line with it, as `msgmerge` does:

```console
$ odl pot my_module --merge
```

- Translations of messages the template still has are kept, with their
  flags and translator comments; new messages come untranslated.
- Translations of removed messages stay as obsolete entries (`#~`), which Odoo
  ignores; `--no-obsolete` drops them.
- `--check` writes nothing and exits with 1 when a template or `.po` file is
  out of date, for CI.
- The first merge usually rewrites every `.po` file: references follow the
  template, and entries are sorted by `msgid` as Odoo exports them. A `.po`
  file that does not parse is left alone and reported.

Labels and descriptions come from the module's dependencies: put them on the
addons path (`addons-path` in the configuration, or `--addons-path`), Odoo's
own addons included. Without them `odl pot` says which templates may miss
labels. What a module's records get follows Odoo's install order, so a field
another module adds later is in that module's template, not in this one.

To reproduce OCA's templates, which its CI exports from a database with every
module of the repository installed and nothing installed automatically:

```console
$ odl pot . --installed . --skip-auto-install
```

In the editor and in `odl check`, the opt-in rule
[PO106](rules/PO106.md) reports a template that differs from what `odl pot`
writes, with what is installed as the plain command has it: the module, its
dependencies and what installs automatically.

Demo data counts as installed as Odoo's default says: up to 18.0 it is, from
19.0 it is not; `--with-demo` and `--without-demo` override that.

Some terms only exist once Odoo runs, and `odl pot` cannot see them:
selections computed by a method or taken from a library, models built while
the registry loads (such as `spec_driven_model` in l10n-brazil), and records
created by hooks or company setup (charts of accounts). On OCA's 18.0 and
19.0 repositories the generated templates match OCA's own for 96% and 99% of
their entries; most of the rest are OCA templates older than their code.

## Reloading translations

Even a correct file only shows up once Odoo reloads it, and that differs per
kind of translation:

`_()` in Python and JavaScript (`#: code:`)
: Read from the `.po` files on first use and cached per worker process.
**Restart the server**; updating the module does not reload them.

Records and views (`#: model:` and `#: model_terms:`)
: Stored in the database when the module is installed or updated. An update
does **not** overwrite translations that already exist: use
`odoo-bin -u <module> --i18n-overwrite`, or Settings > Translations > Import
with _Overwrite Existing Terms_. Records loaded with `noupdate="1"` are only
overwritten by a forced import.

The language itself must be installed and active, or nothing is loaded for
it.
