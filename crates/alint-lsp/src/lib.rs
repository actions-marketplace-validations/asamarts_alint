//! Language Server Protocol server for alint.
//!
//! A thin `tower-lsp` backend that runs the alint engine over the
//! workspace and publishes the resulting violations as LSP diagnostics.
//! It is driven by the `alint lsp` subcommand, speaking LSP over stdio
//! (see [`run_stdio`]).
//!
//! Evaluation paths:
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
//! - **Hover** over a violation marker renders the rule id, message,
//!   and `policy_url` from the per-file cache of the last-published
//!   findings.
//!
//! Code actions are deferred to a later slice of the LSP epic.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tower_lsp::lsp_types::{
    CodeDescription, Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams, Hover,
    HoverContents, HoverParams, HoverProviderCapability, InitializeParams, InitializeResult,
    InitializedParams, MarkupContent, MarkupKind, MessageType, NumberOrString, Position, Range,
    ServerCapabilities, ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer, LspService, Server, jsonrpc::Result as JsonRpcResult};

use alint_core::{
    Engine, Error, FileIndex, Level, RuleEntry, RuleResult, Violation, WalkOptions, walk,
};

/// One cached finding for a file: enough to publish a diagnostic and to
/// render a hover. Kept per URI in [`State::diagnostics`] so `hover`
/// can answer from the last-published set without re-running rules.
#[derive(Debug, Clone)]
struct Finding {
    range: Range,
    severity: DiagnosticSeverity,
    rule_id: String,
    message: String,
    policy_url: Option<String>,
}

/// Per-file findings keyed by absolute path.
type FindingsByPath = HashMap<PathBuf, Vec<Finding>>;

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
    /// Last-published findings per open URI, so `hover` can answer by
    /// position without re-running rules.
    diagnostics: HashMap<Url, Vec<Finding>>,
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
                diagnostics: HashMap::new(),
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
                let to_publish = {
                    let mut state = self.state.lock().unwrap();
                    state.session = Some(session);
                    cache_and_collect(&mut state, &open, &by_path)
                };
                self.publish_all(to_publish).await;
            }
            Ok(Ok(None)) => {
                // No `.alint.yml` — clear any stale diagnostics.
                let to_publish = {
                    let mut state = self.state.lock().unwrap();
                    state.session = None;
                    cache_and_collect(&mut state, &open, &FindingsByPath::new())
                };
                self.publish_all(to_publish).await;
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
                .map(|results| group_findings(&session.root, &results))
        })
        .await;

        match outcome {
            Ok(Ok(by_path)) => {
                let findings = by_path.get(&abs_key).cloned().unwrap_or_default();
                let diagnostics = findings.iter().map(finding_to_diagnostic).collect();
                self.state
                    .lock()
                    .unwrap()
                    .diagnostics
                    .insert(uri.clone(), findings);
                self.client
                    .publish_diagnostics(uri, diagnostics, None)
                    .await;
            }
            Ok(Err(Error::FileNotInIndex { .. })) => {
                // Excluded from linting — clear any diagnostics.
                self.state.lock().unwrap().diagnostics.remove(&uri);
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

    async fn publish_all(&self, items: Vec<(Url, Vec<Diagnostic>)>) {
        for (uri, diagnostics) in items {
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
                hover_provider: Some(HoverProviderCapability::Simple(true)),
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
        {
            let mut state = self.state.lock().unwrap();
            state.open.remove(&uri);
            state.diagnostics.remove(&uri);
        }
        // Clear any diagnostics the editor is still showing.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn hover(&self, params: HoverParams) -> JsonRpcResult<Option<Hover>> {
        let pos = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;

        let findings = {
            let state = self.state.lock().unwrap();
            state.diagnostics.get(&uri).cloned()
        };
        let Some(findings) = findings else {
            return Ok(None);
        };
        let matching: Vec<&Finding> = findings
            .iter()
            .filter(|f| range_contains(f.range, pos))
            .collect();
        if matching.is_empty() {
            return Ok(None);
        }

        let value = matching
            .iter()
            .map(|f| render_finding(f))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: matching.first().map(|f| f.range),
        }))
    }
}

