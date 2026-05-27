//! Focused `alint lsp` protocol-contract tests (companion to the linear
//! `lsp_smoke.rs` script). Each pins one behavior every editor client
//! depends on:
//!   1. the diagnostic shape (rule id in `code`, level → `severity`),
//!   2. workspace-root resolution via `workspace_folders` (the branch
//!      the server checks first, before `rootUri`),
//!   3. that an apply-fix `WorkspaceEdit`'s own `newText` actually
//!      resolves the violation when fed back to the server,
//!   4. config discovery from an ancestor when the client roots at a
//!      subdirectory.
//!
//! Unix-only for the same `file://` reason as the shared harness.
#![cfg(unix)]

mod lsp_common;

use serde_json::json;

use lsp_common::{recv_response, spawn_server, wait_for_diagnostics};

/// A config with one error-level (unfixable) and one warning-level
/// (fixable) per-file rule. Returned content trips both on a `.txt`.
const TWO_RULE_CONFIG: &str = "version: 1\nrules:\n  \
     - id: no-todo\n    kind: file_content_forbidden\n    \
     paths: \"**/*.txt\"\n    pattern: 'TODO'\n    level: error\n  \
     - id: clean-ws\n    kind: no_trailing_whitespace\n    \
     paths: \"**/*.txt\"\n    level: warning\n    fix:\n      \
     file_trim_trailing_whitespace: {}\n";

fn find_by_code<'a>(diags: &'a [serde_json::Value], code: &str) -> &'a serde_json::Value {
    diags
        .iter()
        .find(|d| d["code"] == code)
        .unwrap_or_else(|| panic!("no diagnostic with code {code:?} in {diags:?}"))
}

/// (1) The diagnostic contract: rule id surfaces in `code`, the rule's
/// level maps to the LSP `severity` integer (Error=1, Warning=2), and
/// every diagnostic carries `source: "alint"` and a real range.
#[test]
fn lsp_diagnostic_shape_pins_code_and_severity() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), TWO_RULE_CONFIG).unwrap();
    let text = "has a TODO here   \n";
    std::fs::write(root.join("a.txt"), text).unwrap();

    let root_uri = format!("file://{}", root.to_str().unwrap());
    let file_uri = format!("{root_uri}/a.txt");

    let mut server = spawn_server(root);
    server.send(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "rootUri": root_uri, "capabilities": {} }
    }));
    recv_response(&server.rx, 1);
    server.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": file_uri, "languageId": "plaintext", "version": 1, "text": text
        }}
    }));

    let diags = wait_for_diagnostics(&server.rx, &file_uri);
    assert_eq!(diags.len(), 2, "expected both rules to fire: {diags:?}");

    let todo = find_by_code(&diags, "no-todo");
    assert_eq!(todo["severity"], 1, "error level → LSP severity 1");
    assert_eq!(todo["source"], "alint");

    let ws = find_by_code(&diags, "clean-ws");
    assert_eq!(ws["severity"], 2, "warning level → LSP severity 2");
    assert_eq!(ws["source"], "alint");

    // Every diagnostic carries a structured range.
    for d in &diags {
        assert!(
            d["range"]["start"]["line"].is_number(),
            "range.start.line: {d:?}"
        );
        assert!(
            d["range"]["end"]["character"].is_number(),
            "range.end.character: {d:?}"
        );
    }

    server.send(&json!({ "jsonrpc": "2.0", "id": 9, "method": "shutdown", "params": null }));
    server.send(&json!({ "jsonrpc": "2.0", "method": "exit", "params": null }));
}

/// (2) Root resolution via `workspace_folders` (no `rootUri`). The
/// server prefers `workspace_folders[0]` over `root_uri` (lib.rs:530),
/// so a client that only sends folders must still discover the config.
#[test]
fn lsp_workspace_folders_root_resolves_config() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), TWO_RULE_CONFIG).unwrap();
    let text = "TODO and trailing   \n";
    std::fs::write(root.join("a.txt"), text).unwrap();

    let root_uri = format!("file://{}", root.to_str().unwrap());
    let file_uri = format!("{root_uri}/a.txt");

    let mut server = spawn_server(root);
    server.send(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "workspaceFolders": [{ "uri": root_uri, "name": "fixture" }],
            "capabilities": {}
        }
    }));
    recv_response(&server.rx, 1);
    server.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": file_uri, "languageId": "plaintext", "version": 1, "text": text
        }}
    }));

    let diags = wait_for_diagnostics(&server.rx, &file_uri);
    assert!(
        !diags.is_empty(),
        "workspaceFolders root should discover .alint.yml and fire, got none"
    );

    server.send(&json!({ "jsonrpc": "2.0", "id": 9, "method": "shutdown", "params": null }));
    server.send(&json!({ "jsonrpc": "2.0", "method": "exit", "params": null }));
}

