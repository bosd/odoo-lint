"""scripts/release_asset.py: `odl` from a wheel, packed as a release asset."""

import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path

SCRIPT = Path(__file__).parent.parent / "scripts" / "release_asset.py"


def make_wheel(path: Path, exe: str) -> Path:
    wheel = path / "odoo_linter-0.1.0a9-py3-none-any.whl"
    with zipfile.ZipFile(wheel, "w") as whl:
        whl.writestr(f"odoo_linter-0.1.0a9.data/scripts/{exe}", b"binary")
        whl.writestr("odoo_linter-0.1.0a9.dist-info/METADATA", b"")
    return wheel


def pack(tmp_path: Path, wheel: Path, asset: str) -> None:
    subprocess.run(
        [sys.executable, SCRIPT, "pack", wheel, asset],
        cwd=tmp_path,
        check=True,
        capture_output=True,
    )


def test_tar_holds_executable_odl(tmp_path):
    wheel = make_wheel(tmp_path, "odl")
    pack(tmp_path, wheel, "odl-x86_64-unknown-linux-gnu.tar.gz")
    with tarfile.open(tmp_path / "odl-x86_64-unknown-linux-gnu.tar.gz") as archive:
        member = archive.getmember("odl")
        assert member.mode == 0o755
        assert archive.extractfile(member).read() == b"binary"


def test_zip_holds_odl_exe(tmp_path):
    wheel = make_wheel(tmp_path, "odl.exe")
    pack(tmp_path, wheel, "odl-x86_64-pc-windows-msvc.zip")
    with zipfile.ZipFile(tmp_path / "odl-x86_64-pc-windows-msvc.zip") as archive:
        assert archive.read("odl.exe") == b"binary"


def test_names_cover_every_platform():
    names = subprocess.run(
        [sys.executable, SCRIPT, "names"], check=True, capture_output=True, text=True
    ).stdout.split()
    assert len(names) == 7
    assert "odl-aarch64-apple-darwin.tar.gz" in names
