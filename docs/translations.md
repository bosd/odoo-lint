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
