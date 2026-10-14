# Formatting XML

`odl format` formats the XML files of a project with
[lax-markup](https://github.com/bartlomieju/lax), the formatter behind
`deno fmt`, which only moves whitespace: attribute values (domains,
`&lt;`, quotes), comments and text stay as written.

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

## Configuration

```toml
[tool.odoo-lint.format]
line-width = 88   # OCA's prettier printWidth
indent-width = 4
```

## As a hook

```yaml
- repo: local
  hooks:
    - id: odl-format
      name: Format XML
      entry: odl format --force-exclude
      language: system
      files: \.xml$
```
