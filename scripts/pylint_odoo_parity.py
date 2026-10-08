"""Compare odl with pylint-odoo on pylint-odoo's own test repository.

pylint-odoo's test suite states, per check, how many messages it expects on
``testing/resources/test_repo`` (``EXPECTED_ERRORS`` in ``tests/test_main.py``).
This script runs ``odl`` on the same files and compares the counts.

pylint-odoo is AGPL-3.0, so its sources are fetched at a pinned commit into
``.cache/`` at run time and never copied into this repository.

Usage::

    uv run nox -s parity                # CI: fail on mismatches or stale docs
    uv run nox -s parity -- --write     # update docs/parity.md and the badge
    python scripts/pylint_odoo_parity.py  # only print the comparison
"""

from __future__ import annotations

import argparse
import ast
import json
import re
import shutil
import subprocess
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

PYLINT_ODOO_REPO = "https://github.com/OCA/pylint-odoo.git"
PYLINT_ODOO_COMMIT = "30224d0a2ae20645b197776358a734d81197aafc"

ROOT = Path(__file__).resolve().parent.parent
CACHE = ROOT / ".cache" / "pylint-odoo" / PYLINT_ODOO_COMMIT
DOC_PAGE = ROOT / "docs" / "parity.md"
BADGE = ROOT / "docs" / "_static" / "parity.json"
# Version used for rules without an Odoo version range.
DEFAULT_ODOO_VERSION = "17.0"

REGENERATE = "uv run nox -s parity -- --write"

BRAND_ORANGE = "ef662f"
BRAND_MINT = "3fa886"


@dataclass(frozen=True)
class Row:
    """Comparison result for one pylint-odoo check."""

    name: str
    code: str
    expected: int
    actual: int | None  # None: not implemented in odl

    @property
    def matches(self) -> bool:
        """Whether odl reports exactly as many messages as pylint-odoo."""
        return self.actual == self.expected


def git(*args: str, cwd: Path) -> None:
    """Run git quietly in ``cwd``."""
    subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True)


def fetch_pylint_odoo() -> Path:
    """Shallow-fetch pylint-odoo at the pinned commit into the cache."""
    if (CACHE / "tests" / "test_main.py").is_file():
        return CACHE
    if CACHE.exists():
        shutil.rmtree(CACHE)
    CACHE.mkdir(parents=True)
    git("init", "-q", cwd=CACHE)
    git("fetch", "-q", "--depth", "1", PYLINT_ODOO_REPO, PYLINT_ODOO_COMMIT, cwd=CACHE)
    git("checkout", "-q", "FETCH_HEAD", cwd=CACHE)
    return CACHE


def expected_errors(source: Path) -> dict[str, int]:
    """``EXPECTED_ERRORS`` from pylint-odoo's ``tests/test_main.py``."""
    tree = ast.parse((source / "tests" / "test_main.py").read_text())
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(
            isinstance(t, ast.Name) and t.id == "EXPECTED_ERRORS" for t in node.targets
        ):
            return ast.literal_eval(node.value)
    sys.exit("EXPECTED_ERRORS not found in pylint-odoo tests/test_main.py")


def pylint_odoo_codes(source: Path) -> dict[str, str]:
    """Check name -> message id, from the checks table in pylint-odoo's README."""
    readme = (source / "README.md").read_text()
    table = readme.split("[//]: # (start-checks)")[1].split("[//]: # (end-checks)")[0]
    codes = {}
    for line in table.splitlines():
        parts = [p.strip() for p in line.split("|")]
        if len(parts) >= 3 and re.fullmatch(r"[A-Z]\d{4}", parts[-1]):
            codes[parts[0]] = parts[-1]
    return codes


def run_odl(odl: str, *args: str) -> str:
    """Run odl and return stdout; exit codes 0 and 1 both mean success."""
    result = subprocess.run(
        [odl, *args], capture_output=True, encoding="utf-8", check=False
    )
    if result.returncode not in (0, 1):
        sys.exit(f"odl {' '.join(args)} failed:\n{result.stderr}")
    return result.stdout


def odl_rules(odl: str) -> dict[str, dict]:
    """Rules odl implements, by name."""
    rules = json.loads(run_odl(odl, "rule", "--output-format", "json"))
    return {rule["name"]: rule for rule in rules}


def odl_counts(odl: str, test_repo: Path, version: str) -> Counter[str]:
    """Violations per rule name when linting the test repo for ``version``."""
    empty_config = CACHE.parent / "odoo-lint.toml"
    empty_config.write_text("")
    output = run_odl(
        odl,
        "check",
        str(test_repo),
        "--config",
        str(empty_config),
        "--select",
        "ALL",
        "--version",
        version,
        "--output-format",
        "json",
    )
    return Counter(v["name"] for v in json.loads(output))


