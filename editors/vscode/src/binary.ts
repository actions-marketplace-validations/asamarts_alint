// Binary resolution. Consistent with the other install channels:
//   1. the `alint.path` setting (explicit override),
//   2. `alint` on PATH,
//   3. a previously downloaded copy in the extension's global storage,
//   4. otherwise prompt the user to download or locate one.
// Auto-download is opt-in (the prompt) — never silent.

import * as fs from "node:fs";
import * as path from "node:path";

import { type ExtensionContext, type OutputChannel, window, workspace } from "vscode";

import { downloadAlint } from "./download";
import { binaryName } from "./target";

export async function resolveAlintBinary(
  context: ExtensionContext,
  log: OutputChannel,
): Promise<string | undefined> {
  // 1. Explicit setting.
  const configured = workspace.getConfiguration("alint").get<string>("path")?.trim();
  if (configured) {
    if (fs.existsSync(configured)) {
      return configured;
    }
    window.showWarningMessage(`alint.path points to a missing file: ${configured}`);
  }

  // 2. PATH.
  const onPath = findOnPath();
  if (onPath) {
    return onPath;
  }

  // 3. Previously downloaded copy.
  const cached = cachedBinaryPath(context);
  if (fs.existsSync(cached)) {
    return cached;
  }

  // 4. Prompt (opt-in download).
  const choice = await window.showInformationMessage(
    "alint was not found on your PATH. Download the matching release, or locate an existing binary?",
    "Download",
    "Locate…",
  );
  if (choice === "Download") {
    try {
      const version = (context.extension.packageJSON as { version: string }).version;
      const dest = await downloadAlint(version, path.dirname(cached), (m) =>
        log.appendLine(`[download] ${m}`),
      );
      return dest;
    } catch (err) {
      window.showErrorMessage(`alint download failed: ${(err as Error).message}`);
      return undefined;
    }
  }
  if (choice === "Locate…") {
    const picked = await window.showOpenDialog({
      canSelectMany: false,
      openLabel: "Select alint binary",
    });
    if (picked && picked[0]) {
      return picked[0].fsPath;
    }
  }
  return undefined;
}

function cachedBinaryPath(context: ExtensionContext): string {
  return path.join(context.globalStorageUri.fsPath, "bin", binaryName(process.platform));
}

/** First directory on PATH that contains the alint binary, if any. */
function findOnPath(): string | undefined {
  const name = binaryName(process.platform);
  const pathVar = process.env.PATH ?? "";
  const sep = process.platform === "win32" ? ";" : ":";
  for (const dir of pathVar.split(sep)) {
    if (!dir) {
      continue;
    }
    const candidate = path.join(dir, name);
    try {
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    } catch {
      // Unreadable PATH entry — skip it.
    }
  }
  return undefined;
}
