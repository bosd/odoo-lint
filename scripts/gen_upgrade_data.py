"""Generate the rename and removal data of the upgrade rules (U1590...).

Reads checkouts of OCA/OpenUpgrade (one per branch, the ``openupgrade_scripts``
folder is enough) and writes ``src/rules/upgrade/data/v<major>.tsv``: what
Odoo renamed and removed between the previous version and that one.

- renamed models: ``apriori.renamed_models`` and ``openupgrade.rename_models()``
  calls in the migration scripts; merged models: ``apriori.merged_models``;
- renamed fields: ``openupgrade.rename_fields()`` calls;
- renamed XML ids: ``openupgrade.rename_xmlids()`` calls;
- removed models and fields: the ``obsolete model`` and ``DEL`` lines of the
  ``upgrade_analysis.txt`` files, minus what another module adds back
  (``NEW``), what a rename explains, and what the Odoo sources of that
  version still define.

Only facts are kept (names), no code. Usage:

    python scripts/gen_upgrade_data.py OPENUPGRADE ODOO 15 16 17 18 19

where ``OPENUPGRADE/<major>.0`` is a checkout of that OpenUpgrade branch and
``ODOO/odoo<major>`` one of odoo/odoo (its Python files are enough).
"""

import ast
import re
import subprocess
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "src" / "rules" / "upgrade" / "data"

FIELD_LINE = re.compile(r"^(\S+)\s*/\s*(\S+)\s*/\s*(\S+) \(([^)]*)\)\s*: (.*)$")
MODEL_LINE = re.compile(r"^(obsolete|new) model (\S+)")


def literal(node, assignments):
    """The value of a literal, or of a module-level name bound to one."""
    if isinstance(node, ast.Name) and node.id in assignments:
        node = assignments[node.id]
    try:
        return ast.literal_eval(node)
    except (ValueError, TypeError, SyntaxError, RecursionError):
        return None


def script_calls(path):
    """``openupgrade.<name>(...)`` calls of a migration script: (name, args)."""
    try:
        tree = ast.parse(path.read_text(encoding="utf-8"))
    except (SyntaxError, UnicodeDecodeError):
        return
    assignments = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign) and len(node.targets) == 1:
            target = node.targets[0]
            if isinstance(target, ast.Name):
                assignments[target.id] = node.value
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        func = node.func
        name = (
            func.attr if isinstance(func, ast.Attribute) else getattr(func, "id", None)
        )
        if (
            name in {"rename_fields", "rename_models", "rename_xmlids"}
            and len(node.args) >= 2
        ):
            value = literal(node.args[1], assignments)
            if isinstance(value, list | tuple):
                yield name, value


def apriori_models(root, name):
    path = root / "openupgrade_scripts" / "apriori.py"
    if not path.exists():
        return {}
    tree = ast.parse(path.read_text(encoding="utf-8"))
    for node in tree.body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1:
            target = node.targets[0]
            if isinstance(target, ast.Name) and target.id == name:
                value = literal(node.value, {})
                if isinstance(value, dict):
                    return {k: v for k, v in value.items() if k and v and k != v}
    return {}


def odoo_definitions(odoo):
    """The models (``_name``) and fields (model, name) the sources define."""
    models, fields = set(), set()
    for path in odoo.rglob("*.py"):
        if "/tests/" in str(path) or "/static/" in str(path):
            continue
        try:
            tree = ast.parse(path.read_text(encoding="utf-8"))
        except (SyntaxError, UnicodeDecodeError, ValueError):
            continue
        for node in ast.walk(tree):
            if not isinstance(node, ast.ClassDef):
                continue
            names = set()
            defined = []
            for stmt in node.body:
                if not isinstance(stmt, ast.Assign) or len(stmt.targets) != 1:
                    continue
                target = stmt.targets[0]
                if not isinstance(target, ast.Name):
                    continue
                if target.id in {"_name", "_inherit"}:
                    value = literal(stmt.value, {})
                    values = (
                        [value]
                        if isinstance(value, str)
                        else value
                        if isinstance(value, list)
                        else []
                    )
                    names.update(v for v in values if isinstance(v, str))
                    if target.id == "_name" and isinstance(value, str):
                        models.add(value)
                elif isinstance(stmt.value, ast.Call):
                    func = stmt.value.func
                    if (
                        isinstance(func, ast.Attribute)
                        and isinstance(func.value, ast.Name)
                        and func.value.id == "fields"
                    ):
                        defined.append(target.id)
            fields.update((model, field) for model in names for field in defined)
    return models, fields


