//! End-to-end smoke test for the `alint lsp` language server.
//!
//! Spawns the real binary, speaks LSP over stdio, and drives the
//! open → diagnostics → change → cleared-diagnostics loop, asserting on
//! the published diagnostics. Also records the change→publish round-trip
//! latency as a coarse performance check.
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

/// Block until the response to request `id` arrives.
fn wait_for_response(rx: &Receiver<Value>, id: i64) {
    let deadline = Instant::now() + RECV_TIMEOUT;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for initialize response");
        let msg = rx
            .recv_timeout(remaining)
            .expect("timed out waiting for initialize response");
        if msg["id"] == id {
            return;
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
fn lsp_open_change_diagnostics_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
         paths: \"**/*.txt\"\n    pattern: 'TODO'\n    level: error\n",
    )
    .unwrap();
    std::fs::write(root.join("a.txt"), "has a TODO here\n").unwrap();

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
    wait_for_response(&server.rx, 1);
    server.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));

    // didOpen the violating file → expect a diagnostic.
    server.send(&json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": file_uri, "languageId": "plaintext", "version": 1,
            "text": "has a TODO here\n"
        }}
    }));
    let diags = wait_for_diagnostics(&server.rx, &file_uri);
    assert!(!diags.is_empty(), "expected a diagnostic on open, got none");
    assert_eq!(diags[0]["source"], "alint");

    // didChange to clean content → expect diagnostics cleared.
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
        "expected diagnostics cleared after fixing the file, got {after_change:?}"
    );
    // Coarse perf smoke (not the design's strict p95): the single-file
    // hot path should round-trip well under the recv timeout.
    assert!(
        roundtrip < RECV_TIMEOUT,
        "change->publish round-trip {roundtrip:?} exceeded {RECV_TIMEOUT:?}"
    );

    // Graceful shutdown.
    server.send(&json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": null }));
    server.send(&json!({ "jsonrpc": "2.0", "method": "exit", "params": null }));
}
