"""Compare ``odl pot`` with the templates OCA's bot exports from Odoo.

OCA regenerates each module's ``i18n/<module>.pot`` with Odoo's own export
after every merge, so a checkout of OCA repositories is a large set of
reference templates. This script runs ``odl pot`` on those modules into a
temporary folder and compares the entries as ``(msgid, reference)`` pairs,
per kind of reference (``code``, ``model:ir.model.fields,field_description``,
...). Every repository of the corpus is on the addons path, with the folders
given in ``--addons-path`` (Odoo's addons). Like OCA's CI, which exports
from a database with every module of the repository installed, each run
counts the whole repository as installed (``odl pot --installed``), and
nothing installs automatically (``--skip-auto-install``).

Usage::

    python scripts/pot_parity.py ~/oca/19.0 --addons-path ~/odoo/addons
    python scripts/pot_parity.py ~/oca/19.0 --kind code: --samples 20
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ODL = ROOT / "target" / "release" / "odl"

# Odoo itself and OpenUpgrade: their templates do not come from OCA's bot.
SKIPPED = {"OCB", "OpenUpgrade"}

ENTRY = re.compile(r"\n\n(?=#)")


def pairs(text: str, kinds: tuple[str, ...]) -> set[tuple[str, str]]:
    """The ``(msgid, reference)`` pairs of a template whose reference starts
    with one of ``kinds``."""
    found = set()
    for block in ENTRY.split(text):
        references = []
        msgid_lines: list[str] = []
        in_msgid = False
        for line in block.splitlines():
            if line.startswith("#: "):
                references.extend(line[3:].split())
            elif line.startswith("msgid "):
                in_msgid = True
                msgid_lines.append(line[6:])
            elif line.startswith("msgstr"):
                in_msgid = False
            elif in_msgid and line.startswith('"'):
                msgid_lines.append(line)
        msgid = "".join(part[1:-1] for part in msgid_lines)
        for reference in references:
            if reference.startswith(kinds):
                found.add((msgid, reference))
    return found


def kind_of(reference: str) -> str:
    """``code`` or ``model:<model>,<field>`` of a reference."""
    parts = reference.split(":")
    return parts[0] if parts[0] == "code" else ":".join(parts[:2])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("corpus", type=Path, help="folder with OCA repositories")
    parser.add_argument("--odl", type=Path, default=ODL)
    parser.add_argument(
        "--kind",
        action="append",
        help="compare only references starting with this (repeatable)",
    )
    parser.add_argument("--addons-path", default="", help="more addons folders")
    parser.add_argument("--samples", type=int, default=10)
    args = parser.parse_args()
    kinds = tuple(args.kind or ["code:", "model:", "model_terms:"])

    totals = Counter()
    per_kind: dict[str, Counter] = {}
    per_module: Counter = Counter()
    missing_samples: list[tuple[str, str, str]] = []
    extra_samples: list[tuple[str, str, str]] = []
    with tempfile.TemporaryDirectory() as out:
        out_dir = Path(out)
        repos = sorted(
            p
            for p in args.corpus.iterdir()
            if p.is_dir() and not p.name.startswith(".") and p.name not in SKIPPED
        )
        addons_path = ",".join(
            [str(r) for r in repos] + [p for p in args.addons_path.split(",") if p]
        )
        for repo in repos:
            subprocess.run(
                [
                    str(args.odl),
                    "pot",
                    str(repo),
                    "--output-dir",
                    str(out_dir / repo.name),
                    "--addons-path",
                    addons_path,
                    "--installed",
                    str(repo),
                    "--skip-auto-install",
                ],
                check=False,
                capture_output=True,
            )
            for reference_pot in sorted(repo.glob("*/i18n/*.pot")):
                module = reference_pot.parent.parent.name
                if reference_pot.stem != module:
                    continue
                ours_path = out_dir / repo.name / f"{module}.pot"
                theirs = pairs(reference_pot.read_text(encoding="utf-8"), kinds)
                # No template: odl found no terms (or failed); OCA's count as
                # missing.
                if ours_path.is_file():
                    ours = pairs(ours_path.read_text(encoding="utf-8"), kinds)
                else:
                    totals["modules without output"] += 1
                    ours = set()
                for label, found in (("in OCA", theirs), ("matched", theirs & ours)):
                    for _, reference in found:
                        per_kind.setdefault(kind_of(reference), Counter())[label] += 1
                for _, reference in ours - theirs:
                    per_kind.setdefault(kind_of(reference), Counter())["extra"] += 1
                totals["modules"] += 1
                totals["modules equal"] += theirs == ours
                totals["pairs in OCA"] += len(theirs)
                totals["pairs matched"] += len(theirs & ours)
                totals["pairs missing"] += len(theirs - ours)
                totals["pairs extra"] += len(ours - theirs)
                per_module[module] += len(theirs ^ ours)
                missing_samples += [(module, *p) for p in sorted(theirs - ours)]
                extra_samples += [(module, *p) for p in sorted(ours - theirs)]

    for key, value in totals.items():
        print(f"{key:24} {value}")
    if totals["pairs in OCA"]:
        print(f"{'matched':24} {totals['pairs matched'] / totals['pairs in OCA']:.1%}")
    print(f"\n{'kind':50} {'in OCA':>7} {'matched':>8} {'extra':>6}")
    ranked = sorted(per_kind.items(), key=lambda item: -item[1]["in OCA"])
    for kind, counts in ranked[:25]:
        print(
            f"{kind:50} {counts['in OCA']:7} {counts['matched']:8} {counts['extra']:6}"
        )
    print("\nmodules with the most differences:")
    for module, count in per_module.most_common(15):
        print(f"  {module:50} {count}")
    for title, samples in (("missing", missing_samples), ("extra", extra_samples)):
        print(f"\n{title} (first {args.samples}):")
        for module, msgid, reference in samples[: args.samples]:
            print(f"  {module}: {msgid[:70]!r}  {reference}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