/// (3) Apply-fix correctness: take the quick-fix `WorkspaceEdit`'s own
/// `newText`, feed it back as the buffer, and assert the fixable
/// violation clears. This verifies the *proposed* fix actually resolves
/// the finding (not just that some clean text would), exercising the
/// content-fix path end to end.
#[test]
fn lsp_apply_fix_resolves_violation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // Only the fixable rule, so the corrected buffer is fully clean.
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  \
         - id: clean-ws\n    kind: no_trailing_whitespace\n    \
         paths: \"**/*.txt\"\n    level: warning\n    fix:\n      \
         file_trim_trailing_whitespace: {}\n",
    )
    .unwrap();
    let dirty = "trailing ws here   \n";
    std::fs::write(root.join("b.txt"), dirty).unwrap();

    let root_uri = format!("file://{}", root.to_str().unwrap());
    let file_uri = format!("{root_uri}/b.txt");

    let mut server = spawn_server(root);
    server.send(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "rootUri": root_uri, "capabilities": {} }
    }));
    recv_response(&server.rx, 1);
    server.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": file_uri, "languageId": "plaintext", "version": 1, "text": dirty
        }}
    }));
    let diags = wait_for_diagnostics(&server.rx, &file_uri);
    assert_eq!(
        diags.len(),
        1,
        "expected the trailing-ws diagnostic: {diags:?}"
    );
    let diag_range = diags[0]["range"].clone();

    // Ask for code actions over the finding's own range, passing the
    // diagnostic in context.
    server.send(&json!({
        "jsonrpc": "2.0", "id": 2, "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": file_uri },
            "range": diag_range,
            "context": { "diagnostics": diags }
        }
    }));
    let actions = recv_response(&server.rx, 2);
    let actions = actions.as_array().cloned().unwrap_or_default();
    let fix = actions
        .iter()
        .find(|a| a["kind"] == "quickfix" && a["edit"].is_object())
        .expect("expected an apply-fix code action with a WorkspaceEdit");

    // Pull the fix's OWN proposed replacement text out of the
    // WorkspaceEdit `changes` map and feed it back as the buffer.
    let edits = fix["edit"]["changes"][file_uri.as_str()]
        .as_array()
        .cloned()
        .expect("WorkspaceEdit.changes[file] should be an array of TextEdits");
    let new_text = edits[0]["newText"]
        .as_str()
        .expect("TextEdit.newText should be a string")
        .to_string();
    assert!(
        !new_text.contains("   \n"),
        "the proposed fix should strip trailing whitespace, got {new_text:?}"
    );

    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": file_uri, "version": 2 },
            "contentChanges": [{ "text": new_text }]
        }
    }));
    let after = wait_for_diagnostics(&server.rx, &file_uri);
    assert!(
        after.is_empty(),
        "applying the fix's own newText should clear the violation, got {after:?}"
    );

    server.send(&json!({ "jsonrpc": "2.0", "id": 9, "method": "shutdown", "params": null }));
    server.send(&json!({ "jsonrpc": "2.0", "method": "exit", "params": null }));
}

/// (4) Root discovery from a subfolder: a client that opens a *subdir*
/// as its workspace (Sublime/Eglot/Helix have no uniform root marker)
/// must still pick up the repo-root `.alint.yml` from an ancestor and
/// lint correctly. The server roots at the discovered config's
/// directory, so a file in the subdir still gets diagnostics.
#[test]
fn lsp_discovers_config_in_ancestor_when_rooted_at_subdir() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::write(repo.join(".alint.yml"), TWO_RULE_CONFIG).unwrap();
    let sub = repo.join("sub");
    std::fs::create_dir(&sub).unwrap();
    let text = "has a TODO here   \n";
    std::fs::write(sub.join("bad.py"), "# TODO\n").unwrap();
    // (.py so the no-todo rule's **/*.py glob matches under the repo root)
    std::fs::write(sub.join("bad.txt"), text).unwrap();

    // Root the client at the SUBDIR, not the repo root.
    let sub_uri = format!("file://{}", sub.to_str().unwrap());
    let file_uri = format!("{sub_uri}/bad.txt");

    let mut server = spawn_server(&sub);
    server.send(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "rootUri": sub_uri, "capabilities": {} }
    }));
    recv_response(&server.rx, 1);
    server.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": file_uri, "languageId": "plaintext", "version": 1, "text": text
        }}
    }));

    let diags = wait_for_diagnostics(&server.rx, &file_uri);
    assert!(
        !diags.is_empty(),
        "rooting at a subdir should still discover the ancestor .alint.yml and fire, got none"
    );

    server.send(&json!({ "jsonrpc": "2.0", "id": 9, "method": "shutdown", "params": null }));
    server.send(&json!({ "jsonrpc": "2.0", "method": "exit", "params": null }));
}
