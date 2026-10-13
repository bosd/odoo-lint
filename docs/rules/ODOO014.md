<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# action-read (ODOO014)

An action is read with `env.ref(...).read()`, which only administrators may do.

Applies to Odoo 14.0 and later.

## What it does

Reports `read()` on an action taken with `env.ref()`:
`self.env.ref("sale.action_orders").read()[0]`, also through a variable.

## Why is this bad?

Only `base.group_system` may read `ir.actions.act_window`,
`ir.actions.server` and the other action models: for every other user the
button that returns the action raises an access error. Since Odoo 14.0
`env["ir.actions.actions"]._for_xml_id("sale.action_orders")` returns the
action as the web client needs it, for every user.

## Example

```python
def action_view_orders(self):
    action = self.env.ref("sale.action_orders").read()[0]
    action["domain"] = [("partner_id", "=", self.id)]
    return action
```

Use instead:

```python
def action_view_orders(self):
    action = self.env["ir.actions.actions"]._for_xml_id("sale.action_orders")
    action["domain"] = [("partner_id", "=", self.id)]
    return action
```

## Fix safety

Unsafe: `_for_xml_id` returns the fields the web client reads, not all of
them, so code that uses another field of the action needs a look. There is
a fix for `.read()[0]` only; the `env.ref()` line goes when nothing else
uses its variable.

## Limitations

An XML id counts as an action when its name or the variable's contains
`action`.
