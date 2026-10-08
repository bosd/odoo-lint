# Benchmarks

```{image} _static/benchmark.svg
:alt: Bar chart of the timings below
:width: 640px
```

| Tool                             | Time (median of 5) |
| -------------------------------- | ------------------ |
| odoo-lint 0.1.0-alpha.1          | 0.10 s             |
| odoo-lint, one thread            | 0.77 s             |
| pylint 3.1.1 + pylint-odoo 9.1.3 | 25.8 s             |

Both lint all of [OCA/sale-workflow](https://github.com/OCA/sale-workflow)
18.0: 173 modules and 1333 Python files, measured on a 28-thread x86-64
Linux machine after one warm-up run.

- pylint runs with the repository's own `.pylintrc` and in one process, as
  OCA's pre-commit hook runs it (the hook sets `require_serial: true`), on
  the same files pre-commit passes it.
- odoo-lint runs with its default rules. That is more work than pylint does
  here: it also checks the 1268 `.po` files, and it runs rules the
  repository's `.pylintrc` does not enable.
- "One thread" is odoo-lint with `RAYON_NUM_THREADS=1`.

To reproduce, with pylint-odoo installed in a virtual environment:

```bash
python scripts/benchmark.py path/to/sale-workflow --pylint path/to/venv/bin/pylint
```

The script prints the timings and rewrites the chart in `docs/_static/`.
