"""Compare odl with OCA's linters on their own test repositories.

Each OCA tool's test suite states, per check, how many messages it expects on
its test repository (``EXPECTED_ERRORS``). This script runs ``odl`` on the
same files and compares the counts, for:

- pylint-odoo (AGPL-3.0): ``tests/test_main.py``,
  ``testing/resources/test_repo``;
- oca-checks-po from odoo-pre-commit-hooks (LGPL-3.0):
  ``tests/test_checks_po.py``, ``test_repo``;
- oca-checks-odoo-module from the same repository, its XML checks:
  ``tests/test_checks.py``, ``test_repo``.

The sources are fetched at pinned commits into ``.cache/`` at run time and
never copied into this repository.

Usage::

    uv run nox -s parity                # CI: fail on mismatches or stale docs
    uv run nox -s parity -- --write     # update docs/parity.md and the badge
    python scripts/oca_parity.py        # only print the comparison
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

ROOT = Path(__file__).resolve().parent.parent
CACHE = ROOT / ".cache"
DOC_PAGE = ROOT / "docs" / "parity.md"
BADGE = ROOT / "docs" / "_static" / "parity.json"
# Version used for rules without an Odoo version range.
DEFAULT_ODOO_VERSION = "17.0"

REGENERATE = "uv run nox -s parity -- --write"

BRAND_ORANGE = "ef662f"
BRAND_MINT = "3fa886"


@dataclass(frozen=True)
class Source:
    """An OCA tool and where its expectations live."""

    title: str
    repo: str
    commit: str
    test_file: str
    test_repo: str
    #: Checks table in the README, for the codes of checks odl lacks.
    readme_codes: bool
    #: Only the checks whose names start with this.
    prefix: str = ""

    @property
    def cache(self) -> Path:
        """Where this source is checked out."""
        return CACHE / self.repo.rsplit("/", 1)[-1].removesuffix(".git") / self.commit

    @property
    def short(self) -> str:
        """Abbreviated commit."""
        return self.commit[:7]

    @property
    def web(self) -> str:
        """Browsable URL of the repository."""
        return self.repo.removesuffix(".git")


SOURCES = [
    Source(
        title="pylint-odoo",
        repo="https://github.com/OCA/pylint-odoo.git",
        commit="30224d0a2ae20645b197776358a734d81197aafc",
        test_file="tests/test_main.py",
        test_repo="testing/resources/test_repo",
        readme_codes=True,
    ),
    Source(
        title="oca-checks-po",
        repo="https://github.com/OCA/odoo-pre-commit-hooks.git",
        commit="82a2e95fa8bbea73a02bc377980ef8bd10e40dfd",
        test_file="tests/test_checks_po.py",
        test_repo="test_repo",
        readme_codes=False,
    ),
    Source(
        title="oca-checks-odoo-module (XML)",
        repo="https://github.com/OCA/odoo-pre-commit-hooks.git",
        commit="82a2e95fa8bbea73a02bc377980ef8bd10e40dfd",
        test_file="tests/test_checks.py",
        test_repo="test_repo",
        readme_codes=False,
        prefix="xml-",
    ),
]


@dataclass(frozen=True)
class Row:
    """Comparison result for one check."""

    name: str
    code: str
    expected: int
    actual: int | None  # None: not implemented in odl

    @property
    def matches(self) -> bool:
        """Whether odl reports exactly as many messages as the OCA tool."""
        return self.actual == self.expected


def git(*args: str, cwd: Path) -> None:
    """Run git quietly in ``cwd``."""
    subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True)


def fetch(source: Source) -> Path:
    """Shallow-fetch a source at its pinned commit into the cache."""
    path = source.cache
    if (path / source.test_file).is_file():
        return path
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)
    git("init", "-q", cwd=path)
    git("fetch", "-q", "--depth", "1", source.repo, source.commit, cwd=path)
    git("checkout", "-q", "FETCH_HEAD", cwd=path)
    return path


def expected_errors(path: Path, test_file: str) -> dict[str, int]:
    """``EXPECTED_ERRORS`` from a test module."""
    tree = ast.parse((path / test_file).read_text())
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(
            isinstance(t, ast.Name) and t.id == "EXPECTED_ERRORS" for t in node.targets
        ):
            return ast.literal_eval(node.value)
    sys.exit(f"EXPECTED_ERRORS not found in {test_file}")


def readme_codes(path: Path) -> dict[str, str]:
    """Check name -> message id, from the checks table in pylint-odoo's README."""
    readme = (path / "README.md").read_text()
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
    """Violations per rule name when linting a test repo for ``version``."""
    empty_config = CACHE / "odoo-lint.toml"
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


