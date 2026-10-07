<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# manifest-author (ODOO010)

`__manifest__.py` author does not match the configured author.

## What it does

Checks that the `author` key of every `__manifest__.py` matches the author
expected for that module. The expected author is chosen by the name of the
module folder, so one repository can enforce different authors for different
groups of modules.

## Why is this bad?

The manifest author is shown in the Apps menu and on the Odoo Apps store, and
OCA tooling relies on it. Inconsistent authors make it unclear who maintains a
module and break automated checks that filter on author.

## Configuration

```toml
[tool.odoo-lint.rules.manifest-author]
# Used when no pattern matches (defaults to the OCA)
default = "Odoo Community Association (OCA)"

[tool.odoo-lint.rules.manifest-author.mapping]
"acme_*" = "Acme Corp"
"acme_hr_*" = "Acme HR"
"acme_special" = "Someone Else"
```

An exact module name wins; otherwise the longest matching `prefix*` pattern
wins, then `default`. With the configuration above:

| Module folder    | Expected author                  |
| ---------------- | -------------------------------- |
| `acme_sale`      | Acme Corp                        |
| `acme_hr_leave`  | Acme HR                          |
| `acme_special`   | Someone Else                     |
| `sale_stock_ext` | Odoo Community Association (OCA) |

## Example

For a module folder `acme_sale`:

```python
{
    "name": "Acme Sale",
    "author": "Odoo Community Association (OCA)",
}
```

Use instead:

```python
{
    "name": "Acme Sale",
    "author": "Acme Corp",
}
```
