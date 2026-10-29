# Usage

## `odl check`

Lint addons directories or single files.

```bash
odl check [PATHS]... [--version VERSION] [--config FILE]
          [--select RULES] [--ignore RULES] [--output-format FORMAT]
          [--fix] [--unsafe-fixes] [--diff] [--force-exclude] [-v] [-q]
          [--exit-zero] [--addons-path DIRS] [--statistics]
```

`PATHS`
: Directories or files to lint. Defaults to the current directory. The first
path is where the [configuration](configuration.md) search starts. A path
that does not exist is an error (exit code `2`); a file odoo-lint does not
lint (a `README.md`, say) is skipped.

`--version`, `--odoo-version VERSION`
: Target Odoo version, for example `17.0`. Overrides `target-version` from the
configuration. Without either, the Odoo version of the project's
`Framework :: Odoo :: X` classifier is used, else `17.0`.

`--config FILE`
: Use this `odoo-lint.toml` or `pyproject.toml` instead of searching for one.
Its folder is the project root that `exclude` and `per-file-ignores` patterns
are relative to, wherever odoo-lint runs from.

`--addons-path DIRS`
: Comma-separated folders with the addons the checked modules depend on, for
the checks across modules; replaces `addons-path` from the configuration.

`--select RULES`
: Comma-separated codes, names or code prefixes to run; replaces `select`
from the configuration. A selector that matches no rule is an error here (a
typo would turn the check off); in the configuration it is a warning, so a
list copied from a `.pylintrc` works.

`--ignore RULES`
: Comma-separated rules to skip; added to `ignore` from the configuration.

`--output-format FORMAT`
: `text` (default), `json`, `github`, `sarif` or `gitlab`.

