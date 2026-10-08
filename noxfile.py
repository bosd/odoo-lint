"""Nox sessions."""

import os
import shlex
import shutil
from pathlib import Path
from textwrap import dedent

import nox

nox.options.default_venv_backend = "uv"

python_versions = ["3.13", "3.14", "3.12", "3.11", "3.10"]
nox.needs_version = ">= 2021.6.6"
nox.options.sessions = (
    "pre-commit",
    "fmt",
    "clippy",
    "cargo-test",
    "tests",
    "parity",
    "docs-build",
)


def activate_virtualenv_in_precommit_hooks(session: nox.Session) -> None:
    """Activate virtualenv in hooks installed by pre-commit.

    This function patches git hooks installed by pre-commit to activate the
    session's virtual environment. This allows pre-commit to locate hooks in
    that environment when invoked from git.

    Args:
        session: The Session object.
    """
    assert session.bin is not None  # nosec

    # Only patch hooks containing a reference to this session's bindir. Support
    # quoting rules for Python and bash, but strip the outermost quotes so we
    # can detect paths within the bindir, like <bindir>/python.
    bindirs = [
        bindir[1:-1] if bindir[0] in "'\"" else bindir
        for bindir in (repr(session.bin), shlex.quote(session.bin))
    ]

    virtualenv = session.env.get("VIRTUAL_ENV")
    if virtualenv is None:
        return

    headers = {
        # pre-commit < 2.16.0
        "python": f"""\
            import os
            os.environ["VIRTUAL_ENV"] = {virtualenv!r}
            os.environ["PATH"] = os.pathsep.join((
                {session.bin!r},
                os.environ.get("PATH", ""),
            ))
            """,
        # pre-commit >= 2.16.0
        "bash": f"""\
            VIRTUAL_ENV={shlex.quote(virtualenv)}
            PATH={shlex.quote(session.bin)}"{os.pathsep}$PATH"
            """,
        # pre-commit >= 2.17.0 on Windows forces sh shebang
        "/bin/sh": f"""\
            VIRTUAL_ENV={shlex.quote(virtualenv)}
            PATH={shlex.quote(session.bin)}"{os.pathsep}$PATH"
            """,
    }

    hookdir = Path(".git") / "hooks"
    if not hookdir.is_dir():
        return

    for hook in hookdir.iterdir():
        if hook.name.endswith(".sample") or not hook.is_file():
            continue

        if not hook.read_bytes().startswith(b"#!"):
            continue

        text = hook.read_text()

        if not any(
            (Path("A") == Path("a") and bindir.lower() in text.lower())
            or bindir in text
            for bindir in bindirs
        ):
            continue

        lines = text.splitlines()

        for executable, header in headers.items():
            if executable in lines[0].lower():
                lines.insert(1, dedent(header))
                hook.write_text("\n".join(lines))
                break


def sync(session: nox.Session, *groups: str) -> None:
    """Sync the session venv with uv, building `odl` via maturin.

    Args:
        session: The Session object.
        groups: Dependency groups to install.
    """
    group_args = [arg for group in groups for arg in ("--group", group)]
    session.run(
        "uv",
        "sync",
        "--locked",
        *group_args,
        external=True,
        env={"UV_PROJECT_ENVIRONMENT": session.virtualenv.location},
    )


@nox.session(name="pre-commit", python=python_versions[0])
def precommit(session: nox.Session) -> None:
    """Lint using pre-commit."""
    args = session.posargs or [
        "run",
        "--all-files",
        "--hook-stage=manual",
        "--show-diff-on-failure",
    ]
    session.run(
        "uv",
        "sync",
        "--locked",
        "--no-install-project",
        "--group",
        "lint",
        external=True,
        env={"UV_PROJECT_ENVIRONMENT": session.virtualenv.location},
    )
    session.run("pre-commit", *args)
    if args and args[0] == "install":
        activate_virtualenv_in_precommit_hooks(session)


@nox.session(python=False)
def fmt(session: nox.Session) -> None:
    """Check Rust formatting with rustfmt."""
    args = session.posargs or ["--check"]
    session.run("cargo", "fmt", "--all", "--", *args, external=True)


@nox.session(python=False)
def clippy(session: nox.Session) -> None:
    """Lint Rust code with clippy, denying warnings."""
    session.run(
        "cargo",
        "clippy",
        "--locked",
        "--all-targets",
        "--",
        "-D",
        "warnings",
        *session.posargs,
        external=True,
    )


@nox.session(name="cargo-test", python=False)
def cargo_test(session: nox.Session) -> None:
    """Run the Rust unit and integration tests."""
    session.run("cargo", "test", "--locked", *session.posargs, external=True)


@nox.session(python=python_versions)
def tests(session: nox.Session) -> None:
    """Run the CLI tests against the `odl` binary installed by maturin."""
    sync(session, "dev")
    session.run("pytest", *session.posargs)


@nox.session(python=python_versions[0])
def parity(session: nox.Session) -> None:
    """Compare odl with OCA's linters on their own test repositories.

    Without arguments it fails when an implemented check reports a different
    number of messages than pylint-odoo, or when docs/parity.md is stale.
    Pass ``-- --write`` to regenerate the page and the README badge.
    """
    sync(session, "dev")
    args = session.posargs or ["--check"]
    session.run("python", "scripts/oca_parity.py", *args)


@nox.session(python=python_versions[0])
def build(session: nox.Session) -> None:
    """Build the sdist and a wheel for the current platform into dist/."""
    session.run("uv", "build", "--out-dir", "dist", *session.posargs, external=True)


def build_docs(session: nox.Session, builder_args: list[str]) -> None:
    """Sync the docs group and run sphinx-build into a clean docs/_build.

    Args:
        session: The Session object.
        builder_args: Arguments for sphinx-build.
    """
    session.run(
        "uv",
        "sync",
        "--locked",
        "--no-install-project",
        "--group",
        "docs",
        external=True,
        env={"UV_PROJECT_ENVIRONMENT": session.virtualenv.location},
    )
    build_dir = Path("docs", "_build")
    if build_dir.exists():
        shutil.rmtree(build_dir)
    session.run("sphinx-build", *builder_args)


@nox.session(name="docs-build", python=python_versions[0])
def docs_build(session: nox.Session) -> None:
    """Build the documentation, treating warnings as errors."""
    args = session.posargs or ["-W", "--keep-going", "docs", "docs/_build"]
    if not session.posargs and "FORCE_COLOR" in os.environ:
        args.insert(0, "--color")
    build_docs(session, args)


@nox.session(name="docs-linkcheck", python=python_versions[0])
def docs_linkcheck(session: nox.Session) -> None:
    """Check links in the documentation.

    Not part of the default sessions; run on a weekly schedule via the
    ``linkcheck`` workflow so flaky external links never block a merge.
    """
    args = session.posargs or ["-b", "linkcheck", "--keep-going", "docs", "docs/_build"]
    build_docs(session, args)


@nox.session(python=python_versions[0])
def docs(session: nox.Session) -> None:
    """Build and serve the documentation with live reloading on file changes."""
    session.run(
        "uv",
        "sync",
        "--locked",
        "--no-install-project",
        "--group",
        "docs",
        external=True,
        env={"UV_PROJECT_ENVIRONMENT": session.virtualenv.location},
    )
    args = session.posargs or ["--open-browser", "docs", "docs/_build"]
    session.run("sphinx-autobuild", *args)
