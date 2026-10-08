<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# module-large-file (ODOO003)

A file inside a module is larger than the limit (1 MiB by default).

## What it does

Reports files in a module larger than a limit, 1024 KiB by default.

## Why is this bad?

Large files bloat the repository, every checkout and every Odoo instance
that installs the module, and git keeps them forever, even after removal.
Compress images, keep demo data small, and store large assets elsewhere.

## Options

```toml
[tool.odoo-lint.rules.module-large-file]
max-kib = 2048
```
