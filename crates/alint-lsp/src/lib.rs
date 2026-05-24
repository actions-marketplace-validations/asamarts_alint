//! Language Server Protocol server for alint.
//!
//! A thin `tower-lsp` backend that runs the alint engine over the
//! workspace and publishes the resulting violations as LSP diagnostics.
//! It is driven by the `alint lsp` subcommand, speaking LSP over stdio
//! (see [`run_stdio`]).
//!
//! Two evaluation paths:
//!
//! - **Open / save** run the full [`alint_core::Engine`] over the
//!   workspace (cross-file rules included) and publish per-file
//!   diagnostics for every open document.
//! - **Change** uses the single-file hot path
//!   ([`alint_core::Engine::run_for_file`]) against the editor's
//!   in-memory bytes, so per-keystroke feedback costs one file's
//!   evaluation, not the whole tree's. Cross-file rules are not
//!   re-run on change (they refresh on the next save), matching
//!   `docs/design/v0.11/single_file_reevaluation.md`.
//!
//! Hover and code actions are deferred to later slices of the LSP epic.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, InitializeParams, InitializeResult,
    InitializedParams, MessageType, NumberOrString, Position, Range, ServerCapabilities,
    ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer, LspService, Server, jsonrpc::Result as JsonRpcResult};

use alint_core::{
    Engine, Error, FileIndex, Level, RuleEntry, RuleResult, Violation, WalkOptions, walk,
};

/// Per-file diagnostics keyed by absolute path.
type DiagnosticsByPath = HashMap<PathBuf, Vec<Diagnostic>>;

/// A loaded workspace: the config-built engine plus the walked index.
/// Cached on open/save and reused by the change hot path so a keystroke
/// doesn't re-load the config or re-walk the tree.
#[derive(Debug)]
struct Session {
    root: PathBuf,
    engine: Engine,
    index: FileIndex,
}

/// Build a tokio runtime and serve the alint language server over
/// stdio until the client disconnects. Called by the `alint lsp`
/// subcommand so the CLI itself stays synchronous.
pub fn run_stdio() -> std::io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let (service, socket) = LspService::new(Backend::new);
        Server::new(stdin, stdout, socket).serve(service).await;
    });
    Ok(())
}

#[derive(Debug)]
struct State {
    /// Workspace root, from the `initialize` handshake.
    root: Option<PathBuf>,
    /// URIs of documents the editor currently has open. Diagnostics
    /// are published (and cleared) for these.
    open: HashSet<Url>,
    /// Cached engine + index from the last full check. `None` until the
    /// first open/save; the change hot path needs it.
    session: Option<Arc<Session>>,
}

