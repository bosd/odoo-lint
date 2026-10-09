// Runs inside VS Code: opens a manifest without a license and waits for
// odoo-lint's finding and its "Fix all" action.

import assert from "node:assert/strict";
import * as path from "node:path";
import * as vscode from "vscode";

async function waitFor<T>(
  what: string,
  get: () => T | undefined | Promise<T | undefined>,
): Promise<T> {
  for (let attempt = 0; attempt < 100; attempt++) {
    // Requests can be cancelled while the server starts; try again.
    const value = await Promise.resolve(get()).catch(() => undefined);
    if (value) {
      return value;
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`timed out waiting for ${what}`);
}

export async function run(): Promise<void> {
  await vscode.workspace
    .getConfiguration("odoo-lint")
    .update("path", process.env.ODL, vscode.ConfigurationTarget.Global);
  const folder = vscode.workspace.workspaceFolders![0].uri.fsPath;
  const uri = vscode.Uri.file(
    path.join(folder, "demo_addon", "__manifest__.py"),
  );
  await vscode.window.showTextDocument(uri);

  const finding = await waitFor("the C8102 diagnostic", () =>
    vscode.languages
      .getDiagnostics(uri)
      .find(
        (d) =>
          d.source === "odoo-lint" && JSON.stringify(d.code).includes("C8102"),
      ),
  );
  assert.match(finding.message, /license/);

  const kind = vscode.CodeActionKind.SourceFixAll.append("odoo-lint");
  const actions = await waitFor("the Fix all action", async () => {
    const found = await vscode.commands.executeCommand<vscode.CodeAction[]>(
      "vscode.executeCodeActionProvider",
      uri,
      new vscode.Range(0, 0, 0, 0),
      kind.value,
    );
    return found?.length ? found : undefined;
  });
  assert.ok(actions.some((action) => action.kind?.value === kind.value));
  console.log(
    `odoo-lint: ${finding.message}; ${actions.length} Fix all action(s)`,
  );
}
