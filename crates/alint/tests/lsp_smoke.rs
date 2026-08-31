//! End-to-end smoke test for the `alint lsp` language server.
//!
//! Spawns the real binary, speaks LSP over stdio, and drives the full
//! loop: open → diagnostics → hover → code action (apply-fix
//! `WorkspaceEdit`) → change → cleared → save → re-run. Asserts on the
//! responses, and records the change→publish round-trip latency as a
//! coarse performance check.
//!
//! Unix-only: it builds `file://` URIs by hand, which would need
//! drive-letter handling on Windows. The server itself is
//! cross-platform; this is a test-harness simplification.
#![cfg(unix)]

mod lsp_common;

use std::time::Instant;

use serde_json::json;

use lsp_common::{RECV_TIMEOUT, recv_response, spawn_server, wait_for_diagnostics};

#[test]
#[allow(clippy::too_many_lines)] // one linear end-to-end LSP script
fn lsp_open_hover_codeaction_change_save_e2e() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // Two per-file rules: a forbidden-pattern rule (no fixer) and a
    // trailing-whitespace rule WITH a fixer (so code actions have
    // something to offer).
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  \
         - id: no-todo\n    kind: file_content_forbidden\n    \
         paths: \"**/*.txt\"\n    pattern: 'TODO'\n    level: error\n  \
         - id: clean-ws\n    kind: no_trailing_whitespace\n    \
         paths: \"**/*.txt\"\n    level: warning\n    fix:\n      \
         file_trim_trailing_whitespace: {}\n",
    )
    .unwrap();
    // Dirty content: contains TODO AND has trailing whitespace.
    let dirty = "has a TODO here   \n";
    std::fs::write(root.join("a.txt"), dirty).unwrap();

    let root_uri = format!("file://{}", root.to_str().unwrap());
    let file_uri = format!("{root_uri}/a.txt");

    let mut server = spawn_server(root);

    // initialize / initialized handshake.
    server.send(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "rootUri": root_uri, "capabilities": {} }
    }));
    // Wait for the initialize result before driving the session (proper
    // LSP client handshake).
    recv_response(&server.rx, 1);
    server.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));

    // didOpen the violating file → expect diagnostics.
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": file_uri, "languageId": "plaintext", "version": 1,
            "text": dirty
        }}
    }));
    let diags = wait_for_diagnostics(&server.rx, &file_uri);
    assert!(!diags.is_empty(), "expected a diagnostic on open, got none");
    assert_eq!(diags[0]["source"], "alint");

    // hover over the first diagnostic's marker → markdown with the rule.
    let pos = &diags[0]["range"]["start"];
    server.send(&json!({
        "jsonrpc": "2.0", "id": 2, "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": file_uri },
            "position": { "line": pos["line"], "character": pos["character"] }
        }
    }));
    let hover = recv_response(&server.rx, 2);
    let hover_text = hover["contents"]["value"].as_str().unwrap_or_default();
    assert!(
        hover_text.contains("alint"),
        "hover should render the alint finding, got: {hover_text:?}"
    );

    // codeAction over line 0 → a quick-fix with a WorkspaceEdit (the
    // trailing-whitespace fix; the forbidden-pattern rule has no fixer).
    server.send(&json!({
        "jsonrpc": "2.0", "id": 3, "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": file_uri },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 80 }
            },
            "context": { "diagnostics": [] }
        }
    }));
    let actions = recv_response(&server.rx, 3);
    let actions = actions.as_array().cloned().unwrap_or_default();
    let fix = actions
        .iter()
        .find(|a| a["kind"] == "quickfix" && a["edit"].is_object())
        .expect("expected an apply-fix code action with a WorkspaceEdit");
    assert!(
        fix["title"]
            .as_str()
            .unwrap_or_default()
            .contains("alint: fix"),
        "code action title: {:?}",
        fix["title"]
    );
    assert!(
        fix["edit"]["changes"].is_object(),
        "content fix should use the WorkspaceEdit `changes` map: {:?}",
        fix["edit"]
    );

    // didChange to clean content → per-file diagnostics cleared.
    let start = Instant::now();
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": file_uri, "version": 2 },
            "contentChanges": [{ "text": "all clean now\n" }]
        }
    }));
    let after_change = wait_for_diagnostics(&server.rx, &file_uri);
    let roundtrip = start.elapsed();
    assert!(
        after_change.is_empty(),
        "expected diagnostics cleared after fixing the buffer, got {after_change:?}"
    );
    assert!(
        roundtrip < RECV_TIMEOUT,
        "change->publish round-trip {roundtrip:?} exceeded {RECV_TIMEOUT:?}"
    );

    // didSave: write the cleaned content to disk and save → the full
    // run reads disk and republishes (now clean → diagnostics cleared).
    std::fs::write(root.join("a.txt"), "all clean now\n").unwrap();
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didSave",
        "params": { "textDocument": { "uri": file_uri } }
    }));
    let after_save = wait_for_diagnostics(&server.rx, &file_uri);
    assert!(
        after_save.is_empty(),
        "expected diagnostics cleared after saving clean content, got {after_save:?}"
    );

    // Graceful shutdown.
    server.send(&json!({ "jsonrpc": "2.0", "id": 4, "method": "shutdown", "params": null }));
    server.send(&json!({ "jsonrpc": "2.0", "method": "exit", "params": null }));
}
