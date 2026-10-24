"""Compare ``odl pot`` with the templates OCA's bot exports from Odoo.

OCA regenerates each module's ``i18n/<module>.pot`` with Odoo's own export
after every merge, so a checkout of OCA repositories is a large set of
reference templates. This script runs ``odl pot`` on those modules into a
temporary folder and compares the entries, per kind of reference (``code``
for now), as ``(msgid, reference)`` pairs.

Usage::

    python scripts/pot_parity.py ~/oca/19.0             # every repository in the folder
    python scripts/pot_parity.py ~/oca/19.0 --samples 20
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

ENTRY = re.compile(r"\n\n(?=#)")


def pairs(text: str, kind: str) -> set[tuple[str, str]]:
    """The ``(msgid, reference)`` pairs of ``kind`` in a template."""
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
            if reference.startswith(kind + ":"):
                found.add((msgid, reference))
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("corpus", type=Path, help="folder with OCA repositories")
    parser.add_argument("--odl", type=Path, default=ODL)
    parser.add_argument("--kind", default="code")
    parser.add_argument("--samples", type=int, default=10)
    args = parser.parse_args()

    totals = Counter()
    missing_samples: list[tuple[str, str, str]] = []
    extra_samples: list[tuple[str, str, str]] = []
    with tempfile.TemporaryDirectory() as out:
        out_dir = Path(out)
        repos = sorted(
            p
            for p in args.corpus.iterdir()
            if p.is_dir() and not p.name.startswith(".")
        )
        for repo in repos:
            subprocess.run(
                [
                    str(args.odl),
                    "pot",
                    str(repo),
                    "--output-dir",
                    str(out_dir / repo.name),
                ],
                check=False,
                capture_output=True,
            )
            for reference_pot in sorted(repo.glob("*/i18n/*.pot")):
                module = reference_pot.parent.parent.name
                if reference_pot.stem != module:
                    continue
                ours_path = out_dir / repo.name / f"{module}.pot"
                if not ours_path.is_file():
                    totals["modules without output"] += 1
                    continue
                theirs = pairs(reference_pot.read_text(encoding="utf-8"), args.kind)
                ours = pairs(ours_path.read_text(encoding="utf-8"), args.kind)
                totals["modules"] += 1
                totals["modules equal"] += theirs == ours
                totals["pairs in OCA"] += len(theirs)
                totals["pairs matched"] += len(theirs & ours)
                totals["pairs missing"] += len(theirs - ours)
                totals["pairs extra"] += len(ours - theirs)
                missing_samples += [(module, *p) for p in sorted(theirs - ours)]
                extra_samples += [(module, *p) for p in sorted(ours - theirs)]

    for key, value in totals.items():
        print(f"{key:24} {value}")
    if totals["pairs in OCA"]:
        print(f"{'matched':24} {totals['pairs matched'] / totals['pairs in OCA']:.1%}")
    for title, samples in (("missing", missing_samples), ("extra", extra_samples)):
        print(f"\n{title} (first {args.samples}):")
        for module, msgid, reference in samples[: args.samples]:
            print(f"  {module}: {msgid[:70]!r}  {reference}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
