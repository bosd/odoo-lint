// Picks the odoo-linter wheel for this platform from PyPI's JSON API and
// finds `odl` in it. No VS Code API here, so it can be tested on its own.

export interface PypiFile {
  filename: string;
  url: string;
  digests: { sha256: string };
}

/**
 * Fragments of the wheel file name that run on a platform, or undefined.
 * A Linux with musl (Alpine) needs a musllinux wheel.
 */
export function wheelTags(
  platform: string,
  arch: string,
  musl = false,
): string[] | undefined {
  const linux = musl ? "musllinux" : "manylinux";
  switch (`${platform}-${arch}`) {
    case "linux-x64":
      return [linux, "x86_64"];
    case "linux-arm64":
      return [linux, "aarch64"];
    case "darwin-arm64":
      return ["macosx", "arm64"];
    case "darwin-x64":
      return ["macosx", "x86_64"];
    case "win32-x64":
      return ["win_amd64"];
    default:
      return undefined;
  }
}

export function pickWheel(
  files: PypiFile[],
  platform: string,
  arch: string,
  musl = false,
): PypiFile | undefined {
  const tags = wheelTags(platform, arch, musl);
  if (!tags) {
    return undefined;
  }
  return files.find(
    (file) =>
      file.filename.endsWith(".whl") &&
      tags.every((tag) => file.filename.includes(tag)),
  );
}

/** Where `odl` is inside the wheel of a version. */
export function scriptInWheel(version: string, platform: string): string {
  const exe = platform === "win32" ? "odl.exe" : "odl";
  return `odoo_linter-${version}.data/scripts/${exe}`;
}
