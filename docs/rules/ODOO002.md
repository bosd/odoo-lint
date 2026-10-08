<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# module-unwanted-file (ODOO002)

A package, archive, executable, database dump or video inside a module.

## What it does

Reports files with an extension that does not belong in an Odoo module:
packages (`.rpm`, `.deb`), archives (`.zip`, `.tar.gz`, `.7z`…),
executables (`.exe`, `.dll`, `.jar`), database dumps (`.dump`, `.sql`,
`.backup`), disk images and videos.

## Why is this bad?

Such files end up in the repository and in every installation of the module.
They are usually committed by accident, and some (dumps) hold customer data.

## Options

```toml
[tool.odoo-lint.rules.module-unwanted-file]
extensions = ["rpm", "deb", "zip", "sql"]  # replaces the default list
```
