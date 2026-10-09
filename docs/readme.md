# Module READMEs

`odl readme` builds a module's documentation from the Markdown fragments in
its `readme/` folder, the way the OBS client template does:

- `README.md`: the fragments joined, each under a heading;
- `static/description/index.html`: that README rendered, the page Odoo shows
  for the module.

```bash
odl readme modules/custom          # every module with a readme/ folder
odl readme modules/custom/acme_sale/readme/USAGE.md   # the module of a file
odl readme --check modules/custom  # write nothing; exit 1 when out of date
```

Only modules with a `readme/` folder are touched, and a file is only written
when it changes.

## Fragments

| File | Heading |
| --- | --- |
| `DESCRIPTION.md` | Introduction |
| `FEATURES.md` | Features |
| `INSTALL.md` | Installation |
| `CONFIGURE.md` | Configuration |
| `USAGE.md` | Usage |
| `CONTEXT.md` | Context |
| `HISTORY.md` | History |
| `ROADMAP.md` | Roadmap |
| `CONTRIBUTORS.md` | Contributors |
| `CREDITS.md` | Credits |

Empty or missing fragments are left out.

## Same output, much faster

The output is byte for byte what the template's Python generator
(`convert_readme2html.py`) writes. It uses the same Markdown engine
([comrak](https://github.com/kivikakk/comrak) 0.54, with GitHub tables,
task lists, footnotes, strikethrough, superscript, description lists and
smart punctuation) and the same page. On the 86 documented modules of a
client repository it takes 0.06 seconds; the Python generator starts a `uv
run` per module.

Mermaid diagrams (` ```mermaid ` blocks) become `<div class="mermaid">`,
which the page renders with mermaid.js. The Python generator switches to
another Markdown engine for documents with a diagram; `odl readme` renders
them like every other document, so moving over changes their punctuation
once (`'quotes'` become typographic quotes, `...` becomes `…`).

## As a hook

In `hk.pkl`:

```text
["readme"] {
  glob = "modules/custom/**/readme/**/*.md"
  fix = "odl readme {{files}}"
}
```

or with pre-commit:

```yaml
- repo: local
  hooks:
    - id: odl-readme
      name: Module READMEs
      entry: odl readme
      language: system
      files: /readme/.*\.md$
```
