# Formatting XML and stylesheets

`odl format` formats the XML files of a project with
[lax-markup](https://github.com/bartlomieju/lax), the formatter behind
`deno fmt`, and its SCSS, CSS and Less with lax-css. Both only move
whitespace: attribute values (domains, `&lt;`, quotes), comments, text and
declarations stay as written. Libraries a module ships as they are
(`static/lib`, `static/src/lib`) are left alone.

```bash
odl format                  # every XML file under the current folder
odl format views/sale.xml   # one file
odl format --check          # write nothing; exit 1 when a file is not formatted
```

In Odoo XML, whitespace is not always layout. A record's `<field>` is
loaded as written, surrounding spaces and line breaks included, and QWeb
renders the spaces between inline elements. So after formatting, odl reads
the file again and compares it with the original: the elements, the
attributes, the comments word by word, every text that is more than the
indentation between elements, and the whitespace of record fields and
`<value>`s. When anything differs it leaves the file as it is and says so.
(lax-markup 0.3 moves a `<![CDATA[...]]>` that fills a field onto a line of
its own; such files are left alone.)

Compared with prettier and `@prettier/plugin-xml` in OCA's configuration
(`xmlWhitespaceSensitivity: "preserve"`), lax-markup also re-indents lines
that were indented by hand and drops a blank line right after an opening
tag or before a closing one. On OCA repositories formatted by prettier that
is a one-time change of a few lines per file; on OBSNL/obs-modules none.

## Stylesheets

lax-css indents rules and puts each declaration on a line of its own. odl
checks that only whitespace moved where it does not matter: between tokens,
around `{`, `}`, `;` and `,`, after `:` and inside parentheses. A space
before `:` or `(` (`a :hover`, `url (x)`) or between two selectors stays.
The one other change is the `;` it adds after a block's last declaration,
as prettier does.
Unlike prettier it does not break long selectors over lines, and it joins
those prettier broke inside `:not(...)`. On OCA/web 18.0 that changed 7 of
54 stylesheets, by 51 lines.

## Configuration

```toml
[tool.odoo-lint.format]
line-width = 88   # OCA's prettier printWidth
indent-width = 4
```

## As a hook

With pre-commit, the `odoo-lint-format` hook of
[odoo-lint-pre-commit](integrations.md#pre-commit) runs
`odl format --force-exclude` on the staged `.xml`, `.scss`, `.css` and
`.less` files:

```yaml
- repo: https://github.com/bosd/odoo-lint-pre-commit
  rev: v0.1.0a18
  hooks:
    - id: odoo-lint-format
```

In hk, as a step next to [the odoo-lint step](integrations.md#hk):

```text
["odl-format"] {
  glob = List("**/*.xml", "**/*.scss", "**/*.css", "**/*.less")
  check = "odl format --check --force-exclude {{files}}"
  fix = "odl format --force-exclude {{files}}"
}
```