def compare(odl: str, source: Source) -> list[Row]:
    """Compare odl with one OCA tool for every check its tests count."""
    path = fetch(source)
    expected = {
        name: count
        for name, count in expected_errors(path, source.test_file).items()
        if name.startswith(source.prefix)
    }
    codes = readme_codes(path) if source.readme_codes else {}
    rules = odl_rules(odl)
    test_repo = path / source.test_repo

    # The expectations hold for all Odoo versions at once; run each rule with
    # a version it applies to.
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
        code = rule["code"] if rule else codes.get(name, "")
        rows.append(Row(name, code, count, actual))
    return rows


def render_table(source: Source, rows: list[Row]) -> list[str]:
    """Markdown section for one OCA tool."""
    implemented = sum(r.actual is not None for r in rows)
    matching = sum(r.matches for r in rows)
    lines = [
        f"## {source.title}",
        "",
        f"Compared on `{source.test_repo}` at commit "
        f"[`{source.short}`]({source.web}/tree/{source.commit}/{source.test_repo}), "
        f"with the counts from `{source.test_file}`.",
        "",
        f"**{matching} of {len(rows)} checks match**; "
        f"odoo-lint implements {implemented} of them.",
        "",
        "| Check | Code | Expected | odl | Status |",
        "| ----- | ---- | -------: | --: | ------ |",
    ]
    for r in rows:
        if r.actual is None:
            actual, status = "", "not implemented yet"
        else:
            actual = str(r.actual)
            status = "✅ match" if r.matches else "❌ differs"
        code = f"[{r.code}](rules/{r.code}.md)" if r.actual is not None else r.code
        lines.append(f"| `{r.name}` | {code} | {r.expected} | {actual} | {status} |")
    lines.append("")
    return lines


def render_page(results: list[tuple[Source, list[Row]]]) -> str:
    """Markdown page for the documentation."""
    total = sum(len(rows) for _, rows in results)
    matching = sum(r.matches for _, rows in results for r in rows)
    lines = [
        f"<!-- Generated by scripts/oca_parity.py; run `{REGENERATE}` -->",
        "",
        "# OCA parity",
        "",
        "odoo-lint is compared with OCA's own linters on their own test",
        "repositories. Each tool's test suite states for every check how many",
        "messages it expects there. A check matches when `odl` reports exactly",
        "as many.",
        "",
        f"**{matching} of {total} checks match.**",
        "",
    ]
    for source, rows in results:
        lines += render_table(source, rows)
    lines += [
        f"Regenerate this page with `{REGENERATE}`. CI runs the comparison on",
        "every change and fails when an implemented check stops matching.",
        "",
    ]
    return "\n".join(lines)


def render_badge(results: list[tuple[Source, list[Row]]]) -> str:
    """shields.io endpoint JSON for the README badge."""
    total = sum(len(rows) for _, rows in results)
    matching = sum(r.matches for _, rows in results for r in rows)
    badge = {
        "schemaVersion": 1,
        "label": "OCA parity",
        "message": f"{matching}/{total} checks",
        "color": BRAND_MINT if matching == total else BRAND_ORANGE,
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

    results = [(source, compare(args.odl, source)) for source in SOURCES]
    page, badge = render_page(results), render_badge(results)
    status = 0
    for source, rows in results:
        matching = sum(r.matches for r in rows)
        print(f"{source.title} parity: {matching}/{len(rows)} checks match")
        for r in rows:
            if r.actual is not None and not r.matches:
                print(f"  {r.code} {r.name}: expected {r.expected}, odl {r.actual}")
                status = 1

    if args.write:
        DOC_PAGE.write_text(page, encoding="utf-8")
        BADGE.write_text(badge, encoding="utf-8")
        print(f"wrote {DOC_PAGE.relative_to(ROOT)} and {BADGE.relative_to(ROOT)}")
        return 0

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
