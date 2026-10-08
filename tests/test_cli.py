"""End-to-end tests for the `odl` binary shipped in the wheel."""

import json
import shutil
import subprocess
from pathlib import Path

import pytest

ODL = shutil.which("odl")

pytestmark = pytest.mark.skipif(ODL is None, reason="odl is not installed")

OCA = "Odoo Community Association (OCA)"

CONFIG = """
[tool.odoo-lint]
target-version = "16.0"

[tool.odoo-lint.rules.manifest-required-author]
authors = "Odoo Community Association (OCA)"

[tool.odoo-lint.rules.manifest-required-author.mapping]
"acme_*" = "Acme Corp"
"""


def run_odl(*args: str) -> subprocess.CompletedProcess[str]:
    """Run `odl` and capture its output."""
    assert ODL is not None
    return subprocess.run([ODL, *args], capture_output=True, text=True, check=False)


def make_module(root: Path, name: str, author: str) -> Path:
    """Create a minimal addon with the given manifest author."""
    module = root / "addons" / name
    module.mkdir(parents=True)
    (module / "__manifest__.py").write_text(
        f"{{\n    'name': '{name}',\n    'author': '{author}',\n}}\n"
    )
    return module


@pytest.fixture
def project(tmp_path: Path) -> Path:
    """A project with the CONFIG above in its pyproject.toml."""
    (tmp_path / "pyproject.toml").write_text(CONFIG)
    return tmp_path


def test_version() -> None:
    """`odl --version` reports the crate version."""
    result = run_odl("--version")
    assert result.returncode == 0
    assert result.stdout.startswith("odl ")


def test_clean_project(project: Path) -> None:
    """A project without violations exits with 0."""
    make_module(project, "acme_sale", "Acme Corp")
    make_module(project, "sale_extra", OCA)

    result = run_odl("check", str(project / "addons"))

    assert result.returncode == 0, result.stdout
    assert "(Odoo 16.0)" in result.stderr


def test_author_mapping_violation(project: Path) -> None:
    """A module matching a mapping pattern must use the mapped author."""
    make_module(project, "acme_sale", OCA)

    result = run_odl("check", str(project / "addons"), "--version", "18.0")

    assert result.returncode == 1
    line = result.stdout.splitlines()[0]
    assert line.endswith(
        "__manifest__.py:3:4: C8101: One of the following authors must be "
        "present in manifest: 'Acme Corp' (manifest-required-author)"
    )
    assert "(Odoo 18.0)" in result.stderr


def test_pylint_disable_comment(project: Path) -> None:
    """`# pylint: disable=<name>` suppresses a violation on that line."""
    module = project / "addons" / "acme_sale"
    module.mkdir(parents=True)
    (module / "__manifest__.py").write_text(
        "{\n    'author': 'Other',  # pylint: disable=manifest-required-author\n}\n"
    )

    assert run_odl("check", str(project / "addons")).returncode == 0


def test_select_and_ignore_on_cli(project: Path) -> None:
    """--ignore disables a rule; --select limits the run to some rules."""
    make_module(project, "acme_sale", OCA)
    addons = str(project / "addons")

    assert run_odl("check", addons, "--ignore", "C8101").returncode == 0
    assert run_odl("check", addons, "--select", "ODOO").returncode == 0
    assert run_odl("check", addons, "--select", "C81").returncode == 1


def test_json_output(project: Path) -> None:
    """JSON output is machine readable and has no banner on stdout."""
    make_module(project, "acme_sale", OCA)

    result = run_odl("check", str(project / "addons"), "--output-format", "json")

    data = json.loads(result.stdout)
    assert [v["code"] for v in data] == ["C8101"]
    assert data[0]["line"] == 3
    assert data[0]["name"] == "manifest-required-author"


def test_github_output(project: Path) -> None:
    """GitHub output produces workflow annotations."""
    make_module(project, "acme_sale", OCA)

    result = run_odl("check", str(project / "addons"), "--output-format", "github")

    assert result.stdout.startswith("::warning file=")
    assert "title=C8101 (manifest-required-author)::" in result.stdout


def test_syntax_error(project: Path) -> None:
    """Unparsable Python is reported as E0001 instead of being skipped."""
    module = make_module(project, "sale_extra", OCA)
    (module / "broken.py").write_text("def broken(:\n    pass\n")

    result = run_odl("check", str(project / "addons"))

    assert result.returncode == 1
    assert "E0001: Parsing failed" in result.stdout


def test_virtualenv_is_skipped(project: Path) -> None:
    """Directories like .venv are never linted."""
    venv = project / "addons" / ".venv"
    venv.mkdir(parents=True)
    (venv / "broken.py").write_text("def broken(:\n")

    assert run_odl("check", str(project / "addons")).returncode == 0


def test_invalid_config(tmp_path: Path) -> None:
    """Unknown config keys are rejected with exit code 2."""
    (tmp_path / "pyproject.toml").write_text(
        '[tool.odoo-lint.rules.manifest_author]\ndefault = "x"\n'
    )
    (tmp_path / "addons").mkdir()

    result = run_odl("check", str(tmp_path / "addons"))

    assert result.returncode == 2
    assert "manifest_author" in result.stderr


def test_rule_command() -> None:
    """`odl rule` lists rules and explains one by code or name."""
    listing = run_odl("rule")
    assert "C8101" in listing.stdout
    assert "manifest-required-author" in listing.stdout

    by_name = run_odl("rule", "manifest-required-author")
    assert by_name.returncode == 0
    assert by_name.stdout.startswith("# manifest-required-author (C8101)")