def compare(odl: str) -> list[Row]:
    """Compare odl with pylint-odoo for every check pylint-odoo tests."""
    source = fetch_pylint_odoo()
    expected = expected_errors(source)
    codes = pylint_odoo_codes(source)
    rules = odl_rules(odl)
    test_repo = source / "testing" / "resources" / "test_repo"

    # pylint-odoo's expectations hold for all Odoo versions at once; run each
    # rule with a version it applies to.
    counts_by_version: dict[str, Counter[str]] = {}
    rows = []
    for name, count in sorted(expected.items()):
        rule = rules.get(name)
        actual = None
        if rule is not None:
            version = (
                rule["min_odoo_version"]
                or rule["max_odoo_version"]
                or DEFAULT_ODOO_VERSION
            )
            if version not in counts_by_version:
                counts_by_version[version] = odl_counts(odl, test_repo, version)
            actual = counts_by_version[version][name]
        rows.append(
            Row(name, rule["code"] if rule else codes.get(name, ""), count, actual)
        )
    return rows


def render_page(rows: list[Row]) -> str:
    """Markdown page for the documentation."""
    total = len(rows)
    implemented = sum(r.actual is not None for r in rows)
    matching = sum(r.matches for r in rows)
    short = PYLINT_ODOO_COMMIT[:7]
    lines = [
        f"<!-- Generated by scripts/pylint_odoo_parity.py; run `{REGENERATE}` -->",
        "",
        "# pylint-odoo parity",
        "",
        "odoo-lint is compared with pylint-odoo on pylint-odoo's own test repository,",
        f"`testing/resources/test_repo` at commit [`{short}`]"
        f"(https://github.com/OCA/pylint-odoo/tree/{PYLINT_ODOO_COMMIT}/testing/resources/test_repo).",
        "pylint-odoo's test suite states for every check how many messages it expects",
        "there. A check matches when `odl` reports exactly as many.",
        "",
        f"**{matching} of {total} checks match**; "
        f"odoo-lint implements {implemented} of them.",
        "",
        "| Check | Code | pylint-odoo | odl | Status |",
        "| ----- | ---- | ----------: | --: | ------ |",
    ]
    for r in rows:
        if r.actual is None:
            actual, status = "", "not implemented yet"
        else:
            actual = str(r.actual)
            status = "✅ match" if r.matches else "❌ differs"
        code = f"[{r.code}](rules/{r.code}.md)" if r.actual is not None else r.code
        lines.append(f"| `{r.name}` | {code} | {r.expected} | {actual} | {status} |")
    lines += [
        "",
        f"Regenerate this page with `{REGENERATE}`. CI runs the",
        "comparison on every change and fails when an implemented check stops",
        "matching.",
        "",
    ]
    return "\n".join(lines)


def render_badge(rows: list[Row]) -> str:
    """shields.io endpoint JSON for the README badge."""
    matching = sum(r.matches for r in rows)
    badge = {
        "schemaVersion": 1,
        "label": "pylint-odoo parity",
        "message": f"{matching}/{len(rows)} checks",
        "color": BRAND_MINT if matching == len(rows) else BRAND_ORANGE,
    }
    return json.dumps(badge, indent=2) + "\n"


def main() -> int:
    """Entry point."""
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--write", action="store_true", help="update docs/parity.md and the badge"
    )
    mode.add_argument(
        "--check", action="store_true", help="fail on mismatches or stale files"
    )
    parser.add_argument(
        "--odl", default=shutil.which("odl"), help="path to the odl binary"
    )
    args = parser.parse_args()
    if not args.odl:
        sys.exit("odl not found; run `uv sync` or pass --odl")

    rows = compare(args.odl)
    page, badge = render_page(rows), render_badge(rows)
    differing = [r for r in rows if r.actual is not None and not r.matches]
    matching = sum(r.matches for r in rows)
    print(f"pylint-odoo parity: {matching}/{len(rows)} checks match")
    for r in differing:
        print(f"  {r.code} {r.name}: pylint-odoo {r.expected}, odl {r.actual}")

    if args.write:
        DOC_PAGE.write_text(page, encoding="utf-8")
        BADGE.write_text(badge, encoding="utf-8")
        print(f"wrote {DOC_PAGE.relative_to(ROOT)} and {BADGE.relative_to(ROOT)}")
        return 0

    status = 1 if differing else 0
    if args.check:
        stale = [
            path.relative_to(ROOT)
            for path, content in ((DOC_PAGE, page), (BADGE, badge))
            if not path.is_file() or path.read_text(encoding="utf-8") != content
        ]
        if stale:
            print(f"out of date: {', '.join(map(str, stale))}; run `{REGENERATE}`")
            status = 1
    return status


if __name__ == "__main__":
    sys.exit(main())
