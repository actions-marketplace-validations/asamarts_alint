//! Language Server Protocol server for alint.
//!
//! A thin `tower-lsp` backend that runs the existing alint engine over
//! the workspace and publishes the resulting violations as LSP
//! diagnostics. It is driven by the `alint lsp` subcommand, speaking
//! LSP over stdio (see [`run_stdio`]).
//!
//! This is the v0.11 *scaffold*: it advertises full-document sync and
//! refreshes diagnostics on open and save by running the full
//! [`alint_core::Engine`] over the workspace. Per-edit (`didChange`)
//! re-evaluation and the single-file hot path are deferred to the
//! `single_file_reevaluation` design; hover and code actions are
//! deferred to later slices of the LSP epic.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, InitializeParams, InitializeResult,
    InitializedParams, MessageType, NumberOrString, Position, Range, ServerCapabilities,
    ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer, LspService, Server, jsonrpc::Result as JsonRpcResult};

use alint_core::{Engine, Level, RuleEntry, WalkOptions, walk};

/// Per-file diagnostics keyed by absolute path.
type DiagnosticsByPath = HashMap<PathBuf, Vec<Diagnostic>>;

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
            }),
        }
    }

    /// Run the full engine over the workspace (off the async runtime)
    /// and publish per-file diagnostics for every open document,
    /// clearing those that no longer have findings.
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

        let by_path = match tokio::task::spawn_blocking(move || run_check(&root)).await {
            Ok(Ok(map)) => map,
            Ok(Err(err)) => {
                self.client
                    .log_message(MessageType::WARNING, format!("alint: {err}"))
                    .await;
                return;
            }
            Err(join_err) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("alint: check panicked: {join_err}"),
                    )
                    .await;
                return;
            }
        };

        for uri in open {
            let diagnostics = uri
                .to_file_path()
                .ok()
                .and_then(|abs| by_path.get(&abs).cloned())
                .unwrap_or_default();
            self.client
                .publish_diagnostics(uri, diagnostics, None)
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

    async fn did_change(&self, _: DidChangeTextDocumentParams) {
        // Scaffold: live re-evaluation on every keystroke would
        // trigger a full repo walk. Diagnostics refresh on save until
        // the single-file hot path lands (single_file_reevaluation).
    }

    async fn did_save(&self, _: DidSaveTextDocumentParams) {
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

/// Load the workspace config, run the engine, and group the resulting
/// violations into per-file LSP diagnostics keyed by absolute path.
/// Returns an empty map (not an error) when no config is present so
/// callers clear stale diagnostics.
fn run_check(root: &Path) -> Result<DiagnosticsByPath, String> {
    let Some(config_path) = alint_dsl::discover(root) else {
        return Ok(DiagnosticsByPath::new());
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
    let report = engine
        .run(root, &index)
        .map_err(|e| format!("running rules: {e}"))?;

    let mut by_path = DiagnosticsByPath::new();
    for result in &report.results {
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
    Ok(by_path)
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
fn diagnostic(
    severity: DiagnosticSeverity,
    rule_id: &str,
    violation: &alint_core::Violation,
) -> Diagnostic {
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
    use alint_core::Violation;
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
    fn run_check_returns_empty_map_when_no_config() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_check(dir.path()).unwrap();
        assert!(result.is_empty());
    }
}