/// Cache the findings for each open document and collect the
/// `(uri, diagnostics)` pairs to publish. Documents absent from
/// `by_path` are cached empty and cleared.
fn cache_and_collect(
    state: &mut State,
    open: &[Url],
    by_path: &FindingsByPath,
) -> Vec<(Url, Vec<Diagnostic>)> {
    let mut out = Vec::with_capacity(open.len());
    for uri in open {
        let findings = uri
            .to_file_path()
            .ok()
            .and_then(|abs| by_path.get(&abs).cloned())
            .unwrap_or_default();
        let diagnostics = findings.iter().map(finding_to_diagnostic).collect();
        state.diagnostics.insert(uri.clone(), findings);
        out.push((uri.clone(), diagnostics));
    }
    out
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
fn build_and_run(root: &Path) -> Result<Option<(Arc<Session>, FindingsByPath)>, String> {
    let Some(session) = build_session(root)? else {
        return Ok(None);
    };
    let report = session
        .engine
        .run(&session.root, &session.index)
        .map_err(|e| format!("running rules: {e}"))?;
    let by_path = group_findings(&session.root, &report.results);
    Ok(Some((Arc::new(session), by_path)))
}

/// Group rule-result violations into per-file findings keyed by
/// absolute path. File- and tree-level findings (no path) are skipped —
/// they have no document to attach to.
fn group_findings(root: &Path, results: &[RuleResult]) -> FindingsByPath {
    let mut by_path = FindingsByPath::new();
    for result in results {
        let Some(severity) = severity_of(result.level) else {
            continue;
        };
        let policy_url = result.policy_url.as_ref().map(ToString::to_string);
        for violation in &result.violations {
            let Some(rel) = &violation.path else {
                continue;
            };
            let abs = root.join(rel.as_ref());
            by_path.entry(abs).or_default().push(Finding {
                range: violation_range(violation),
                severity,
                rule_id: result.rule_id.to_string(),
                message: violation.message.to_string(),
                policy_url: policy_url.clone(),
            });
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

fn severity_label(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::ERROR => "error",
        DiagnosticSeverity::WARNING => "warning",
        _ => "info",
    }
}

/// alint line/column are 1-indexed and optional; LSP positions are
/// 0-indexed. File- and tree-level findings (no line) anchor at the
/// start of the file. The range is one character wide so the editor has
/// something to attach the marker (and hover) to.
fn violation_range(violation: &Violation) -> Range {
    let line = violation
        .line
        .map_or(0, |l| u32::try_from(l.saturating_sub(1)).unwrap_or(0));
    let col = violation
        .column
        .map_or(0, |c| u32::try_from(c.saturating_sub(1)).unwrap_or(0));
    Range::new(
        Position::new(line, col),
        Position::new(line, col.saturating_add(1)),
    )
}

fn finding_to_diagnostic(f: &Finding) -> Diagnostic {
    let code_description = f
        .policy_url
        .as_deref()
        .and_then(|u| Url::parse(u).ok())
        .map(|href| CodeDescription { href });
    Diagnostic {
        range: f.range,
        severity: Some(f.severity),
        code: Some(NumberOrString::String(f.rule_id.clone())),
        code_description,
        source: Some("alint".to_string()),
        message: f.message.clone(),
        ..Diagnostic::default()
    }
}

/// True when `pos` falls within `range` (inclusive of both ends so a
/// hover on the single-character marker registers).
fn range_contains(range: Range, pos: Position) -> bool {
    let after_start = (pos.line, pos.character) >= (range.start.line, range.start.character);
    let before_end = (pos.line, pos.character) <= (range.end.line, range.end.character);
    after_start && before_end
}

/// Markdown hover body for one finding: rule id + severity, the
/// message, and a policy link when the rule declares one.
fn render_finding(f: &Finding) -> String {
    let mut s = format!(
        "**alint** · `{}` ({})\n\n{}",
        f.rule_id,
        severity_label(f.severity),
        f.message
    );
    if let Some(url) = &f.policy_url {
        s.push_str("\n\n[Policy →](");
        s.push_str(url);
        s.push(')');
    }
    s
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

    fn finding(policy_url: Option<&str>) -> Finding {
        Finding {
            range: Range::new(Position::new(3, 6), Position::new(3, 7)),
            severity: DiagnosticSeverity::ERROR,
            rule_id: "my-rule".to_string(),
            message: "boom".to_string(),
            policy_url: policy_url.map(ToString::to_string),
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
    fn violation_range_converts_one_indexed_to_zero_indexed() {
        let r = violation_range(&violation(Some(4), Some(7)));
        assert_eq!(r.start, Position::new(3, 6));
        assert_eq!(r.end, Position::new(3, 7));
    }

    #[test]
    fn violation_range_without_line_anchors_at_file_start() {
        let r = violation_range(&violation(None, None));
        assert_eq!(r.start, Position::new(0, 0));
        assert_eq!(r.end, Position::new(0, 1));
    }

    #[test]
    fn finding_to_diagnostic_carries_rule_and_policy_link() {
        let d = finding_to_diagnostic(&finding(Some("https://example.com/policy")));
        assert_eq!(d.code, Some(NumberOrString::String("my-rule".to_string())));
        assert_eq!(d.source.as_deref(), Some("alint"));
        assert_eq!(d.message, "boom");
        assert_eq!(
            d.code_description.unwrap().href.as_str(),
            "https://example.com/policy"
        );
    }

    #[test]
    fn finding_to_diagnostic_omits_code_description_for_non_url_policy() {
        let d = finding_to_diagnostic(&finding(Some("not a url")));
        assert!(d.code_description.is_none());
    }

    #[test]
    fn range_contains_is_inclusive_of_both_ends() {
        let r = Range::new(Position::new(3, 6), Position::new(3, 7));
        assert!(range_contains(r, Position::new(3, 6)));
        assert!(range_contains(r, Position::new(3, 7)));
        assert!(!range_contains(r, Position::new(3, 8)));
        assert!(!range_contains(r, Position::new(2, 6)));
    }

    #[test]
    fn render_finding_includes_rule_message_and_policy() {
        let md = render_finding(&finding(Some("https://example.com/p")));
        assert!(md.contains("my-rule"), "{md}");
        assert!(md.contains("(error)"), "{md}");
        assert!(md.contains("boom"), "{md}");
        assert!(md.contains("https://example.com/p"), "{md}");
    }

    #[test]
    fn render_finding_omits_policy_link_when_absent() {
        let md = render_finding(&finding(None));
        assert!(!md.contains("Policy"), "{md}");
    }

    #[test]
    fn build_session_returns_none_when_no_config() {
        let dir = tempfile::tempdir().unwrap();
        assert!(build_session(dir.path()).unwrap().is_none());
    }
}
