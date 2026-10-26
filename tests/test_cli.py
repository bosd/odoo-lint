"""End-to-end tests for the `odl` binary shipped in the wheel."""

import json
import shutil
import subprocess
import sys
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


def run_odl(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    """Run `odl` (in `cwd`) and capture its output."""
    assert ODL is not None
    # odl writes UTF-8; Windows would otherwise decode with the ANSI code page.
    return subprocess.run(
        [ODL, *args], capture_output=True, encoding="utf-8", check=False, cwd=cwd
    )


def make_module(root: Path, name: str, author: str) -> Path:
    """Create an addon that is clean apart from the given manifest author."""
    module = root / "addons" / name
    module.mkdir(parents=True)
    (module / "__manifest__.py").write_text(
        f"{{\n    'name': '{name}',\n    'author': '{author}',\n"
        "    'license': 'AGPL-3',\n}\n"
    )
    (module / "README.rst").write_text(f"{name}\n")
    (module / "__init__.py").write_text("")
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

    result = run_odl("check", "-v", str(project / "addons"))

    assert result.returncode == 0, result.stdout
    assert "Odoo version: 16.0" in result.stderr


def test_author_mapping_violation(project: Path) -> None:
    """A module matching a mapping pattern must use the mapped author."""
    make_module(project, "acme_sale", OCA)

    result = run_odl("check", "-v", str(project / "addons"), "--version", "18.0")

    assert result.returncode == 1
    line = result.stdout.splitlines()[0]
    assert line.endswith(
        "__manifest__.py:3:4: C8101: One of the following authors must be "
        "present in manifest: 'Acme Corp' (manifest-required-author)"
    )
    assert "Odoo version: 18.0" in result.stderr


def test_pylint_disable_comment(project: Path) -> None:
    """`# pylint: disable=<name>` suppresses a violation on that line."""
    module = make_module(project, "acme_sale", "Other")
    (module / "__manifest__.py").write_text(
        "{\n    'author': 'Other',  # pylint: disable=manifest-required-author\n"
        "    'license': 'AGPL-3',\n}\n"
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


def test_rule_json() -> None:
    """`odl rule --output-format json` lists rules for tooling."""
    rules = json.loads(run_odl("rule", "--output-format", "json").stdout)
    c8101 = next(r for r in rules if r["code"] == "C8101")
    assert c8101["name"] == "manifest-required-author"
    assert c8101["min_odoo_version"] is None


def test_sarif_output(project: Path) -> None:
    """SARIF output references the rule and its documentation."""
    make_module(project, "acme_sale", OCA)

    result = run_odl("check", str(project / "addons"), "--output-format", "sarif")

    run = json.loads(result.stdout)["runs"][0]
    assert run["tool"]["driver"]["name"] == "odoo-lint"
    assert [r["ruleId"] for r in run["results"]] == ["C8101"]
    rule = run["tool"]["driver"]["rules"][run["results"][0]["ruleIndex"]]
    assert rule["helpUri"].endswith("/rules/C8101.html")


def test_gitlab_output(project: Path) -> None:
    """GitLab output is a Code Quality report."""
    make_module(project, "acme_sale", OCA)

    result = run_odl("check", str(project / "addons"), "--output-format", "gitlab")

    issue = json.loads(result.stdout)[0]
    assert issue["check_name"] == "C8101"
    assert issue["location"]["lines"]["begin"] == 3
    assert {"description", "fingerprint", "severity"} <= issue.keys()


def test_diff_and_fix(project: Path) -> None:
    """`--diff` shows the fixes, `--fix` writes them, and a second run is clean."""
    module = make_module(project, "acme_sale", "Acme Corp")
    manifest = module / "__manifest__.py"
    original = manifest.read_text()
    manifest.write_text(original.replace("}\n", "    'active': True,\n}\n"))
    changed = manifest.read_text()

    diff = run_odl("check", str(project / "addons"), "--select", "C8116", "--diff")
    assert diff.returncode == 1
    assert "-    'active': True," in diff.stdout
    assert "1 fix(es) would change 1 file(s)." in diff.stderr
    assert manifest.read_text() == changed

    fixed = run_odl("check", str(project / "addons"), "--select", "C8116", "--fix")
    assert fixed.returncode == 0, fixed.stdout
    assert "Fixed 1 violation(s)." in fixed.stdout
    assert manifest.read_text() == original

    again = run_odl("check", str(project / "addons"), "--select", "C8116", "--diff")
    assert again.returncode == 0
    assert again.stdout == ""


def test_force_exclude(project: Path) -> None:
    """Explicit files are linted unless `--force-exclude` applies `exclude`."""
    module = make_module(project, "legacy_sale", "Someone Else")
    config = CONFIG.replace(
        'target-version = "16.0"\n',
        'target-version = "16.0"\nexclude = ["addons/legacy_*"]\n',
    )
    (project / "pyproject.toml").write_text(config)
    manifest = str(module / "__manifest__.py")

    linted = run_odl("check", manifest, "--select", "C8101")
    assert linted.returncode == 1, linted.stdout

    skipped = run_odl("check", manifest, "--select", "C8101", "--force-exclude")
    assert skipped.returncode == 0, skipped.stdout

    # A folder given on the command line too, by a pattern for the folder only.
    (project / "pyproject.toml").write_text(config.replace("legacy_*", "legacy_sale"))
    folder = run_odl("check", str(module), "--select", "C8101", "--force-exclude")
    assert folder.returncode == 0, folder.stdout


def excluded_legacy_module(project: Path) -> Path:
    """A module `exclude` names, with a C8101 violation."""
    module = make_module(project, "legacy_sale", "Someone Else")
    config = CONFIG.replace(
        'target-version = "16.0"\n',
        'target-version = "16.0"\nexclude = ["addons/legacy_*"]\n',
    )
    (project / "pyproject.toml").write_text(config)
    return module


def test_exclude_applies_from_a_subdirectory(project: Path) -> None:
    """`exclude` patterns are relative to the config, not to the cwd."""
    excluded_legacy_module(project)

    result = run_odl("check", ".", "--select", "C8101", cwd=project / "addons")

    assert result.returncode == 0, result.stdout


def test_exclude_applies_with_a_relative_config(project: Path) -> None:
    """A relative `--config` is resolved like a discovered one."""
    excluded_legacy_module(project)

    result = run_odl(
        "check", ".", "--select", "C8101", "--config", "pyproject.toml", cwd=project
    )

    assert result.returncode == 0, result.stdout


def test_missing_path_is_an_error(project: Path) -> None:
    """A path that does not exist exits with 2 instead of passing as clean."""
    result = run_odl("check", str(project / "nope.py"))

    assert result.returncode == 2
    assert "no such file or directory" in result.stderr


def test_explicit_file_of_another_kind_is_skipped(project: Path) -> None:
    """Hooks may pass every changed file; only addon files are linted."""
    (project / "README.md").write_text("# Notes\n\nsome text\n")

    result = run_odl("check", str(project / "README.md"))

    assert result.returncode == 0, result.stdout


def test_ignore_all_turns_off_opt_in_rules(project: Path) -> None:
    """`ALL` in `ignore` also covers the rules `ALL` in `select` leaves out."""
    module = make_module(project, "sale_extra", OCA)
    manifest = module / "__manifest__.py"
    manifest.write_text(
        manifest.read_text().replace("}", "    'data': ['views.xml'],\n}")
    )
    (module / "views.xml").write_text("<odoo/>\n")
    target = str(module / "views.xml")

    reported = run_odl("check", target, "--select", "XML002")
    ignored = run_odl("check", target, "--select", "XML002", "--ignore", "ALL")

    assert reported.returncode == 1, reported.stdout
    assert ignored.returncode == 0, ignored.stdout


def test_upgrade_check(project: Path) -> None:
    """`upgrade-check` reports the steps after each module's version."""
    module = make_module(project, "acme_sale", "Acme Corp")
    manifest = module / "__manifest__.py"
    manifest.write_text(
        manifest.read_text().replace("}\n", "    'version': '17.0.1.0.0',\n}\n")
    )
    (module / "__init__.py").write_text("from . import models\n")
    (module / "models").mkdir()
    (module / "models" / "__init__.py").write_text("from . import partner\n")
    (module / "models" / "partner.py").write_text(
        "from odoo import models\n\n\nclass Partner(models.Model):\n"
        "    _inherit = 'res.partner'\n\n    def run(self):\n"
        "        self._cr.execute('SELECT 1')\n"
    )
    addons = str(project / "addons")

    ready = run_odl("upgrade-check", addons, "--target", "18.0")
    assert ready.returncode == 0, ready.stdout
    assert "ready for Odoo 18.0 (code only; dependencies not checked)" in ready.stdout

    upgrade = run_odl("upgrade-check", addons, "--target", "19.0")
    assert upgrade.returncode == 1
    assert "acme_sale (17.0 → 19.0)" in upgrade.stdout
    assert "W8165" in upgrade.stdout

    report = json.loads(
        run_odl(
            "upgrade-check", addons, "--target", "19.0", "--output-format", "json"
        ).stdout
    )
    assert report["target"] == "19.0"
    assert report["changes"] == report["automatic"] == 1

    fixed = run_odl("upgrade-check", addons, "--target", "19.0", "--fix")
    assert fixed.returncode == 0, fixed.stdout
    assert "self.env.cr" in (module / "models" / "partner.py").read_text()


def test_exit_zero(project: Path) -> None:
    """`--exit-zero` reports violations but exits 0, for advisory hooks."""
    make_module(project, "acme_sale", "Other")
    addons = str(project / "addons")
    assert run_odl("check", addons).returncode == 1
    result = run_odl("check", "--exit-zero", addons)
    assert result.returncode == 0
    assert "C8101" in result.stdout
    # Errors are still errors.
    assert (
        run_odl(
            "check", "--exit-zero", "--config", str(project / "nope.toml"), addons
        ).returncode
        == 2
    )


def test_unknown_selector_is_an_error(project: Path) -> None:
    """A typo in `--select` must not turn the check off."""
    make_module(project, "sale_x", OCA)
    result = run_odl("check", str(project / "addons"), "--select", "BOGUS")
    assert result.returncode == 2
    assert "BOGUS" in result.stderr


def test_quiet_by_default_verbose_on_request(project: Path) -> None:
    """No banner without `--verbose`; `--quiet` drops the summary."""
    make_module(project, "sale_x", OCA)
    addons = str(project / "addons")
    result = run_odl("check", addons)
    assert result.returncode == 0
    assert result.stderr == ""
    verbose = run_odl("check", "-v", addons)
    assert "Odoo version: 16.0" in verbose.stderr
    assert "pyproject.toml" in verbose.stderr
    quiet = run_odl("check", "-q", addons)
    assert quiet.stdout == ""


def test_addons_path_problems_are_reported(project: Path) -> None:
    """A wrong addons path would silently turn the checks across modules off."""
    module = make_module(project, "sale_x", OCA)
    (module / "__manifest__.py").write_text(
        f"{{'name': 'x', 'author': '{OCA}', 'license': 'AGPL-3', "
        "'depends': ['sale']}\n"
    )
    empty = project / "empty"
    empty.mkdir()
    result = run_odl(
        "check",
        str(project / "addons"),
        "--addons-path",
        f"{empty},{project / 'missing'}",
    )
    assert f"{empty} holds no module" in result.stderr
    assert "does not exist" in result.stderr
    assert "sale_x: `base`, `sale` not found on the addons path" in result.stderr


@pytest.mark.skipif(sys.platform == "win32", reason="SIGPIPE is POSIX")
def test_closed_pipe_is_not_a_crash() -> None:
    """`odl rule | head -1` stops quietly."""
    assert ODL is not None
    # More output than a pipe buffers, so odl is still writing.
    proc = subprocess.Popen(
        [ODL, "rule", "--output-format", "json"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert proc.stdout is not None
    proc.stdout.readline()
    proc.stdout.close()
    _, stderr = proc.communicate()
    assert b"panicked" not in stderr


def test_findings_across_modules_keep_the_given_path(tmp_path: Path) -> None:
    """ODOO005 reports `addons/...` like every other rule, not an absolute path."""
    assert ODL is not None
    base = tmp_path / "core" / "base"
    base.mkdir(parents=True)
    (base / "__manifest__.py").write_text("{'name': 'base'}\n")
    module = make_module(tmp_path, "sale_x", OCA)
    (module / "__init__.py").write_text("from . import models\n")
    (module / "models.py").write_text(
        "from odoo import models\n\n\nclass P(models.Model):\n"
        "    _name = 'x.y'\n\n    def f(self):\n"
        "        return self.env.ref('base.missing')\n"
    )
    result = subprocess.run(
        [ODL, "check", "addons", "--addons-path", "core", "--select", "ODOO005"],
        capture_output=True,
        encoding="utf-8",
        check=False,
        cwd=tmp_path,
    )
    assert result.stdout.startswith(str(Path("addons/sale_x/models.py"))), result
