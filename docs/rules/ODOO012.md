<!-- Generated from the rule sources; run `UPDATE_DOCS=1 cargo test --test generated_docs` -->

# view-field-without-widget (ODOO012)

A view shows a field without the widget that suits it.

## What it does

Reports fields in form and list views that have no `widget` while one
suits their type, the way a reviewer would:

| Field | Widget |
| --- | --- |
| `Many2many` (without an embedded list or form) | `many2many_tags` |
| `Many2one` to `res.users` | `many2one_avatar_user` |
| `Selection` `state` in a form's `<header>` | `statusbar` |
| `Selection` `priority` | `priority` |
| `Integer` `color` | `color_picker` |
| `Char`/`Text` `domain` or `*_domain` | `domain` |

Hidden fields (`invisible="1"`, `column_invisible="1"`) are left alone.

## Why is this bad?

It works, but reads worse: a many2many as a list of ids, a state as a plain
selection, a domain as raw text. Odoo's own views use these widgets.

## Configuration

Opt-in: select it (`select = ["ALL", "ODOO012"]`), as it is a style
review rather than an error. It needs the field types of the dependencies,
Odoo's included, in `addons-path`.

## Fix safety

Unsafe: the fix adds the `widget` attribute, which changes how the view
looks; a domain widget may want `options="{'model': ...}"` as well.
