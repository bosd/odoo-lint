"""Release assets: `odl` from a built wheel, in an archive per platform.

Editors download these from the GitHub release (the Zed extension through
Zed's API), so the names are fixed and carry no version:

    python scripts/release_asset.py pack dist/odoo_linter-*.whl \
        odl-x86_64-unknown-linux-gnu.tar.gz
    python scripts/release_asset.py names

The binary comes from the wheel rather than a plain `cargo build`, so the
Linux ones are the manylinux and musllinux builds that run on older systems.
"""

import argparse
import io
import sys
import tarfile
import zipfile
from pathlib import Path

# Asset name -> the executable's name in the wheel.
ASSETS = {
    "odl-x86_64-unknown-linux-gnu.tar.gz": "odl",
    "odl-aarch64-unknown-linux-gnu.tar.gz": "odl",
    "odl-x86_64-unknown-linux-musl.tar.gz": "odl",
    "odl-aarch64-unknown-linux-musl.tar.gz": "odl",
    "odl-x86_64-apple-darwin.tar.gz": "odl",
    "odl-aarch64-apple-darwin.tar.gz": "odl",
    "odl-x86_64-pc-windows-msvc.zip": "odl.exe",
}


def pack(wheel: Path, asset: str) -> None:
    exe = ASSETS[asset]
    with zipfile.ZipFile(wheel) as whl:
        members = [n for n in whl.namelist() if n.endswith(f".data/scripts/{exe}")]
        if len(members) != 1:
            sys.exit(f"{wheel}: expected one {exe}, found {members}")
        data = whl.read(members[0])
    if asset.endswith(".zip"):
        with zipfile.ZipFile(asset, "w", zipfile.ZIP_DEFLATED) as archive:
            archive.writestr(exe, data)
    else:
        with tarfile.open(asset, "w:gz") as archive:
            info = tarfile.TarInfo(exe)
            info.size = len(data)
            info.mode = 0o755
            archive.addfile(info, io.BytesIO(data))
    print(f"{asset}: {exe} from {wheel.name}, {len(data)} bytes")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    pack_parser = commands.add_parser("pack", help="write an asset from a wheel")
    pack_parser.add_argument("wheel", type=Path)
    pack_parser.add_argument("asset", choices=sorted(ASSETS))
    commands.add_parser("names", help="print the names of all assets")
    args = parser.parse_args()
    if args.command == "pack":
        pack(args.wheel, args.asset)
    else:
        print("\n".join(sorted(ASSETS)))


if __name__ == "__main__":
    main()