`--fix`
: Apply the safe fixes, then report what is left. See [Fixes](#fixes).

`--unsafe-fixes`
: With `--fix` or `--diff`, also apply the unsafe fixes.

`--diff`
: Show the fixes as a unified diff instead of writing them. Exits with `1`
when a file would change.

`--force-exclude`
: Apply `exclude` from the configuration to files and folders given on the
command line too. Git hooks pass the changed files one by one; without this flag, an
excluded file is linted when it is named explicitly.

`--exit-zero`
: Exit with `0` even when there are violations, so a hook or CI job reports
them without failing. Invalid configuration or usage still exits with `2`.

`-v`, `--verbose`
: Also print the Odoo version the rules follow and the configuration used.

`-q`, `--quiet`
: Print only the findings, without the summary line.

`--statistics`
: Count the findings per rule instead of listing them, the most frequent
first; `[*]` marks the rules `--fix` fixes. With `--output-format json`, an
array of objects with `code`, `name`, `count` and `fixable`.

```text
12	W8113	[*] attribute-string-redundant
 3	C8101	    manifest-required-author
```

Findings, diffs and reports go to stdout, the summary of `odl check` too;
warnings, errors and the progress of commands that write files (`odl format`,
`odl readme`, the summary of `--diff`) go to stderr. So
`odl check --diff > fixes.patch` gives a patch, and a JSON or SARIF report
on stdout stays valid.

With an addons path, `odl check` warns about what makes the checks across
modules skip a module: an addons path folder that does not exist, one that
holds no module while a folder below it does (the Odoo checkout itself
instead of its `addons` and `odoo/addons`), and dependencies it cannot find.
A folder without any module, such as a repository with none for this Odoo
version yet, is fine.

### Output formats

`text`
: pylint's default format, so existing tooling keeps working. The column is
0-based, as in pylint:

```text
addons/acme_sale/__manifest__.py:3:4: C8101: One of the following authors must be present in manifest: 'Acme Corp' (manifest-required-author)
```

On a terminal the path is bold and the code red; `NO_COLOR` turns the
colours off. Piped output, as in CI or pre-commit, has no colours, even with
`FORCE_COLOR` set.

`json`
: An array of objects with `file_path`, `line`, `column` (both 1-based),
`code`, `name` and `message`, and `fix` (`applicability` and `title`) when
the violation can be fixed.

`github`
: GitHub Actions annotations, shown inline on pull requests. `E` and `F`
codes are errors, everything else is a warning.

```yaml
- run: odl check --output-format github
```

`sarif`
: [SARIF 2.1.0](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html),
for GitHub code scanning, reviewdog (Forgejo, Gitea) and IDE viewers. Every
rule links to its documentation page.

`gitlab`
: A GitLab Code Quality report, shown in merge requests.

See [CI integration](integrations.md) for complete workflows.

### Fixes

Some rules can fix what they report; their pages have a _Fix safety_
section. As in Ruff, a fix is either:

safe
: It keeps the behaviour of the code, or only changes formatting, such as
`self._cr` to `self.env.cr` or sorting a `.po` file. `--fix` applies these.

unsafe
: It is probably right but needs review, such as adding `return` before a
trailing `super()` call or copying a translation into the `.pot` template.
These need `--unsafe-fixes` as well.

```bash
odl check --diff                  # review the safe fixes
odl check --fix                   # apply them
odl check --fix --unsafe-fixes    # apply the unsafe ones too
```

Fixes are applied in passes until nothing is left to fix, so a fix that
enables another one (a duplicate merged, then the file sorted) is completed in
one run. Running `--fix` twice changes nothing the second time. Fixes to
translation files are described in [Translations](translations.md).

### Exit codes

| Code | Meaning                                |
| ---- | -------------------------------------- |
| `0`  | No violations found (or `--exit-zero`) |
| `1`  | One or more violations found           |
| `2`  | Invalid configuration or usage         |

## `odl rule`

```bash
odl rule                            # list all rules
odl rule C8101                      # explain a rule by code
odl rule manifest-required-author   # or by name
odl rule W83                        # list the rules of a code prefix
odl rule 8101                       # or with a number (C8101, E8101)
```

The output is the same as the pages under [rules](rules/index.md).

## `odl upgrade-check`

What modules need to run on a newer Odoo version, per version step. See
[Upgrades](upgrades.md).

## `odl badge`

README badges: the share of clean modules, or readiness for an Odoo version.
See [README badges](integrations.md#readme-badges).

## `odl pot`

Writes each module's translation template (`i18n/<module>.pot`) from its
source, and with `--merge` updates its `.po` files. See
[Generating the template](translations.md#generating-the-template-odl-pot).

## `odl server`

A language server for editors. See [Editors](editors.md).

## `odl mcp` and `odl hook`

For AI coding agents: a Model Context Protocol server, and a post-edit hook.
See [AI coding agents](ai.md).

## In pre-commit

[odoo-lint-pre-commit](https://github.com/bosd/odoo-lint-pre-commit) installs
the prebuilt wheel, with a tag for every release:

```yaml
repos:
  - repo: https://github.com/bosd/odoo-lint-pre-commit
    rev: v0.1.0a17
    hooks:
      - id: odoo-lint
        # args: [--fix]   # also apply the safe fixes
```

It lints the staged `.py`, `.xml`, `.po` and `.pot` files, with `--force-exclude`,
and fails the commit when it finds something. To try odoo-lint next to your
other linters first, use `odoo-lint-advisory` instead: it shows the findings
on every commit (`--exit-zero`, `verbose`) but never fails it.
Checks across a module's files read the other files from disk.

## In hk

[hk](https://hk.jdx.dev) runs the step on the staged files, and `hk fix` (or
the pre-commit hook) applies the safe fixes. Use the version of your hk in
the `amends` line.

```text
amends "package://github.com/jdx/hk/releases/download/v1.10.4/hk@1.10.4#/Config.pkl"

local odoo_lint = new Step {
  glob = List("**/*.py", "**/*.xml", "**/*.po", "**/*.pot")
  check = "odl check --force-exclude {{files}}"
  fix = "odl check --fix --force-exclude {{files}}"
}

hooks {
  ["pre-commit"] {
    fix = true
    stash = "git"
    steps { ["odoo-lint"] = odoo_lint }
  }
  ["check"] { steps { ["odoo-lint"] = odoo_lint } }
  ["fix"] {
    fix = true
    steps { ["odoo-lint"] = odoo_lint }
  }
}
```

Only the staged files are linted; checks across files of a module (such as a
`.po` against its `.pot`) see the other files as they are on disk.
