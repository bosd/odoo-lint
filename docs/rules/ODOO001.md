<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# missing-depends (ODOO001)

Compute method referenced by `compute=` lacks `@api.depends`.

## What it does

Checks every model class for stored fields (`store=True`) declared with
`compute=` and reports the compute method when it has neither
`@api.depends(...)` nor `@api.depends_context(...)`.

Both `compute="_compute_total"` and `compute=_compute_total` are recognised.
When several fields share one compute method, the method is reported once and
the message lists all fields.

## Why is this bad?

Without declared dependencies the ORM does not know when to invalidate the
value. A stored computed field is never recomputed after its inputs change, so
the database silently keeps stale data.

A non-stored compute without dependencies is a common, deliberate pattern
(counts, SQL queries, values of the moment): Odoo computes it again in each
transaction. Those are left alone unless configured.

## Example

```python
class SaleOrder(models.Model):
    _inherit = "sale.order"

    margin_total = fields.Float(compute="_compute_margin_total", store=True)

    def _compute_margin_total(self):
        for order in self:
            order.margin_total = sum(order.order_line.mapped("margin"))
```

Use instead:

```python
    @api.depends("order_line.margin")
    def _compute_margin_total(self):
        for order in self:
            order.margin_total = sum(order.order_line.mapped("margin"))
```

If the value only depends on the context (for example the current company or
user), declare that with `@api.depends_context("company")`.

## Configuration

```toml
[tool.odoo-lint.rules.missing-depends]
# Report non-stored computes as well (default: false)
non-stored = true
```

## Limitations

Only compute methods defined in the same class as the field are checked; a
method inherited from another class is not reported.
