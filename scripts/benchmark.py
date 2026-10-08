"""Time odoo-lint against pylint + pylint-odoo on an OCA repository.

Usage:
    python scripts/benchmark.py REPO --pylint PATH/TO/pylint [--odl odl]

Both tools lint the whole repository, after one warm-up run: pylint with
the repository's own `.pylintrc`, in one process as OCA's pre-commit hook
runs it (`require_serial: true`); odoo-lint with its defaults, which also
run the PO checks. The median of the runs is reported, and the README chart
(`docs/_static/benchmark.svg`) is written from it.
"""

from __future__ import annotations

import argparse
import os
import platform
import statistics
import subprocess
import time
from pathlib import Path

STATIC = Path(__file__).resolve().parent.parent / "docs" / "_static"

# Mid-tone colours, readable on light and dark pages alike: the chart is an
# image, so it cannot follow the page's theme.
THEME = {"text": "#7d8590", "muted": "#7d8590", "accent": "#ef662f", "bar": "#7d8590"}


def python_files(repo: Path) -> list[str]:
    """The `.py` files pre-commit would pass: everything outside hidden folders."""
    files = []
    for root, dirs, names in os.walk(repo):
        dirs[:] = sorted(d for d in dirs if not d.startswith("."))
        files += [
            str(Path(root, n).relative_to(repo))
            for n in sorted(names)
            if n.endswith(".py")
        ]
    return files


def timed(command: list[str], cwd: Path, runs: int) -> float:
    """Median wall-clock seconds of `command`, after a warm-up run."""
    subprocess.run(command, cwd=cwd, capture_output=True, check=False)
    samples = []
    for _ in range(runs):
        start = time.perf_counter()
        subprocess.run(command, cwd=cwd, capture_output=True, check=False)
        samples.append(time.perf_counter() - start)
    return statistics.median(samples)


def seconds(value: float) -> str:
    return f"{value:.2f}s" if value < 10 else f"{value:.1f}s"


def chart(
    results: list[tuple[str, float]], caption: str, theme: dict[str, str] = THEME
) -> str:
    """A horizontal bar chart, fastest first, in the style of Ruff's README."""
    width, label_width, row = 640, 130, 34
    bar_space = width - label_width - 70
    longest = max(t for _, t in results)
    height = row * len(results) + 34
    font = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif"
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}" font-family="{font}" font-size="14">'
    ]
    for i, (name, value) in enumerate(results):
        y = i * row + 6
        bar = max(2.0, bar_space * value / longest)
        first = i == 0
        fill = theme["accent"] if first else theme["bar"]
        weight = "bold" if first else "normal"
        parts.append(
            f'<text x="{label_width - 10}" y="{y + 17}" text-anchor="end" '
            f'fill="{theme["text"]}" font-weight="{weight}">{name}</text>'
        )
        parts.append(
            f'<rect x="{label_width}" y="{y + 3}" width="{bar:.1f}" height="20" '
            f'rx="3" fill="{fill}"/>'
        )
        parts.append(
            f'<text x="{label_width + bar + 8:.1f}" y="{y + 17}" '
            f'fill="{theme["text"]}" font-weight="{weight}">{seconds(value)}</text>'
        )
    parts.append(
        f'<text x="{label_width}" y="{height - 8}" fill="{theme["muted"]}" '
        f'font-size="12">{caption}</text>'
    )
    parts.append("</svg>\n")
    return "\n".join(parts)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("repo", type=Path)
    parser.add_argument(
        "--pylint", required=True, help="pylint with pylint-odoo installed"
    )
    parser.add_argument("--odl", default="odl")
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--no-chart", action="store_true")
    args = parser.parse_args()

    repo = args.repo.resolve()
    files = python_files(repo)
    modules = sum(1 for _ in repo.glob("*/__manifest__.py"))
    pylint_version = subprocess.run(
        [args.pylint, "--version"], capture_output=True, text=True, check=True
    ).stdout.split()[1]
    odl_version = subprocess.run(
        [args.odl, "--version"], capture_output=True, text=True, check=True
    ).stdout.split()[1]

    odl = timed([args.odl, "check", "."], repo, args.runs)
    pylint = timed(
        [args.pylint, "--rcfile=.pylintrc", "--exit-zero", *files],
        repo,
        args.runs,
    )
    print(
        f"{repo.name}: {modules} modules, {len(files)} Python files, "
        f"{os.cpu_count()} CPUs"
    )
    print(f"odoo-lint {odl_version}: {seconds(odl)}")
    print(f"pylint {pylint_version} + pylint-odoo: {seconds(pylint)}")
    print(f"odoo-lint is {pylint / odl:.0f}x faster ({platform.machine()})")

    if not args.no_chart:
        caption = (
            f"Linting OCA/{repo.name} ({modules} modules, {len(files)} Python files)"
        )
        results = [("odoo-lint", odl), ("pylint-odoo", pylint)]
        (STATIC / "benchmark.svg").write_text(chart(results, caption))


if __name__ == "__main__":
    main()
