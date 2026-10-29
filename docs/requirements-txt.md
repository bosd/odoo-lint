# requirements.txt

`odl requirements` writes the `requirements.txt` of a folder of addons: the
Python `external_dependencies` of its installable modules, sorted and
without duplicates. It is the file OCA's `oca-gen-external-dependencies`
hook writes, byte for byte, without building each module's package:

```bash
odl requirements                 # the addons in the current folder
odl requirements modules/custom  # another folder
odl requirements --check         # write nothing; exit 1 when out of date
```

- Odoo and Odoo addons (`odoo`, `odoo-addon-*`) are left out.
- Import names that are not the package's name take the package's, as whool
  does: `dateutil` is `python-dateutil`, `ldap` is `python-ldap`, ...
- A module's `pyproject.toml` can map a name itself, as for whool:

  ```toml
  [tool.whool.external_dependencies_override.python]
  zeep = "zeep==4.2.1"
  ```

- Without any requirement, the file goes.

## As a hook

With pre-commit, the `odoo-lint-requirements` hook of
[odoo-lint-pre-commit](integrations.md#pre-commit) runs `odl requirements`
when a manifest or `pyproject.toml` changes:

```yaml
- repo: https://github.com/bosd/odoo-lint-pre-commit
  rev: v0.1.0a18
  hooks:
    - id: odoo-lint-requirements
```

It replaces OCA's `oca-gen-external-dependencies`; see
[Migrating from OCA's tooling](migrating.md).