#[derive(Debug)]
struct Backend {
    client: Client,
    state: Mutex<State>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            state: Mutex::new(State {
                root: None,
                open: HashSet::new(),
                session: None,
            }),
        }
    }

    /// Full check: (re)build the session and publish per-file
    /// diagnostics for every open document, clearing those that no
    /// longer have findings. Runs on open and save.
    async fn check_and_publish(&self) {
        let (root, open) = {
            let state = self.state.lock().unwrap();
            (
                state.root.clone(),
                state.open.iter().cloned().collect::<Vec<_>>(),
            )
        };
        let Some(root) = root else {
            return;
        };

        match tokio::task::spawn_blocking(move || build_and_run(&root)).await {
            Ok(Ok(Some((session, by_path)))) => {
                self.state.lock().unwrap().session = Some(session);
                self.publish_each(&open, &by_path).await;
            }
            Ok(Ok(None)) => {
                // No `.alint.yml` — clear any stale diagnostics.
                self.state.lock().unwrap().session = None;
                self.publish_each(&open, &DiagnosticsByPath::new()).await;
            }
            Ok(Err(err)) => {
                self.client
                    .log_message(MessageType::WARNING, format!("alint: {err}"))
                    .await;
            }
            Err(join_err) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("alint: check panicked: {join_err}"),
                    )
                    .await;
            }
        }
    }

    /// Single-file hot path: re-evaluate per-file rules against the
    /// editor's in-memory `text` and publish diagnostics for just this
    /// document. Cross-file findings refresh on the next save.
    async fn reeval_file(&self, uri: Url, text: String) {
        let session = self.state.lock().unwrap().session.clone();
        let Some(session) = session else {
            return; // No cached session yet — open/save will populate it.
        };
        let Ok(abs) = uri.to_file_path() else {
            return;
        };
        let Ok(rel) = abs.strip_prefix(&session.root).map(Path::to_path_buf) else {
            return;
        };

        let abs_key = abs.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            session
                .engine
                .run_for_file(&session.root, &session.index, &rel, text.as_bytes())
                .map(|results| group_violations(&session.root, &results))
        })
        .await;

        match outcome {
            Ok(Ok(by_path)) => {
                let diagnostics = by_path.get(&abs_key).cloned().unwrap_or_default();
                self.client
                    .publish_diagnostics(uri, diagnostics, None)
                    .await;
            }
            Ok(Err(Error::FileNotInIndex { .. })) => {
                // Excluded from linting — clear any diagnostics.
                self.client.publish_diagnostics(uri, Vec::new(), None).await;
            }
            Ok(Err(err)) => {
                self.client
                    .log_message(MessageType::WARNING, format!("alint: {err}"))
                    .await;
            }
            Err(join_err) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("alint: re-eval panicked: {join_err}"),
                    )
                    .await;
            }
        }
    }

    /// Publish per-file diagnostics for each open document, clearing
    /// those absent from `by_path`.
    async fn publish_each(&self, open: &[Url], by_path: &DiagnosticsByPath) {
        for uri in open {
            let diagnostics = uri
                .to_file_path()
                .ok()
                .and_then(|abs| by_path.get(&abs).cloned())
                .unwrap_or_default();
            self.client
                .publish_diagnostics(uri.clone(), diagnostics, None)
                .await;
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> JsonRpcResult<InitializeResult> {
        if let Some(root) = workspace_root(&params) {
            self.state.lock().unwrap().root = Some(root);
        }
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "alint-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "alint language server ready")
            .await;
    }

    async fn shutdown(&self) -> JsonRpcResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.state
            .lock()
            .unwrap()
            .open
            .insert(params.text_document.uri);
        self.check_and_publish().await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        // FULL document sync → the last content change carries the
        // whole new text. Re-evaluate per-file rules against it.
        let Some(change) = params.content_changes.pop() else {
            return;
        };
        self.reeval_file(params.text_document.uri, change.text)
            .await;
    }

    async fn did_save(&self, _: DidSaveTextDocumentParams) {
        // Rebuild the session (the tree / config may have changed) and
        // re-run everything, including cross-file rules.
        self.check_and_publish().await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.state.lock().unwrap().open.remove(&uri);
        // Clear any diagnostics the editor is still showing.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }
}

/// Resolve the workspace root from the `initialize` params, preferring
/// the first workspace folder and falling back to the (deprecated)
/// `root_uri`.
fn workspace_root(params: &InitializeParams) -> Option<PathBuf> {
    if let Some(folders) = &params.workspace_folders {
        if let Some(first) = folders.first() {
            if let Ok(path) = first.uri.to_file_path() {
                return Some(path);
            }
        }
    }
    #[allow(deprecated)]
    params.root_uri.as_ref().and_then(|u| u.to_file_path().ok())
}

/// Load the workspace config and build the engine + index. Returns
/// `Ok(None)` (not an error) when no config is present so callers clear
/// stale diagnostics.
fn build_session(root: &Path) -> Result<Option<Session>, String> {
    let Some(config_path) = alint_dsl::discover(root) else {
        return Ok(None);
    };
    let config = alint_dsl::load(&config_path).map_err(|e| format!("loading config: {e}"))?;

    let registry = alint_rules::builtin_registry();
    let mut entries: Vec<RuleEntry> = Vec::with_capacity(config.rules.len());
    for spec in &config.rules {
        if matches!(spec.level, Level::Off) {
            continue;
        }
        let rule = registry
            .build(spec)
            .map_err(|e| format!("building rule {:?}: {e}", spec.id))?;
        let mut entry = RuleEntry::new(rule);
        if let Some(when_src) = &spec.when {
            let expr = alint_core::when::parse(when_src)
                .map_err(|e| format!("rule {:?}: parsing `when`: {e}", spec.id))?;
            entry = entry.with_when(expr);
        }
        entries.push(entry);
    }

    let engine = Engine::from_entries(entries, registry)
        .with_facts(config.facts)
        .with_vars(config.vars);

    let walk_opts = WalkOptions {
        respect_gitignore: config.respect_gitignore,
        extra_ignores: config.ignore,
    };
    let index = walk(root, &walk_opts).map_err(|e| format!("walking repository: {e}"))?;

    Ok(Some(Session {
        root: root.to_path_buf(),
        engine,
        index,
    }))
}

/// Build a session and run the full engine over it. `Ok(None)` ⇒ no
/// config (caller clears diagnostics).
fn build_and_run(root: &Path) -> Result<Option<(Arc<Session>, DiagnosticsByPath)>, String> {
    let Some(session) = build_session(root)? else {
        return Ok(None);
    };
    let report = session
        .engine
        .run(&session.root, &session.index)
        .map_err(|e| format!("running rules: {e}"))?;
    let by_path = group_violations(&session.root, &report.results);
    Ok(Some((Arc::new(session), by_path)))
}

/// Group rule-result violations into per-file LSP diagnostics keyed by
/// absolute path. File- and tree-level findings (no path) are skipped —
/// they have no document to attach to.
fn group_violations(root: &Path, results: &[RuleResult]) -> DiagnosticsByPath {
    let mut by_path = DiagnosticsByPath::new();
    for result in results {
        let Some(severity) = severity_of(result.level) else {
            continue;
        };
        for violation in &result.violations {
            let Some(rel) = &violation.path else {
                continue;
            };
            let abs = root.join(rel.as_ref());
            by_path
                .entry(abs)
                .or_default()
                .push(diagnostic(severity, &result.rule_id, violation));
        }
    }
    by_path
}

fn severity_of(level: Level) -> Option<DiagnosticSeverity> {
    match level {
        Level::Error => Some(DiagnosticSeverity::ERROR),
        Level::Warning => Some(DiagnosticSeverity::WARNING),
        Level::Info => Some(DiagnosticSeverity::INFORMATION),
        Level::Off => None,
    }
}

/// Map one alint violation to an LSP diagnostic. alint line/column are
/// 1-indexed and optional; LSP positions are 0-indexed. File- and
/// tree-level findings (no line) anchor at the start of the file.
fn diagnostic(severity: DiagnosticSeverity, rule_id: &str, violation: &Violation) -> Diagnostic {
    let line = violation
        .line
        .map_or(0, |l| u32::try_from(l.saturating_sub(1)).unwrap_or(0));
    let col = violation
        .column
        .map_or(0, |c| u32::try_from(c.saturating_sub(1)).unwrap_or(0));
    let start = Position::new(line, col);
    let end = Position::new(line, col.saturating_add(1));
    Diagnostic {
        range: Range::new(start, end),
        severity: Some(severity),
        code: Some(NumberOrString::String(rule_id.to_string())),
        source: Some("alint".to_string()),
        message: violation.message.to_string(),
        ..Diagnostic::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    fn violation(line: Option<usize>, column: Option<usize>) -> Violation {
        Violation {
            path: None,
            message: Cow::Borrowed("boom"),
            line,
            column,
            is_note: false,
        }
    }

    #[test]
    fn severity_maps_levels_and_drops_off() {
        assert_eq!(severity_of(Level::Error), Some(DiagnosticSeverity::ERROR));
        assert_eq!(
            severity_of(Level::Warning),
            Some(DiagnosticSeverity::WARNING)
        );
        assert_eq!(
            severity_of(Level::Info),
            Some(DiagnosticSeverity::INFORMATION)
        );
        assert_eq!(severity_of(Level::Off), None);
    }

    #[test]
    fn diagnostic_converts_one_indexed_position_to_zero_indexed() {
        let d = diagnostic(
            DiagnosticSeverity::ERROR,
            "my-rule",
            &violation(Some(4), Some(7)),
        );
        assert_eq!(d.range.start, Position::new(3, 6));
        assert_eq!(d.range.end, Position::new(3, 7));
        assert_eq!(d.source.as_deref(), Some("alint"));
        assert_eq!(d.code, Some(NumberOrString::String("my-rule".to_string())));
        assert_eq!(d.message, "boom");
    }

    #[test]
    fn diagnostic_without_line_anchors_at_file_start() {
        let d = diagnostic(DiagnosticSeverity::WARNING, "r", &violation(None, None));
        assert_eq!(d.range.start, Position::new(0, 0));
        assert_eq!(d.range.end, Position::new(0, 1));
    }

    #[test]
    fn build_session_returns_none_when_no_config() {
        let dir = tempfile::tempdir().unwrap();
        assert!(build_session(dir.path()).unwrap().is_none());
    }
}
