// odoo-lint for VS Code: runs `odl server`, odoo-lint's language server, for
// Python, XML and translation (.po/.pot) files. Findings, quick fixes and the
// "Fix all" source action come from the server.

import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";
import { findOdl } from "./binary";

let client: LanguageClient | undefined;
let log: vscode.LogOutputChannel;

async function start(context: vscode.ExtensionContext): Promise<void> {
  let command: string;
  try {
    command = await findOdl(context, log);
  } catch (error) {
    log.error(String(error));
    void vscode.window.showErrorMessage(
      `odoo-lint: ${error instanceof Error ? error.message : error}`,
    );
    return;
  }
  log.info(`Using ${command}`);
  const serverOptions: ServerOptions = { command, args: ["server"] };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "python" },
      { scheme: "file", language: "xml" },
      { scheme: "file", pattern: "**/*.{po,pot}" },
    ],
    outputChannel: log,
  };
  client = new LanguageClient(
    "odoo-lint",
    "odoo-lint",
    serverOptions,
    clientOptions,
  );
  await client.start();
}

async function stop(): Promise<void> {
  const running = client;
  client = undefined;
  await running?.stop();
}

// Starts and stops run one after the other, so a restart never stops a
// server that is still starting.
let queue: Promise<void> = Promise.resolve();

function enqueue(task: () => Promise<void>): Promise<void> {
  queue = queue.then(task).catch((error) => log.error(String(error)));
  return queue;
}

export function activate(context: vscode.ExtensionContext): Promise<void> {
  log = vscode.window.createOutputChannel("odoo-lint", { log: true });
  const restart = () =>
    enqueue(async () => {
      await stop();
      await start(context);
    });
  context.subscriptions.push(
    log,
    vscode.commands.registerCommand("odoo-lint.restart", restart),
    vscode.commands.registerCommand("odoo-lint.showLogs", () => log.show()),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("odoo-lint")) {
        void restart();
      }
    }),
  );
  return enqueue(() => start(context));
}

export function deactivate(): Promise<void> {
  return enqueue(stop);
}
