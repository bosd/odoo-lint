// Starts VS Code with the extension and test/fixture as the workspace, and
// runs test/suite in it. ODL is the `odl` to test with.

import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { runTests } from "@vscode/test-electron";

async function main(): Promise<void> {
  const root = path.resolve(__dirname, "..");
  if (!process.env.ODL) {
    throw new Error("Set ODL to the path of the odl to test with");
  }
  // A profile of its own, set up before VS Code starts: changing the setting
  // from the test would restart the server while it starts.
  const userData = fs.mkdtempSync(path.join(os.tmpdir(), "odoo-lint-vscode-"));
  fs.mkdirSync(path.join(userData, "User"));
  fs.writeFileSync(
    path.join(userData, "User", "settings.json"),
    JSON.stringify({
      "odoo-lint.path": path.resolve(process.env.ODL),
      "odoo-lint.download": false,
    }),
  );
  try {
    await runTests({
      extensionDevelopmentPath: root,
      extensionTestsPath: path.join(__dirname, "suite", "index.js"),
      launchArgs: [
        path.join(root, "test", "fixture"),
        "--disable-extensions",
        `--user-data-dir=${userData}`,
      ],
    });
  } finally {
    fs.rmSync(userData, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
