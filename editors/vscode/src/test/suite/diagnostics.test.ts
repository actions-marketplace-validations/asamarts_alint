// Real-client e2e: opens the violating fixture file, waits for the
// alint language server to publish diagnostics, and asserts the
// diagnostic shape + that a fixable rule offers an apply-fix code
// action. The extension launches the prebuilt binary via the
// `alint.path` written into the temp workspace settings by runTest.ts.

import * as assert from 'assert';

import * as vscode from 'vscode';

const EXTENSION_ID = 'asamarts.alint';

async function waitForDiagnostics(
  uri: vscode.Uri,
  timeoutMs = 60_000,
): Promise<vscode.Diagnostic[]> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const diags = vscode.languages.getDiagnostics(uri);
    if (diags.length > 0) {
      return diags;
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  return vscode.languages.getDiagnostics(uri);
}

function badFileUri(): vscode.Uri {
  const folder = vscode.workspace.workspaceFolders?.[0];
  assert.ok(folder, 'expected an open workspace folder');
  return vscode.Uri.joinPath(folder.uri, 'bad.txt');
}

describe('alint VS Code extension (e2e)', function () {
  // Generous: first run spawns the LSP and the host is cold.
  this.timeout(90_000);

  before(async () => {
    const ext = vscode.extensions.getExtension(EXTENSION_ID);
    assert.ok(ext, `extension ${EXTENSION_ID} not found`);
    await ext.activate();
    const doc = await vscode.workspace.openTextDocument(badFileUri());
    await vscode.window.showTextDocument(doc);
  });

  it('publishes diagnostics from the alint language server', async () => {
    const diags = await waitForDiagnostics(badFileUri());
    assert.ok(diags.length > 0, 'expected diagnostics from the alint LSP, got none');
    assert.ok(
      diags.some((d) => d.source === 'alint'),
      `expected an alint-sourced diagnostic, got ${JSON.stringify(diags)}`,
    );
    assert.ok(
      diags.some((d) => String(d.code) === 'no-todo'),
      `expected the no-todo rule id in a diagnostic code, got ${JSON.stringify(
        diags.map((d) => d.code),
      )}`,
    );
  });

  it('offers an apply-fix quick action for the fixable rule', async () => {
    const uri = badFileUri();
    const diags = await waitForDiagnostics(uri);
    const fixable = diags.find((d) => String(d.code) === 'clean-ws');
    assert.ok(fixable, 'expected the clean-ws (fixable) diagnostic');

    const actions = await vscode.commands.executeCommand<vscode.CodeAction[]>(
      'vscode.executeCodeActionProvider',
      uri,
      fixable.range,
      vscode.CodeActionKind.QuickFix.value,
    );
    assert.ok(actions && actions.length > 0, 'expected at least one quick-fix code action');
    assert.ok(
      actions.some((a) => a.edit !== undefined),
      'expected a quick-fix carrying a WorkspaceEdit',
    );
  });
});
