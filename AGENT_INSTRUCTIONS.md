# Instructions for CLI Agent

Target: Build `odoo-lint` (CLI `odl`), a Rust linter for Odoo codebases.

Tasks:
1. Parse `[tool.odoo-lint]` settings from `pyproject.toml` in `src/config.rs`.
2. Complete `@api.depends` check in `src/rules/odoo001_missing_depends.rs` using `ruff_python_ast`.
3. Add unit tests under `tests/`.
