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

use std::io::{BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const RECV_TIMEOUT: Duration = Duration::from_secs(20);

fn frame(msg: &Value) -> Vec<u8> {
    let body = msg.to_string();
    format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

/// Spawn a reader thread that parses framed LSP messages off `stdout`
/// and forwards each as a JSON value.
fn spawn_reader(stdout: std::process::ChildStdout) -> Receiver<Value> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            // Read headers up to the blank line.
            let mut content_len = 0usize;
            loop {
                let mut line = Vec::new();
                // read a single line terminated by \n
                let mut byte = [0u8; 1];
                loop {
                    match reader.read_exact(&mut byte) {
                        Ok(()) => {
                            line.push(byte[0]);
                            if byte[0] == b'\n' {
                                break;
                            }
                        }
                        Err(_) => return,
                    }
                }
                let text = String::from_utf8_lossy(&line);
                let trimmed = text.trim_end();
                if trimmed.is_empty() {
                    break;
                }
                if let Some(n) = trimmed.strip_prefix("Content-Length:") {
                    content_len = n.trim().parse().unwrap_or(0);
                }
            }
            if content_len == 0 {
                continue;
            }
            let mut buf = vec![0u8; content_len];
            if reader.read_exact(&mut buf).is_err() {
                return;
            }
            if let Ok(value) = serde_json::from_slice::<Value>(&buf) {
                if tx.send(value).is_err() {
                    return;
                }
            }
        }
    });
    rx
}

/// Block until the response to request `id` arrives, returning its
/// `result`.
fn recv_response(rx: &Receiver<Value>, id: i64) -> Value {
    let deadline = Instant::now() + RECV_TIMEOUT;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for response");
        let msg = rx
            .recv_timeout(remaining)
            .expect("timed out waiting for response");
        if msg["id"] == id {
            return msg["result"].clone();
        }
    }
}

/// Block until a `textDocument/publishDiagnostics` notification for
/// `uri` arrives, returning its `diagnostics` array.
fn wait_for_diagnostics(rx: &Receiver<Value>, uri: &str) -> Vec<Value> {
    let deadline = Instant::now() + RECV_TIMEOUT;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for publishDiagnostics");
        let msg = rx
            .recv_timeout(remaining)
            .expect("timed out waiting for publishDiagnostics");
        if msg["method"] == "textDocument/publishDiagnostics" && msg["params"]["uri"] == uri {
            return msg["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        }
    }
}

struct Server {
    child: Child,
    stdin: std::process::ChildStdin,
    rx: Receiver<Value>,
}

impl Server {
    fn send(&mut self, msg: &Value) {
        self.stdin.write_all(&frame(msg)).unwrap();
        self.stdin.flush().unwrap();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

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

    let mut child = Command::new(env!("CARGO_BIN_EXE_alint"))
        .arg("lsp")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn alint lsp");
    let stdin = child.stdin.take().unwrap();
    let rx = spawn_reader(child.stdout.take().unwrap());
    let mut server = Server { child, stdin, rx };

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
