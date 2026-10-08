# Upgrades

`odl upgrade-check` shows what your modules need to run on a newer Odoo
version, before you start the migration:

```bash
odl upgrade-check --target 19.0
```

```text
acme_sale (17.0 → 19.0)
  18.0  W8161   prefer-env-translation                     12  12 automatic
  18.0  XML015  xml-deprecated-oe-chatter                   1  1 by hand
  19.0  W8165   deprecated-self-cr                          3  3 automatic
  19.0  XML016  xml-deprecated-res-groups-category-id       1  1 to review
  17 changes: 15 automatic, 1 to review, 1 by hand

12 modules checked for Odoo 19.0: 11 ready, 1 to upgrade (17 changes: 15 automatic, 1 to review, 1 by hand).
```

## What it checks

Every rule that starts applying at a version, such as `name_get` in 17.0 or
`self._cr` in 19.0, is an upgrade step. For each module, the report lists
the findings of the steps after the module's own version (the series in its
manifest's `version`, e.g. `17.0.1.0.0`) up to the target, grouped by step:

automatic
: `--fix` changes it, and the change keeps the behaviour.

to review
: `--fix --unsafe-fixes` proposes a change that may alter behaviour; check
it.

by hand
: No automatic fix.

A module whose manifest has no version, or a short one like `1.0`, is taken
to be on the `target-version` of your configuration, as Odoo does. A module
with a malformed version gets every step up to the target.

The breakdown is deliberately not a percentage: one renamed field can be
more work than fifty `t-esc`. The counts say what can be automated and what
needs a developer.

## Upgrade rules

Rules whose code starts with `U` describe what changed in one Odoo version:
`U18xx` for 18.0, `U19xx` for 19.0, and so on. They use Odoo's own code-upgrade scripts and the
differences between Odoo's branches as their source, and have a fix where the
change is mechanical. `odl upgrade-check` uses them automatically; to check
one step directly:

```bash
odl check --select U18 --version 18.0
```

They apply to modules on that version or newer: a 17.0 module with `<tree>`
views is fine, an 18.0 module with them fails to install. See the
[rules](rules/index.md) for the full list.

## Applying the changes

```bash
odl upgrade-check --target 19.0 --diff                 # review the automatic part
odl upgrade-check --target 19.0 --fix                  # apply it
odl upgrade-check --target 19.0 --fix --unsafe-fixes   # and the part to review
```

The fixes are those of the upgrade steps only; `odl check` remains the place
for everything else.

## Options

`--target VERSION`
: The Odoo version to upgrade to. Required.

`--show-findings`
: List every finding under its module.

`--output-format json`
: The report as JSON: per module the counts, per rule the step and counts,
and the findings.

`--ignore RULES`
: Leave rules out of the report, on top of `ignore` from the configuration.

The command exits with `1` when anything needs to change, so a CI job can
show whether a repository is ready for the next version.

For AI coding agents, the MCP server has the same check as the
`upgrade_check` tool.
