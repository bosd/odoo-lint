import assert from "node:assert/strict";
import { test } from "node:test";
import { pickWheel, scriptInWheel } from "../src/wheel";

// The file names of a real release (odoo-linter 0.1.0a7).
const files = [
  "odoo_linter-0.1.0a7-py3-none-macosx_10_12_x86_64.whl",
  "odoo_linter-0.1.0a7-py3-none-macosx_11_0_arm64.whl",
  "odoo_linter-0.1.0a7-py3-none-manylinux_2_17_aarch64.manylinux2014_aarch64.whl",
  "odoo_linter-0.1.0a7-py3-none-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
  "odoo_linter-0.1.0a7-py3-none-musllinux_1_2_aarch64.whl",
  "odoo_linter-0.1.0a7-py3-none-musllinux_1_2_x86_64.whl",
  "odoo_linter-0.1.0a7-py3-none-win_amd64.whl",
  "odoo_linter-0.1.0a7.tar.gz",
].map((filename) => ({
  filename,
  url: `https://files.example/${filename}`,
  digests: { sha256: "" },
}));

test("picks the wheel for each platform", () => {
  const picked = (platform: string, arch: string) =>
    pickWheel(files, platform, arch)?.filename;
  assert.equal(picked("linux", "x64"), files[3].filename);
  assert.equal(picked("linux", "arm64"), files[2].filename);
  assert.equal(picked("darwin", "arm64"), files[1].filename);
  assert.equal(picked("darwin", "x64"), files[0].filename);
  assert.equal(picked("win32", "x64"), files[6].filename);
});

test("has no wheel for other platforms", () => {
  assert.equal(pickWheel(files, "win32", "arm64"), undefined);
  assert.equal(pickWheel(files, "freebsd", "x64"), undefined);
});

test("finds odl in the wheel", () => {
  assert.equal(
    scriptInWheel("0.1.0a7", "linux"),
    "odoo_linter-0.1.0a7.data/scripts/odl",
  );
  assert.equal(
    scriptInWheel("0.1.0a7", "win32"),
    "odoo_linter-0.1.0a7.data/scripts/odl.exe",
  );
});