def generate(root, odoo, major):
    scripts = root / "openupgrade_scripts" / "scripts"
    models = apriori_models(root, "renamed_models")
    merged = {
        k: v
        for k, v in apriori_models(root, "merged_models").items()
        if k not in models
    }
    fields = {}
    xmlids = {}
    for path in sorted(scripts.glob(f"*/{major}.0.*/*.py")):
        for name, values in script_calls(path):
            for item in values:
                if not isinstance(item, list | tuple) or not all(
                    isinstance(v, str) for v in item
                ):
                    continue
                if name == "rename_models" and len(item) == 2 and item[0] != item[1]:
                    models.setdefault(item[0], item[1])
                elif name == "rename_fields" and len(item) >= 4 and item[2] != item[3]:
                    fields.setdefault((item[0], item[2]), item[3])
                elif name == "rename_xmlids" and len(item) == 2 and item[0] != item[1]:
                    xmlids.setdefault(item[0], item[1])
    deleted, added = set(), set()
    obsolete, new = set(), set()
    for path in sorted(scripts.glob(f"*/{major}.0.*/upgrade_analysis.txt")):
        for line in path.read_text(encoding="utf-8").splitlines():
            if match := MODEL_LINE.match(line):
                (obsolete if match[1] == "obsolete" else new).add(match[2])
            elif match := FIELD_LINE.match(line):
                _, model, field, _, change = match.groups()
                if change.startswith("DEL"):
                    deleted.add((model, field))
                elif change.startswith("NEW"):
                    added.add((model, field))
    defined_models, defined_fields = odoo_definitions(odoo)
    removed_models = obsolete - new - set(models) - set(merged) - defined_models
    removed_fields = {
        (model, field)
        for model, field in deleted - added - defined_fields
        if model not in removed_models
        and model not in models
        and model not in merged
        and (model, field) not in fields
    }
    # What the sources still define is no rename either.
    models = {old: new for old, new in models.items() if old not in defined_models}
    fields = {key: new for key, new in fields.items() if key not in defined_fields}
    # A field renamed in a renamed model is known under the old model too.
    rows = [f"model\t{old}\t{new}" for old, new in sorted(models.items())]
    rows += [f"model-merged\t{old}\t{new}" for old, new in sorted(merged.items())]
    rows += [f"model-removed\t{model}" for model in sorted(removed_models)]
    rows += [
        f"field\t{model}\t{old}\t{new}" for (model, old), new in sorted(fields.items())
    ]
    rows += [
        f"field-removed\t{model}\t{field}" for model, field in sorted(removed_fields)
    ]
    rows += [f"xmlid\t{old}\t{new}" for old, new in sorted(xmlids.items())]
    sha = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "--short", "HEAD"],
        capture_output=True,
        text=True,
        check=False,
    ).stdout.strip()
    header = (
        f"# What Odoo {major}.0 renamed and removed, from the migration scripts and\n"
        f"# analysis of OCA/OpenUpgrade {major}.0 ({sha}). Generated by\n"
        "# scripts/gen_upgrade_data.py; do not edit.\n"
    )
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / f"v{major}.tsv").write_text(
        header + "\n".join(rows) + "\n", encoding="utf-8"
    )
    print(
        f"{major}.0: {len(models)} models renamed, {len(removed_models)} removed; "
        f"{len(merged)} merged; {len(fields)} fields renamed, "
        f"{len(removed_fields)} removed; "
        f"{len(xmlids)} XML ids renamed"
    )


def main():
    openupgrade, odoo = Path(sys.argv[1]).expanduser(), Path(sys.argv[2]).expanduser()
    for major in sys.argv[3:]:
        generate(openupgrade / f"{major}.0", odoo / f"odoo{major}", int(major))


if __name__ == "__main__":
    main()
