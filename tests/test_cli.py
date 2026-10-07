"""End-to-end tests for the `odl` binary shipped in the wheel."""

import shutil
import subprocess
from pathlib import Path

import pytest

ODL = shutil.which("odl")

pytestmark = pytest.mark.skipif(ODL is None, reason="odl is not installed")

CONFIG = """
[tool.odoo-lint]
target-version = "16.0"

[tool.odoo-lint.rules.manifest-author]
default = "Odoo Community Association (OCA)"

[tool.odoo-lint.rules.manifest-author.mapping]
"acme_*" = "Acme Corp"
"""


def run_odl(*args: str) -> subprocess.CompletedProcess[str]:
    """Run `odl` and capture its output."""
    assert ODL is not None
    return subprocess.run([ODL, *args], capture_output=True, text=True, check=False)


def make_module(root: Path, name: str, author: str) -> None:
    """Create a minimal addon with the given manifest author."""
    module = root / "addons" / name
    module.mkdir(parents=True)
    (module / "__manifest__.py").write_text(
        f"{{'name': '{name}', 'author': '{author}'}}\n"
    )


def test_version() -> None:
    """`odl --version` reports the crate version."""
    result = run_odl("--version")
    assert result.returncode == 0
    assert result.stdout.startswith("odl ")


def test_clean_project(tmp_path: Path) -> None:
    """A project without violations exits with 0."""
    (tmp_path / "pyproject.toml").write_text(CONFIG)
    make_module(tmp_path, "acme_sale", "Acme Corp")
    make_module(tmp_path, "sale_extra", "Odoo Community Association (OCA)")

    result = run_odl("check", str(tmp_path / "addons"))

    assert result.returncode == 0, result.stdout
    assert "(v16.0)" in result.stdout


def test_author_mapping_violation(tmp_path: Path) -> None:
    """A module matching a mapping pattern must use the mapped author."""
    (tmp_path / "pyproject.toml").write_text(CONFIG)
    make_module(tmp_path, "acme_sale", "Odoo Community Association (OCA)")

    result = run_odl("check", str(tmp_path / "addons"), "--version", "18.0")

    assert result.returncode == 1
    assert "[ODOO010]" in result.stdout
    assert "Acme Corp" in result.stdout
    assert "(v18.0)" in result.stdout


def test_invalid_config(tmp_path: Path) -> None:
    """Unknown config keys are rejected with exit code 2."""
    (tmp_path / "pyproject.toml").write_text(
        '[tool.odoo-lint.rules.manifest_author]\ndefault = "x"\n'
    )
    (tmp_path / "addons").mkdir()

    result = run_odl("check", str(tmp_path / "addons"))

    assert result.returncode == 2
    assert "manifest-author" in result.stderr
