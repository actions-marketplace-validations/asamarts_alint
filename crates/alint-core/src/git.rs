//! Bounded Git integration.
//!
//! The engine derives active Git dependencies from rule capability metadata,
//! probes repository usability once, and routes probe failure to one typed
//! analysis failure per active owning rule. The helpers in this module retain
//! `Option`-based, best-effort results for operation-specific absence
//! (unborn HEAD, untracked files, unblameable paths) and direct library use.
//! `git_tracked_only` filtering reuses the probe's `git ls-files` set through
//! the rule [`Context`](crate::Context).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{AnalysisTime, AnalysisTimeUnavailable};

/// Environment variables that can redirect a Git command away from the
/// repository selected by `-C`, replace its object/ref view, alter repository
/// discovery, or change literal path interpretation. This includes Git's
/// `rev-parse --local-env-vars` set plus the pathspec-mode controls; clearing it
/// keeps repository identity and path inputs explicit.
const GIT_LOCAL_ENV_VARS: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_COMMON_DIR",
    "GIT_LITERAL_PATHSPECS",
    "GIT_GLOB_PATHSPECS",
    "GIT_NOGLOB_PATHSPECS",
    "GIT_ICASE_PATHSPECS",
];

fn clear_git_repository_env(command: &mut Command) {
    for name in GIT_LOCAL_ENV_VARS {
        // `Command::env_remove` records an owned override even when the parent
        // has no such variable. Avoid building two dozen dead map entries on
        // every Git query; present redirectors are still removed deterministically.
        if std::env::var_os(name).is_some() {
            command.env_remove(name);
        }
    }
}

fn git_command() -> Command {
    let mut command = Command::new("git");
    clear_git_repository_env(&mut command);
    // Repository paths are byte-exact inputs, never user-authored pathspecs;
    // replacement refs are clone-local state, never repository history; log
    // display preferences must not invoke signature verifiers or alter stdout.
    command.args([
        "--literal-pathspecs",
        "--no-replace-objects",
        "-c",
        "log.showSignature=false",
    ]);
    command
}
/// Resolve the current HEAD commit's committer timestamp.
///
/// The command emits only `%ct`; raw Git stderr is deliberately excluded from
/// the stable error returned to rule evaluation and machine formats.
pub fn head_commit_time(root: &Path) -> Result<AnalysisTime, AnalysisTimeUnavailable> {
    let mut command = git_command();
    let output = command
        .arg("-C")
        .arg(root)
        .args(["log", "-1", "--format=%ct", "--end-of-options", "HEAD"])
        .output()
        .map_err(|_| AnalysisTimeUnavailable::GitUnavailable)?;
    if !output.status.success() {
        return Err(AnalysisTimeUnavailable::HeadUnavailable);
    }
    parse_head_commit_time(&output.stdout)
}

fn parse_head_commit_time(stdout: &[u8]) -> Result<AnalysisTime, AnalysisTimeUnavailable> {
    let digits = stdout
        .strip_suffix(b"\n")
        .unwrap_or(stdout)
        .strip_suffix(b"\r")
        .unwrap_or_else(|| stdout.strip_suffix(b"\n").unwrap_or(stdout));
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(AnalysisTimeUnavailable::InvalidHeadTime);
    }
    let text = std::str::from_utf8(digits).map_err(|_| AnalysisTimeUnavailable::InvalidHeadTime)?;
    let seconds = text
        .parse::<u64>()
        .map_err(|_| AnalysisTimeUnavailable::InvalidHeadTime)?;
    AnalysisTime::from_unix_seconds(seconds).ok_or(AnalysisTimeUnavailable::InvalidHeadTime)
}

/// Resolve the repo's tracked-paths set, relative to `root`.
///
/// `root` should be the alint root (the path passed to
/// `alint check`). The implementation runs `git -C <root> ls-files
/// -z` with NO pathspec, so git returns paths relative to `root`
/// whether `root` is the git root or a subdirectory of it (git's
/// `-C <dir>` default). No pathspec means no globbing, so this
/// reader is immune to the pathspec-magic hazard the mutating
/// [`untrack_path`] guards against.
///
/// Returns `None` when:
/// - `git` isn't on PATH
/// - `root` (or any ancestor) isn't inside a git repo
/// - the `git` invocation exits non-zero for any other reason
///
/// All these cases produce an empty `Option`, never panic — the
/// caller is responsible for treating `None` as "no tracked-set
/// available" in whatever way makes sense for the calling rule.
/// Build a `PathBuf` from a NUL-delimited `git -z` path chunk WITHOUT a UTF-8
/// round-trip, so a non-UTF-8 tracked/changed path is PRESERVED (M6) rather than
/// collapsing the whole set to `None` — which made `git_tracked_only`
/// over-permissive (a non-UTF-8 tracked file wasn't recognized as tracked). On
/// Unix the raw bytes ARE the path; elsewhere git emits UTF-8, so a lossy decode
/// keeps a best-effort path instead of dropping the entire set.
fn path_from_git_chunk(chunk: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        PathBuf::from(std::ffi::OsStr::from_bytes(chunk))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(chunk).into_owned())
    }
}

pub fn collect_tracked_paths(root: &Path) -> Option<HashSet<PathBuf>> {
    // `-z` separates entries with NUL so paths with newlines or
    // exotic bytes round-trip correctly. `--full-name` would force
    // repo-root-relative paths, but we want CWD-relative — git's
    // default with `-C <dir>` already gives that.
    let output = git_command()
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut out = HashSet::new();
    for chunk in output.stdout.split(|&b| b == 0) {
        if chunk.is_empty() {
            continue;
        }
        out.insert(path_from_git_chunk(chunk));
    }
    Some(out)
}

/// The result of a [`untrack_path`] attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum UntrackOutcome {
    /// The path was removed from git's index (it stays on disk).
    Untracked,
    /// `root` is not inside a git repo, or `git` is unavailable — nothing done.
    NotAGitRepo,
    /// The path is already absent from the index — nothing done (idempotent).
    NotTracked,
    /// `git rm --cached` ran but exited non-zero (e.g. staged content differs
    /// from HEAD, so a non-forced remove is refused); carries the stderr tail.
    Failed(String),
}

/// Remove `rel_path` from git's index WITHOUT deleting it from the working tree
/// (`git rm --cached -- <path>`), running in `root`. `rel_path` is
/// root-relative (a rule's violation path). Unlike the advisory readers above,
/// this MUTATES the repo (unless `dry_run`), so the caller (the `git_untrack`
/// fixer) reports Applied / Skipped / error from the returned [`UntrackOutcome`].
///
/// `dry_run` adds git's own `--dry-run`: git runs its FULL safety check (incl.
/// refusing a path whose staged content differs from both HEAD and the worktree)
/// but changes nothing, so the fixer's dry-run / `--diff` preview predicts EXACTLY
/// what a real run would do -- `Untracked` = "would untrack", `Failed` = "would
/// fail". The round-4 dry-run-faithfulness invariant then holds by construction
/// (the preview and the real run are the same code path with the same git checks),
/// rather than via a separately-maintained prediction that could drift.
///
/// Safety and idempotence:
/// - the shared `git_command` passes `--literal-pathspecs`, disabling git's
///   pathspec globbing/magic so a path containing `*` / `?` / `[` / `]` (a
///   Next.js `[id].tsx` route, or a file literally named `*`) is matched
///   LITERALLY. `--` only ends OPTION parsing, NOT globbing, so without this a
///   single flagged path would collaterally untrack every sibling git globs it
///   against -- up to the WHOLE index for a `*` -- all hidden behind a "1
///   applied" report. Preferred over a `:(literal)` prefix: it needs no UTF-8
///   round-trip, so a non-UTF-8 path (which this module preserves) stays intact.
///   `collect_tracked_paths` takes no pathspec, so it is unaffected.
/// - the path is passed after `--`, never as an option, so a path beginning with
///   `-` can never be read as a git flag (option-injection guard);
/// - outside a repo it returns `NotAGitRepo` and an already-untracked path returns
///   `NotTracked` (both no-ops), so a second run is a clean skip, not a `git` error;
/// - it does NOT pass `-f`: a path with staged content differing from HEAD is left
///   untouched and reported as `Failed`, never force-removed.
pub fn untrack_path(root: &Path, rel_path: &Path, dry_run: bool) -> UntrackOutcome {
    // Pre-classify by reusing the advisory reader: outside a repo -> NotAGitRepo;
    // not in the index -> NotTracked (the idempotent no-op). This also means the
    // real `git rm` only runs on a path we have already confirmed is tracked.
    match collect_tracked_paths(root) {
        None => return UntrackOutcome::NotAGitRepo,
        Some(tracked) if !tracked.contains(rel_path) => return UntrackOutcome::NotTracked,
        Some(_) => {}
    }
    let mut cmd = git_command();
    cmd.arg("-C").arg(root).args(["rm", "--cached", "--quiet"]);
    if dry_run {
        cmd.arg("--dry-run");
    }
    cmd.arg("--").arg(rel_path);
    match cmd.output() {
        Ok(o) if o.status.success() => UntrackOutcome::Untracked,
        Ok(o) => UntrackOutcome::Failed(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => UntrackOutcome::Failed(e.to_string()),
    }
}

/// Resolve the set of paths that have changed in the working tree
/// (and optionally relative to a base ref), expressed as paths
/// relative to `root`.
///
/// `base` selects the diff:
/// - `Some("main")` — `git diff --name-only --relative main...HEAD`
///   (three-dot — diff against the merge-base of `main` and
///   `HEAD`). Right shape for PR-check use cases.
/// - `None` — the union of `git diff --cached --name-only --relative`
///   and `git ls-files --modified --others --exclude-standard` from
///   `root`. This includes staged, unstaged, and untracked changes,
///   which is the right shape for pre-commit / local-dev use cases.
///   Deleted files are also returned (they're in the diff but not on
///   disk, so the engine's intersect-with-walked-index step filters
///   them out naturally).
///
/// Returns `None` on the same conditions as
/// [`collect_tracked_paths`]: `git` not on PATH, `root` outside
/// a repo, or the invocation exits non-zero. Callers should
/// treat `None` as "no changed-set available" and fall back to
/// a full check (or surface a hard error, depending on intent —
/// `alint check --changed` errors out rather than fall back, so
/// the user's "diff-only" intent isn't silently broken).
pub fn collect_changed_paths(root: &Path, base: Option<&str>) -> Option<HashSet<PathBuf>> {
    // Two distinct invocations: ref-based diff vs. working-tree
    // status. Both emit NUL-separated output so paths with
    // newlines / non-UTF-8 bytes round-trip.
    let outputs = if let Some(base) = base {
        // Defense-in-depth, matching `diff_name_only`: reject a `base`
        // starting with `-` explicitly (treat as "no changed-set"), in
        // addition to the `--end-of-options` guard below.
        if base.starts_with('-') {
            return None;
        }
        let output = git_command()
            .arg("-C")
            .arg(root)
            .args(["diff", "--name-only", "--relative", "-z"])
            // `--end-of-options` so a `base`/`since` starting with `-`
            // can't be parsed as a git OPTION (e.g. `--output=…`, which
            // would write/truncate an arbitrary file).
            .arg("--end-of-options")
            .arg(format!("{base}...HEAD"))
            .output()
            .ok()?;
        vec![output]
    } else {
        // `ls-files --modified` compares the worktree with the index, so a
        // staged edit disappears from it. Union the index-vs-HEAD diff with
        // the working-tree/untracked set; otherwise a pre-commit hook can
        // silently check zero files after the framework stashes unstaged
        // changes. `--cached` also works for an unborn HEAD.
        //
        // The two reads are independent, so the staged diff runs in the
        // background while `ls-files` runs: one process's latency instead of
        // two, on every `--changed` run. A staged list too big for the pipe
        // buffer (64 KiB on Linux, 4 KiB on Windows) makes the child wait for
        // `ls-files` to finish, which is no slower than running them in turn.
        // `wait_with_output` then drains both pipes, so the child can't wedge,
        // and it is always reaped, even when `ls-files` fails to spawn.
        let staged = git_command()
            .arg("-C")
            .arg(root)
            .args(["diff", "--cached", "--name-only", "--relative", "-z"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?;
        let working = git_command()
            .arg("-C")
            .arg(root)
            .args([
                "ls-files",
                "--modified",
                "--others",
                "--exclude-standard",
                "-z",
            ])
            .output();
        let staged = staged.wait_with_output().ok()?;
        vec![staged, working.ok()?]
    };
    if outputs.iter().any(|output| !output.status.success()) {
        return None;
    }
    let mut out = HashSet::new();
    for output in outputs {
        for chunk in output.stdout.split(|&b| b == 0) {
            if chunk.is_empty() {
                continue;
            }
            out.insert(path_from_git_chunk(chunk));
        }
    }
    Some(out)
}

/// Like [`collect_changed_paths`] with a `base` ref, but distinguishes
/// repository unavailability from a ref that does not resolve — the contract
/// `scope_filter.changed_since:` needs. Returns the set of paths changed in
/// `<since>...HEAD` (three-dot, merge-base diff — matching
/// `alint check --changed`), relative to `root`.
///
/// - `Ok(Some(set))` — resolved.
/// - `Ok(None)`       — the repository or Git executable is unavailable.
/// - `Err(BadRange)`  — in a repo, but `<since>` didn't resolve (e.g.
///   a shallow-clone gotcha). The caller surfaces a fetch-depth hint.
pub fn collect_changed_paths_checked(
    root: &Path,
    since: &str,
) -> Result<Option<HashSet<PathBuf>>, CommitRangeError> {
    diff_name_only(root, since, None)
}

/// Like [`collect_changed_paths_checked`] but restricted to a git
/// `--diff-filter` (e.g. `"A"` for added paths, `"M"` for modified).
/// Same posture: `Ok(None)` outside a repo / `git` missing,
/// `Err(BadRange)` on an unresolvable `since`. Used by
/// `changeset_requires_path` to find files *added* in `<since>...HEAD`.
pub fn collect_changed_paths_filtered(
    root: &Path,
    since: &str,
    diff_filter: &str,
) -> Result<Option<HashSet<PathBuf>>, CommitRangeError> {
    diff_name_only(root, since, Some(diff_filter))
}

/// Shared `git diff --name-only --relative -z <since>...HEAD`
/// (optionally `--diff-filter=<…>`), with the git-repo probe and NUL
/// parsing both [`collect_changed_paths_checked`] and
/// [`collect_changed_paths_filtered`] need.
fn diff_name_only(
    root: &Path,
    since: &str,
    diff_filter: Option<&str>,
) -> Result<Option<HashSet<PathBuf>>, CommitRangeError> {
    // Probe: are we in a git repo at all? If not, silent None —
    // matching the advisory posture of the rest of this module.
    let Ok(probe) = git_command()
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--git-dir"])
        .output()
    else {
        return Ok(None);
    };
    if !probe.status.success() {
        return Ok(None);
    }
    let mut cmd = git_command();
    cmd.arg("-C")
        .arg(root)
        .args(["diff", "--name-only", "--relative", "-z"]);
    if since.starts_with('-') {
        return Err(CommitRangeError::BadRange {
            stderr: format!("`since` must not start with '-' (got {since:?})"),
        });
    }
    if let Some(filter) = diff_filter {
        cmd.arg(format!("--diff-filter={filter}"));
    }
    // `--end-of-options`: a config-controlled `since` starting with `-`
    // (e.g. `--output=…`) must never be parsed as a git OPTION — that
    // would write/truncate an arbitrary out-of-tree file. Force it into
    // the revision-range slot.
    cmd.arg("--end-of-options");
    cmd.arg(format!("{since}...HEAD"));
    let Ok(output) = cmd.output() else {
        return Ok(None);
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(CommitRangeError::BadRange { stderr });
    }
    let mut out = HashSet::new();
    for chunk in output.stdout.split(|&b| b == 0) {
        if chunk.is_empty() {
            continue;
        }
        // Preserve a non-UTF-8 changed path instead of collapsing the whole set
        // to `None` (which the caller maps to an empty changed-set, silently
        // no-opping every `changed_since`-scoped rule — a fail-open lint bypass).
        // Same decode the top-level `--changed` collector uses (M6).
        out.insert(path_from_git_chunk(chunk));
    }
    Ok(Some(out))
}

/// HEAD's commit message, as a single string with newlines
/// preserved between subject and body. The subject is the first
/// line; everything after the first blank line is the body.
///
/// Returns `None` when:
/// - `git` isn't on PATH
/// - `root` (or any ancestor) isn't inside a git repo
/// - the repo has no commits yet (HEAD is unborn)
/// - the `git log` invocation otherwise exits non-zero
///
/// Used by the `git_commit_message` rule kind. The engine handles repository
/// unavailability before dispatch; `None` remains valid for an unborn HEAD or
/// an operation-specific failure.
pub fn head_commit_message(root: &Path) -> Option<String> {
    let output = git_command()
        .arg("-C")
        .arg(root)
        .args(["log", "-1", "--format=%B"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // Lossily decode a non-UTF-8 HEAD message instead of returning `None` (which
    // silently no-ops `git_commit_message` — letting a non-UTF-8 commit message
    // bypass conventional-subject / forbidden-pattern / length checks; the same
    // fail-open the range-mode `parse_commit_log` was already hardened against, M6).
    let raw = String::from_utf8_lossy(&output.stdout);
    // `git log --format=%B` appends a trailing newline that's not
    // part of the message body — trim once at the end so length
    // checks against the subject and body don't trip on it.
    Some(raw.trim_end_matches('\n').to_string())
}

/// HEAD as a full [`CommitRecord`] — abbreviated SHA, author name +
/// email, and the message. Used by the commit-validation family's
/// HEAD-only mode (`since:` unset), where rules like
/// `git_commit_author_allowlist` need the author and the SHA in
/// addition to the message.
///
/// Returns `None` on the same conditions as [`head_commit_message`]
/// (no `git`, not a repo, unborn HEAD). The engine gates the first two before
/// rule dispatch; an unborn HEAD has no commit policy input.
/// Uses the same NUL-separated `--format` encoding as
/// [`commit_messages_in_range`] so a single commit round-trips
/// through the shared commit-log parser.
pub fn head_commit_record(root: &Path) -> Option<CommitRecord> {
    let output = git_command()
        .arg("-C")
        .arg(root)
        .args([
            "log",
            "-1",
            "--abbrev=7",
            "--format=%h%x00%an%x00%ae%x00%B%x1e",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_commit_log(&output.stdout).into_iter().next()
}

/// One commit in a `<since>..HEAD` range, as returned by
/// [`commit_messages_in_range`]. `sha` uses a requested seven-hex abbreviation;
/// Git extends it only when that prefix is ambiguous in the repository. `message`
/// is the full commit message (subject + body, separated by a blank line) with the
/// trailing newline that `git log --format=%B` appends already trimmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitRecord {
    pub sha: String,
    pub message: String,
    /// Author name (`git log %an`). Empty when synthesised for a
    /// HEAD-only check that didn't capture authorship.
    pub author_name: String,
    /// Author email (`git log %ae`).
    pub author_email: String,
}

/// Errors that distinguish an invalid range from an unavailable repository.
/// The engine gates repository availability from capability metadata; the rule
/// layer uses `BadRange` to preserve the specific shallow-clone migration hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitRangeError {
    /// The `<since>` ref doesn't resolve, or the range itself is
    /// rejected by git (e.g. `bad revision`). Carries the stderr
    /// `git` produced so the caller can include it in its error.
    /// Typically caused by:
    /// - typo in the ref name
    /// - shallow clone that doesn't have the ref in local objects
    ///   (the most common CI gotcha; `actions/checkout` defaults to
    ///   `fetch-depth: 1`)
    BadRange { stderr: String },
}

/// Enumerate commits reachable from `HEAD` but not from `since`,
/// i.e. the standard `<since>..HEAD` range, oldest first.
///
/// `since` is anything `git rev-parse` accepts: a 40-char SHA, an
/// abbreviated SHA, a branch (`origin/main`), a tag (`v1.2.3`), or
/// a relative ref (`HEAD~5`).
///
/// `include_merges` controls whether merge commits in the range are
/// returned. Defaults to `false` at the call site for PR workflows
/// (where the merge commit at HEAD is the synthetic
/// `actions/checkout`-produced one) but the caller decides.
///
/// Returns:
/// - `Ok(Some(records))` on success. The vec may be empty if the
///   range itself is empty (`since` == HEAD on a force-push PR, or
///   no non-merge commits in the range).
/// - `Ok(None)` if `git` isn't on PATH or `root` isn't inside a Git repository.
///   The engine normally gates this before rule dispatch; the explicit shape
///   keeps this low-level helper total for direct callers.
/// - `Err(CommitRangeError::BadRange)` if `git` is present and the
///   repo is valid but the range couldn't be resolved. Rules
///   surface this as a hard error so the user sees the
///   misconfiguration instead of a confused empty range.
///
/// Implementation note: uses `--format=%h%x00%B%x1e` so the SHA and
/// the message are NUL-separated (NUL never appears in either) and
/// commits are RS-separated (RS = U+001E, "record separator", which
/// also doesn't appear in well-formed commit text). The compound
/// encoding is robust against commit messages containing arbitrary
/// text — including em dashes, blank lines, and Unicode shenanigans
/// — without resorting to fragile line-counting.
pub fn commit_messages_in_range(
    root: &Path,
    since: &str,
    include_merges: bool,
) -> Result<Option<Vec<CommitRecord>>, CommitRangeError> {
    // First check `git rev-parse` (no range syntax) confirms repository
    // availability. If it fails, return the distinct `Ok(None)` shape that
    // engine capability routing promotes before normal rule dispatch.
    let probe = git_command()
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--git-dir"])
        .output();
    let Ok(probe) = probe else {
        return Ok(None);
    };
    if !probe.status.success() {
        return Ok(None);
    }

    // Now invoke `git log <since>..HEAD`. If THIS fails, it's a bad
    // ref / shallow-clone case, not a "no git" case — bubble the
    // BadRange error.
    if since.starts_with('-') {
        return Err(CommitRangeError::BadRange {
            stderr: format!("`since` must not start with '-' (got {since:?})"),
        });
    }
    let range = format!("{since}..HEAD");
    let mut cmd = git_command();
    cmd.arg("-C").arg(root).args([
        "log",
        "--reverse",
        "--abbrev=7",
        "--format=%h%x00%an%x00%ae%x00%B%x1e",
    ]);
    if !include_merges {
        cmd.arg("--no-merges");
    }
    // `--end-of-options`: a config `since` starting with `-` (e.g.
    // `--output=…`) must never be parsed as a git OPTION (which would
    // write/truncate an arbitrary file); force it to the range slot.
    cmd.arg("--end-of-options");
    cmd.arg(&range);

    let Ok(output) = cmd.output() else {
        return Ok(None);
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(CommitRangeError::BadRange { stderr });
    }

    Ok(Some(parse_commit_log(&output.stdout)))
}

/// Parse the NUL+RS-separated `git log` output produced by
/// [`commit_messages_in_range`]'s `--format` string. Empty trailing
/// records (from the final RS) are skipped. Messages have their
/// trailing newline trimmed (`git log` always appends one).
fn parse_commit_log(stdout: &[u8]) -> Vec<CommitRecord> {
    let mut out = Vec::new();
    // Records are RS-separated (0x1e). The last record ends with
    // RS too, so the final split chunk is empty.
    for record in stdout.split(|&b| b == 0x1e) {
        if record.is_empty() {
            continue;
        }
        // Each record is sha + NUL + author-name + NUL +
        // author-email + NUL + message. Trim the leading newline
        // that git inserts between records.
        let record = record.strip_prefix(b"\n").unwrap_or(record);
        let mut parts = record.splitn(4, |&b| b == 0);
        let (Some(sha_bytes), Some(name_bytes), Some(email_bytes), Some(msg_bytes)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        // Lossily decode rather than dropping the whole commit on any
        // non-UTF-8 field: silently skipping a commit lets a contributor
        // bypass commit linting (conventional-subject / author-allowlist /
        // forbidden-pattern) just by using a non-UTF-8 author name or message.
        // With a lossy decode the commit is still linted; the sha is always
        // hex so it is unaffected.
        let sha = String::from_utf8_lossy(sha_bytes);
        let name = String::from_utf8_lossy(name_bytes);
        let email = String::from_utf8_lossy(email_bytes);
        let msg = String::from_utf8_lossy(msg_bytes);
        // `--format=%B` ends every body with a trailing newline.
        let message = msg.trim_end_matches('\n').to_string();
        out.push(CommitRecord {
            sha: sha.to_string(),
            message,
            author_name: name.to_string(),
            author_email: email.to_string(),
        });
    }
    out
}

/// Verify a commit's signature via `git verify-commit <sha>`.
///
/// Returns:
/// - `Some(true)`  — `verify-commit` exited 0 (a good signature that
///   verified against the local keyring).
/// - `Some(false)` — it exited non-zero: the commit is unsigned, or
///   the signature didn't verify (e.g. signed with a key not in the
///   local keyring).
/// - `None`        — `git` isn't on PATH (the shell-out itself
///   failed). Callers iterating commits from a valid repo never see
///   this; it's the advisory-posture escape hatch.
///
/// This reflects git's own verdict and deliberately does NOT
/// distinguish "unsigned" from "signed with an untrusted key" —
/// trust is the user's GPG config / `.git/allowed_signers`, not this
/// rule's job.
pub fn verify_commit(root: &Path, sha: &str) -> Option<bool> {
    let output = git_command()
        .arg("-C")
        .arg(root)
        .args(["verify-commit", sha])
        .output()
        .ok()?;
    Some(output.status.success())
}

/// One line of `git blame --line-porcelain` output: the
/// 1-indexed final line number in the working-tree file, the
/// authoring time of the commit that last touched the line
/// (per `.git-blame-ignore-revs`, when present), and the line
/// content with its trailing newline stripped.
///
/// Used by the `git_blame_age` rule kind to decide whether a
/// pattern-matching line is older than a configured threshold.
/// The line content is preserved as-is so the rule can apply
/// its own regex match.
#[derive(Debug, Clone)]
pub struct BlameLine {
    pub line_number: usize,
    pub author_time: SystemTime,
    pub content: String,
}

const DEFAULT_BLAME_IGNORE_REVS_FILE: &str = ".git-blame-ignore-revs";

/// Run `git blame --line-porcelain` for `rel_path` (relative to
/// `root`) and return one [`BlameLine`] per source line.
///
/// `--line-porcelain` repeats the full per-commit metadata block
/// for every line so we don't have to track the most-recent
/// commit across runs — every line carries its own
/// `author-time`. When the repository contains a regular
/// `.git-blame-ignore-revs` file, alint passes it explicitly rather than
/// depending on clone-local Git configuration.
///
/// Returns `None` when:
/// - `git` isn't on PATH
/// - `root` (or any ancestor) isn't inside a git repo
/// - `rel_path` isn't tracked (untracked files have no blame)
/// - the `git blame` invocation otherwise exits non-zero
///
/// Same advisory posture as the rest of the git module: a
/// non-blameable file silently no-ops the rule rather than
/// raising a hard error.
pub fn blame_lines(root: &Path, rel_path: &Path) -> Option<Vec<BlameLine>> {
    blame_lines_with_ignore_revs(root, rel_path, default_ignore_revs_is_tracked_regular(root))
}

fn blame_lines_with_ignore_revs(
    root: &Path,
    rel_path: &Path,
    use_default_ignore_revs: bool,
) -> Option<Vec<BlameLine>> {
    let mut command = git_command();
    command
        // Clone-local/global ignore lists are ambient machine state. A command-line
        // empty file name clears their revisions before the repository convention
        // is added below. A malformed configured file can still make Git fail while
        // loading its config; W2 will promote that existing advisory failure path.
        // The documented `diff.algorithm` setting changes blame attribution, so pin
        // Myers rather than inheriting user/global configuration.
        .args(["-c", "diff.algorithm=myers"])
        .arg("-C")
        .arg(root)
        .args(["blame", "--ignore-revs-file", "", "--line-porcelain"]);
    if use_default_ignore_revs {
        // Apply only the repository-tracked regular-file convention, independent
        // of each clone's `blame.ignoreRevsFile`.
        command.args(["--ignore-revs-file", DEFAULT_BLAME_IGNORE_REVS_FILE]);
    }
    let output = command.arg("--").arg(rel_path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    // Lossily decode: `--line-porcelain` embeds source-line content AND author
    // names, so one non-UTF-8 byte anywhere in the file must NOT collapse the
    // whole blame to `None` — that silently no-ops `git_blame_age` for the file
    // (every stale line evades the age check), the M6 fail-open the sibling git
    // paths were already hardened against. The porcelain framing (SHA/header
    // keywords, tab-prefixed content lines) is ASCII, so lossy decoding leaves
    // the structure parseable.
    let text = String::from_utf8_lossy(&output.stdout);
    Some(parse_porcelain(&text))
}

/// Parse the `--line-porcelain` output of `git blame`. Pure
/// string-handling so it's exercised by unit tests without
/// shelling out to git.
///
/// Each line of the source file produces one porcelain block:
///
/// ```text
/// <sha> <orig_line> <final_line> <num_lines>
/// author <name>
/// author-mail <<email>>
/// author-time <unix_ts>
/// author-tz <tz>
/// committer …
/// summary …
/// previous … (optional)
/// filename …
/// \t<source line>
/// ```
///
/// We track `author-time` and the trailing tab-prefixed source
/// line; everything else passes through. Lines that don't fit
/// the shape are skipped silently — git blame output is well-
/// defined, but we don't want a parse-error to torpedo a check
/// run on a corrupted repo.
fn parse_porcelain(text: &str) -> Vec<BlameLine> {
    let mut out = Vec::new();
    let mut final_line: Option<usize> = None;
    let mut author_time: Option<SystemTime> = None;
    let mut uncommitted = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('\t') {
            // Source line. Emit a BlameLine when we have both a
            // final-line number and an author-time; otherwise
            // skip (malformed block).
            if let (Some(n), Some(t)) = (final_line.take(), author_time.take()) {
                out.push(BlameLine {
                    line_number: n,
                    author_time: t,
                    content: rest.to_string(),
                });
            }
            continue;
        }
        // Header lines start with a full SHA-1 (40 hex digits) or SHA-256
        // object ID (64 hex digits); subsequent lines are `key value` pairs.
        let mut parts = line.splitn(2, ' ');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");
        match key {
            "author-time" if !uncommitted => {
                if let Ok(secs) = value.parse::<u64>() {
                    // `checked_add`: a crafted commit can carry a 19-digit
                    // author-time that parses as u64 but overflows SystemTime
                    // (which panics on `+`). On overflow, leave the time unset
                    // — matching the "drop the malformed block" posture
                    // elsewhere in the parser — instead of aborting the run.
                    author_time = UNIX_EPOCH.checked_add(Duration::from_secs(secs));
                }
            }
            // Full object-id header + 3 numbers. Accept both repository object
            // formats; hard-coding SHA-1 silently drops every line in a SHA-256
            // repository.
            sha if matches!(sha.len(), 40 | 64) && sha.chars().all(|c| c.is_ascii_hexdigit()) => {
                // Git assigns working-tree-only lines the all-zero pseudo SHA and
                // may synthesize their author time from the wall clock. They are
                // not ageable committed history, so omit them deterministically.
                uncommitted = sha.bytes().all(|b| b == b'0');
                author_time = None;
                // The header line is `<sha> <orig> <final> [<num_lines>]`.
                let mut cols = value.split(' ');
                let _orig = cols.next();
                final_line = cols.next().and_then(|s| s.parse::<usize>().ok());
            }
            _ => {}
        }
    }
    out
}

/// Per-run cache of `git blame` output, shared across rules so
/// multiple `git_blame_age` rules over overlapping `paths:`
/// re-use the parsed result instead of re-shelling-out.
///
/// Constructed once per [`Engine::run`](crate::Engine::run) when
/// at least one rule reports `wants_git_blame()`. Lookups lock
/// once per (path, miss) — `git blame` itself dwarfs any lock
/// contention (process spawn + read of full file history). The
/// cache also memoises *failures* (file untracked, blame exited
/// non-zero) so a rule iterating thousands of out-of-scope files
/// doesn't re-probe each one repeatedly.
#[derive(Debug)]
pub struct BlameCache {
    root: PathBuf,
    use_default_ignore_revs: bool,
    inner: Mutex<HashMap<PathBuf, CacheEntry>>,
}

#[derive(Debug, Clone)]
enum CacheEntry {
    Ok(Arc<Vec<BlameLine>>),
    Failed,
}

fn default_ignore_revs_is_tracked_regular(root: &Path) -> bool {
    let ignore_revs = root.join(DEFAULT_BLAME_IGNORE_REVS_FILE);
    if !ignore_revs
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_file())
    {
        return false;
    }
    git_command()
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "--error-unmatch",
            "--",
            DEFAULT_BLAME_IGNORE_REVS_FILE,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

impl BlameCache {
    pub fn new(root: PathBuf) -> Self {
        let use_default_ignore_revs = default_ignore_revs_is_tracked_regular(&root);
        Self::with_default_ignore_revs(root, use_default_ignore_revs)
    }

    pub(crate) fn new_with_tracked_paths(root: PathBuf, tracked: &HashSet<PathBuf>) -> Self {
        let ignore_revs = root.join(DEFAULT_BLAME_IGNORE_REVS_FILE);
        let use_default_ignore_revs = tracked.contains(Path::new(DEFAULT_BLAME_IGNORE_REVS_FILE))
            && ignore_revs
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.file_type().is_file());
        Self::with_default_ignore_revs(root, use_default_ignore_revs)
    }

    fn with_default_ignore_revs(root: PathBuf, use_default_ignore_revs: bool) -> Self {
        Self {
            root,
            use_default_ignore_revs,
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Return the blame for `rel_path`, computing once and
    /// caching forever (within this run). `None` means this individual path
    /// could not be blamed, so the caller skips it; repository-wide
    /// unavailability is handled before dispatch.
    pub fn get(&self, rel_path: &Path) -> Option<Arc<Vec<BlameLine>>> {
        // Hold the lock through the shell-out: the `git blame`
        // process spawn is the dominant cost, so contention from
        // other threads waiting is negligible relative to letting
        // them duplicate the work. If/when we have evidence of
        // hot-loop contention here, switch to a "compute outside
        // the lock with a Pending sentinel" pattern.
        let mut guard = self.inner.lock().expect("blame cache lock poisoned");
        if let Some(entry) = guard.get(rel_path) {
            return match entry {
                CacheEntry::Ok(arc) => Some(Arc::clone(arc)),
                CacheEntry::Failed => None,
            };
        }
        let computed =
            blame_lines_with_ignore_revs(&self.root, rel_path, self.use_default_ignore_revs);
        if let Some(v) = computed {
            let arc = Arc::new(v);
            guard.insert(rel_path.to_path_buf(), CacheEntry::Ok(Arc::clone(&arc)));
            Some(arc)
        } else {
            guard.insert(rel_path.to_path_buf(), CacheEntry::Failed);
            None
        }
    }
}

/// Test whether `dir_rel` (a relative-to-root directory path)
/// "exists in git" — defined as: at least one tracked file lives
/// underneath it. Used by `dir_exists` / `dir_absent` when
/// `git_tracked_only: true` is set.
///
/// Linear scan over the tracked set. Acceptable for repos with
/// O(thousands) of files; revisit with a prefix-tree if a future
/// dir-rule benchmark shows it dominate.
///
/// Generic over the hasher so callers can use any
/// `HashSet` flavour without an extra collection allocation.
pub fn dir_has_tracked_files<S>(
    dir_rel: &Path,
    tracked: &std::collections::HashSet<PathBuf, S>,
) -> bool
where
    S: std::hash::BuildHasher,
{
    tracked.iter().any(|p| p.starts_with(dir_rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_returns_none_outside_git() {
        let tmp = tempfile::tempdir().unwrap();
        // `git ls-files` in a non-git directory exits non-zero;
        // we report None. Tests that need a populated set
        // construct a real repo via fixtures elsewhere.
        let result = collect_tracked_paths(tmp.path());
        assert!(result.is_none());
    }

    #[test]
    fn collect_changed_returns_none_outside_git() {
        let tmp = tempfile::tempdir().unwrap();
        // Both diff modes shell out to git; both should report
        // None outside a repo so callers can decide between
        // hard-error (CLI's `--changed`) and silent fallback.
        assert!(collect_changed_paths(tmp.path(), None).is_none());
        assert!(collect_changed_paths(tmp.path(), Some("main")).is_none());
    }

    #[test]
    fn collect_changed_without_base_unions_staged_unstaged_and_untracked() {
        let repo = make_repo_with_commits(&["init"]);
        let root = repo.path();
        std::fs::write(root.join("staged.txt"), "old\n").unwrap();
        std::fs::write(root.join("unstaged.txt"), "old\n").unwrap();
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["add", "staged.txt", "unstaged.txt"]);
        run(&["commit", "-m", "add fixtures"]);

        std::fs::write(root.join("staged.txt"), "staged edit\n").unwrap();
        run(&["add", "staged.txt"]);
        std::fs::write(root.join("unstaged.txt"), "unstaged edit\n").unwrap();
        std::fs::write(root.join("untracked.txt"), "new\n").unwrap();

        let changed = collect_changed_paths(root, None).expect("changed paths");
        assert_eq!(
            changed,
            HashSet::from([
                PathBuf::from("staged.txt"),
                PathBuf::from("unstaged.txt"),
                PathBuf::from("untracked.txt"),
            ])
        );
    }

    #[test]
    fn collect_changed_without_base_drains_a_staged_list_larger_than_a_pipe() {
        // The staged diff runs in the background while `ls-files` runs. Its
        // output here (1,500 paths of ~86 bytes, ~126 KiB) overflows a 64 KiB
        // pipe buffer, so the child blocks on write until it's drained: the
        // result must still be complete, with no hang. (Waiting on the child
        // before reading its stdout would deadlock here.)
        let repo = make_repo_with_commits(&["init"]);
        let root = repo.path();
        let dir = root.join("a-directory-name-long-enough-to-grow-each-path");
        std::fs::create_dir(&dir).unwrap();
        let mut expected = HashSet::new();
        for i in 0..1_500 {
            let name = format!("staged-file-with-a-padded-name-{i:05}.txt");
            std::fs::write(dir.join(&name), "x\n").unwrap();
            expected.insert(Path::new("a-directory-name-long-enough-to-grow-each-path").join(name));
        }
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["add", "-A"])
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
        expected.insert(PathBuf::from("untracked.txt"));

        let changed = collect_changed_paths(root, None).expect("changed paths");
        assert_eq!(changed, expected);
    }

    #[test]
    #[cfg(unix)]
    fn path_from_git_chunk_preserves_non_utf8() {
        use std::os::unix::ffi::OsStrExt as _;
        // M6: a non-UTF-8 tracked/changed path must survive as its exact bytes,
        // not collapse the whole set to None (which made `git_tracked_only`
        // over-permissive by not recognizing the file as tracked).
        let raw: &[u8] = b"dir/\xff\xfe_file";
        let p = path_from_git_chunk(raw);
        assert_eq!(p.as_os_str().as_bytes(), raw);
    }

    #[test]
    fn head_commit_time_uses_fixed_committer_timestamp_and_detached_head() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert_eq!(
            head_commit_time(root),
            Err(AnalysisTimeUnavailable::HeadUnavailable)
        );
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            head_commit_time(root),
            Err(AnalysisTimeUnavailable::HeadUnavailable)
        );
        std::fs::write(root.join("tracked"), "x\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "tracked"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.com",
                    "commit",
                    "-qm",
                    "fixed",
                ])
                .env("GIT_AUTHOR_DATE", "2024-01-02T03:04:05Z")
                .env("GIT_COMMITTER_DATE", "2024-01-02T03:04:05Z")
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        let expected =
            "2024-01-02".parse::<AnalysisTime>().unwrap().unix_seconds() + 3 * 3_600 + 4 * 60 + 5;
        assert_eq!(head_commit_time(root).unwrap().unix_seconds(), expected);
        assert!(
            Command::new("git")
                .args(["checkout", "-q", "--detach", "HEAD"])
                .current_dir(root)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(head_commit_time(root).unwrap().unix_seconds(), expected);
    }

    #[test]
    fn git_queries_ignore_foreign_repository_environment() {
        const ROOT_ENV: &str = "ALINT_TEST_HEAD_TIME_ROOT";
        const EXPECTED_ENV: &str = "ALINT_TEST_HEAD_TIME_EXPECTED";
        if let Some(root) = std::env::var_os(ROOT_ENV) {
            let expected = std::env::var(EXPECTED_ENV).unwrap().parse::<u64>().unwrap();
            assert_eq!(
                head_commit_time(Path::new(&root)).unwrap().unix_seconds(),
                expected
            );
            let tracked = collect_tracked_paths(Path::new(&root)).unwrap();
            assert!(tracked.contains(Path::new("tracked.txt")));
            let blame = blame_lines(Path::new(&root), Path::new("tracked.txt")).unwrap();
            assert_eq!(blame.len(), 1);
            assert_eq!(blame[0].content, "2024-01-02T03:04:05Z");
            return;
        }

        let make_repo = |date: &str| {
            let repo = tempfile::tempdir().unwrap();
            for args in [
                &["init", "-q", "-b", "main"][..],
                &["config", "user.email", "test@example.com"][..],
                &["config", "user.name", "Test"][..],
                &["config", "commit.gpgsign", "false"][..],
            ] {
                assert!(
                    Command::new("git")
                        .arg("-C")
                        .arg(repo.path())
                        .args(args)
                        .status()
                        .unwrap()
                        .success()
                );
            }
            std::fs::write(repo.path().join("tracked.txt"), format!("{date}\n")).unwrap();
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(repo.path())
                    .args(["add", "tracked.txt"])
                    .status()
                    .unwrap()
                    .success()
            );
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(repo.path())
                    .args(["commit", "-qm", "fixed"])
                    .env("GIT_AUTHOR_DATE", date)
                    .env("GIT_COMMITTER_DATE", date)
                    .status()
                    .unwrap()
                    .success()
            );
            repo
        };
        let target = make_repo("2024-01-02T03:04:05Z");
        let foreign = make_repo("2030-06-07T08:09:10Z");
        let expected =
            "2024-01-02".parse::<AnalysisTime>().unwrap().unix_seconds() + 3 * 3_600 + 4 * 60 + 5;

        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "git::tests::git_queries_ignore_foreign_repository_environment",
                "--nocapture",
            ])
            .env(ROOT_ENV, target.path())
            .env(EXPECTED_ENV, expected.to_string())
            .env("GIT_DIR", foreign.path().join(".git"))
            .env("GIT_WORK_TREE", foreign.path())
            .env("GIT_INDEX_FILE", foreign.path().join(".git/index"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "isolated child failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[cfg(unix)]
    fn git_log_queries_disable_configured_signature_verification() {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        run(&["init", "-q"]);
        std::fs::write(root.join("tracked"), "x\n").unwrap();
        run(&["add", "tracked"]);
        run(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "base",
        ]);

        // Install a commit carrying a signature header without depending on a
        // real key. With log.showSignature enabled, Git would invoke the
        // configured verifier for otherwise read-only history queries.
        let tree = String::from_utf8(run(&["rev-parse", "HEAD^{tree}"]))
            .unwrap()
            .trim()
            .to_string();
        let commit = format!(
            "tree {tree}\n\
             author Test <test@example.com> 1704164645 +0000\n\
             committer Test <test@example.com> 1704164645 +0000\n\
             gpgsig -----BEGIN PGP SIGNATURE-----\n\
             \x20\n\
             \x20ZmFrZQ==\n\
             \x20-----END PGP SIGNATURE-----\n\
             \n\
             signed\n"
        );
        let mut hash_object = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["hash-object", "-t", "commit", "-w", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        hash_object
            .stdin
            .take()
            .unwrap()
            .write_all(commit.as_bytes())
            .unwrap();
        let hash_output = hash_object.wait_with_output().unwrap();
        assert!(hash_output.status.success());
        let hash = String::from_utf8(hash_output.stdout).unwrap();
        run(&["update-ref", "HEAD", hash.trim()]);

        let verifier = root.join("gpg-probe.sh");
        std::fs::write(
            &verifier,
            "#!/bin/sh\ntouch \"$(dirname \"$0\")/gpg-invoked\"\nexit 1\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&verifier).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&verifier, permissions).unwrap();
        run(&["config", "log.showSignature", "true"]);
        run(&[
            "config",
            "gpg.program",
            verifier.to_str().expect("temporary path is UTF-8"),
        ]);

        assert_eq!(
            head_commit_time(root).unwrap().unix_seconds(),
            1_704_164_645
        );
        assert_eq!(head_commit_message(root).as_deref(), Some("signed"));
        assert_eq!(
            head_commit_record(root).map(|record| record.message),
            Some("signed".to_string())
        );
        assert!(
            !root.join("gpg-invoked").exists(),
            "read-only log queries must not invoke a signature verifier"
        );
    }

    #[test]
    fn git_history_queries_ignore_local_replace_refs() {
        use std::io::Write as _;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        run(&["init", "-q"]);
        std::fs::write(root.join("tracked"), "x\n").unwrap();
        run(&["add", "tracked"]);
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "base",
            ])
            .env("GIT_AUTHOR_DATE", "2024-01-02T03:04:05Z")
            .env("GIT_COMMITTER_DATE", "2024-01-02T03:04:05Z")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );

        let tree = String::from_utf8(run(&["rev-parse", "HEAD^{tree}"]))
            .unwrap()
            .trim()
            .to_string();
        let replacement = format!(
            "tree {tree}\n\
             author Test <test@example.com> 1749283750 +0000\n\
             committer Test <test@example.com> 1749283750 +0000\n\
             \n\
             replacement\n"
        );
        let mut hash_object = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["hash-object", "-t", "commit", "-w", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        hash_object
            .stdin
            .take()
            .unwrap()
            .write_all(replacement.as_bytes())
            .unwrap();
        let replacement_output = hash_object.wait_with_output().unwrap();
        assert!(replacement_output.status.success());
        let replacement_hash = String::from_utf8(replacement_output.stdout).unwrap();
        run(&["replace", "HEAD", replacement_hash.trim()]);

        assert_eq!(
            head_commit_time(root).unwrap().unix_seconds(),
            1_704_164_645,
            "local replace refs must not change HEAD's deterministic timestamp"
        );
        assert_eq!(
            head_commit_message(root).as_deref(),
            Some("base"),
            "local replace refs must not change HEAD's linted message"
        );
        assert_eq!(
            head_commit_record(root).map(|record| record.message),
            Some("base".to_string()),
            "local replace refs must not change HEAD's commit record"
        );
        let blame = blame_lines(root, Path::new("tracked")).expect("tracked file has blame");
        assert_eq!(blame.len(), 1);
        assert_eq!(
            blame[0]
                .author_time
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            1_704_164_645,
            "local replace refs must not change blame's deterministic author time"
        );
    }

    #[test]
    fn head_commit_time_parser_rejects_empty_malformed_and_out_of_range_values() {
        for raw in [
            b"".as_slice(),
            b"\n",
            b" 123\n",
            b"-1\n",
            b"123 extra\n",
            b"253402300800\n",
        ] {
            assert_eq!(
                parse_head_commit_time(raw),
                Err(AnalysisTimeUnavailable::InvalidHeadTime),
                "{raw:?}"
            );
        }
        assert_eq!(
            parse_head_commit_time(b"0\n").unwrap(),
            "1970-01-01".parse().unwrap()
        );
    }

    #[test]
    fn head_message_returns_none_outside_git() {
        let tmp = tempfile::tempdir().unwrap();
        // The low-level helper remains total outside Git. Engine capability
        // routing converts this repository-wide condition to an analysis
        // failure before a commit-message rule dispatches.
        assert!(head_commit_message(tmp.path()).is_none());
    }

    #[test]
    fn parse_porcelain_two_lines_two_commits() {
        // Two source lines, each in its own porcelain block. The
        // first line is from an old commit (1700000000 = 2023-11-15);
        // the second is from a more recent one (1750000000 =
        // 2025-06-15). Both blocks repeat the full metadata per
        // line-porcelain semantics.
        let porcelain = "\
abcd1234abcd1234abcd1234abcd1234abcd1234 1 1 1
author Old Author
author-mail <old@example.com>
author-time 1700000000
author-tz +0000
committer Old Author
committer-mail <old@example.com>
committer-time 1700000000
committer-tz +0000
summary first commit
filename src/main.rs
\told line content
ef01ef01ef01ef01ef01ef01ef01ef01ef01ef01 2 2 1
author New Author
author-mail <new@example.com>
author-time 1750000000
author-tz +0000
committer New Author
committer-mail <new@example.com>
committer-time 1750000000
committer-tz +0000
summary recent commit
filename src/main.rs
\tnew line content
";
        let lines = parse_porcelain(porcelain);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line_number, 1);
        assert_eq!(lines[0].content, "old line content");
        assert_eq!(
            lines[0].author_time,
            UNIX_EPOCH + Duration::from_secs(1_700_000_000)
        );
        assert_eq!(lines[1].line_number, 2);
        assert_eq!(lines[1].content, "new line content");
        assert_eq!(
            lines[1].author_time,
            UNIX_EPOCH + Duration::from_secs(1_750_000_000)
        );
    }

    #[test]
    fn parse_porcelain_handles_previous_marker() {
        // The optional `previous <sha> <name>` line shows up when
        // the line was rewritten — the parser must not get
        // confused by it.
        let porcelain = "\
abcd1234abcd1234abcd1234abcd1234abcd1234 5 5 1
author X
author-mail <x@example.com>
author-time 1700000000
author-tz +0000
committer X
committer-mail <x@example.com>
committer-time 1700000000
committer-tz +0000
summary did a thing
previous 1111111111111111111111111111111111111111 src/old.rs
filename src/main.rs
\tline body
";
        let lines = parse_porcelain(porcelain);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line_number, 5);
        assert_eq!(lines[0].content, "line body");
    }

    #[test]
    fn parse_porcelain_skips_blocks_missing_metadata() {
        // A block whose author-time line is corrupt (non-numeric)
        // should drop that line rather than panic. The next valid
        // block still emits.
        let porcelain = "\
abcd1234abcd1234abcd1234abcd1234abcd1234 1 1 1
author X
author-time not-a-number
filename a.rs
\tbroken
ef01ef01ef01ef01ef01ef01ef01ef01ef01ef01 2 2 1
author Y
author-time 1700000000
filename a.rs
\tworks
";
        let lines = parse_porcelain(porcelain);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].content, "works");
    }

    #[test]
    fn parse_porcelain_omits_uncommitted_zero_sha_lines() {
        let porcelain = "\
0000000000000000000000000000000000000000 1 1 1
author Not Committed Yet
author-time 1999999999
filename a.rs
\tworking tree line
abcd1234abcd1234abcd1234abcd1234abcd1234 2 2 1
author Committed
author-time 1700000000
filename a.rs
\tcommitted line
";
        let lines = parse_porcelain(porcelain);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line_number, 2);
        assert_eq!(lines[0].content, "committed line");
    }

    #[test]
    fn parse_porcelain_supports_sha256_headers_and_zero_ids() {
        let porcelain = "\
0000000000000000000000000000000000000000000000000000000000000000 1 1 1
author Not Committed Yet
author-time 1999999999
filename a.rs
\tworking tree line
abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234 2 2 1
author Committed
author-time 1700000000
filename a.rs
\tcommitted line
";
        let lines = parse_porcelain(porcelain);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line_number, 2);
        assert_eq!(lines[0].content, "committed line");
    }

    // One repository fixture exercises every trust boundary in sequence; keeping
    // its commits together makes attribution failures diagnosable.
    #[allow(clippy::too_many_lines)]
    #[test]
    fn blame_cache_uses_only_tracked_regular_default_ignore_revs_file() {
        const LOCAL_IGNORE_REVS_FILE: &str = ".local-blame-ignore-revs";
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };
        let commit_at = |message: &str, date: &str| {
            let output = Command::new("git")
                .arg("-C")
                .arg(root)
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.com",
                    "commit",
                    "-qm",
                    message,
                ])
                .env("GIT_AUTHOR_DATE", date)
                .env("GIT_COMMITTER_DATE", date)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git commit: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        let author_time = || {
            run(&["show", "-s", "--format=%ct", "HEAD"])
                .parse::<u64>()
                .unwrap()
        };

        run(&["init", "-q"]);
        std::fs::write(root.join("tracked"), "original line here\n").unwrap();
        run(&["add", "tracked"]);
        commit_at("original", "2020-01-02T03:04:05Z");

        std::fs::write(root.join("tracked"), "formatted line here\n").unwrap();
        run(&["add", "tracked"]);
        commit_at("format-only", "2021-02-03T04:05:06Z");
        let locally_ignored = run(&["rev-parse", "HEAD"]);
        let expected_ignored_author_time = author_time();

        std::fs::write(root.join("tracked"), "substantive line here\n").unwrap();
        run(&["add", "tracked"]);
        commit_at("substantive change", "2024-06-07T08:09:10Z");
        let repository_ignored = run(&["rev-parse", "HEAD"]);
        let substantive_author_time = author_time();

        std::fs::write(
            root.join(DEFAULT_BLAME_IGNORE_REVS_FILE),
            format!("{repository_ignored}\n"),
        )
        .unwrap();
        std::fs::write(
            root.join(LOCAL_IGNORE_REVS_FILE),
            format!("{locally_ignored}\n"),
        )
        .unwrap();
        run(&["config", "blame.ignoreRevsFile", LOCAL_IGNORE_REVS_FILE]);

        let untracked_cache = BlameCache::new(root.to_path_buf());
        let untracked_blame = untracked_cache
            .get(Path::new("tracked"))
            .expect("tracked file has blame");
        assert_eq!(
            untracked_blame[0]
                .author_time
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            substantive_author_time,
            "neither an untracked default file nor clone-local config may alter blame"
        );

        run(&["add", DEFAULT_BLAME_IGNORE_REVS_FILE]);
        commit_at("record ignored revision", "2025-01-01T00:00:00Z");
        // A clone-local algorithm must not affect attribution.
        run(&["config", "diff.algorithm", "histogram"]);

        let cache = BlameCache::new(root.to_path_buf());
        let blame = cache
            .get(Path::new("tracked"))
            .expect("tracked file has blame");
        assert_eq!(blame.len(), 1);
        assert_eq!(
            blame[0]
                .author_time
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            expected_ignored_author_time,
            "the tracked repository file must apply, but clone-local config must not"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let target = root.join("ignore-target");
            std::fs::write(&target, format!("{repository_ignored}\n")).unwrap();
            std::fs::remove_file(root.join(DEFAULT_BLAME_IGNORE_REVS_FILE)).unwrap();
            symlink(&target, root.join(DEFAULT_BLAME_IGNORE_REVS_FILE)).unwrap();
            let symlink_cache = BlameCache::new(root.to_path_buf());
            let symlink_blame = symlink_cache
                .get(Path::new("tracked"))
                .expect("tracked file has blame");
            assert_eq!(
                symlink_blame[0]
                    .author_time
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                substantive_author_time,
                "a tracked path replaced by a worktree symlink must not read external ignore data"
            );
        }
    }

    #[test]
    fn blame_cache_pins_diff_algorithm() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        let commit_at = |message: &str, date: &str| {
            let output = Command::new("git")
                .arg("-C")
                .arg(root)
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.com",
                    "commit",
                    "-qam",
                    message,
                ])
                .env("GIT_AUTHOR_DATE", date)
                .env("GIT_COMMITTER_DATE", date)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git commit: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };

        run(&["init", "-q"]);
        std::fs::write(root.join("tracked"), "A\nA\nA\nD\nC\nF\nC\nC\n").unwrap();
        run(&["add", "tracked"]);
        commit_at("base", "2020-01-01T00:00:00Z");
        std::fs::write(root.join("tracked"), "A\nA\nC\nA\nC\nB\nA\nC\nD\n").unwrap();
        commit_at("ambiguous edit", "2024-01-01T00:00:00Z");
        run(&["config", "diff.algorithm", "histogram"]);

        let blame = BlameCache::new(root.to_path_buf())
            .get(Path::new("tracked"))
            .expect("tracked file has blame");
        let author_times: Vec<_> = blame
            .iter()
            .map(|line| {
                line.author_time
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            })
            .collect();
        assert_eq!(
            author_times,
            vec![
                1_577_836_800,
                1_577_836_800,
                1_577_836_800,
                1_704_067_200,
                1_577_836_800,
                1_704_067_200,
                1_704_067_200,
                1_577_836_800,
                1_704_067_200,
            ],
            "clone-local diff.algorithm must not change line ages"
        );
    }

    #[test]
    fn blame_lines_returns_none_outside_git() {
        let tmp = tempfile::tempdir().unwrap();
        // No repo, so blame on anything (existing or not) fails.
        let result = blame_lines(tmp.path(), Path::new("missing.rs"));
        assert!(result.is_none());
    }

    #[test]
    fn blame_cache_memoises_failure() {
        // Calling `get` twice on a non-existent file in a
        // non-git directory must short-circuit on the second
        // call. We can't observe the cache directly from outside,
        // but we can verify both calls return None and the cache
        // ends up with an entry for the path.
        let tmp = tempfile::tempdir().unwrap();
        let cache = BlameCache::new(tmp.path().to_path_buf());
        assert!(cache.get(Path::new("missing.rs")).is_none());
        assert!(cache.get(Path::new("missing.rs")).is_none());
        let guard = cache.inner.lock().unwrap();
        assert!(matches!(
            guard.get(Path::new("missing.rs")),
            Some(CacheEntry::Failed)
        ));
    }

    #[test]
    fn dir_has_tracked_files_walks_prefix() {
        let mut set = HashSet::new();
        set.insert(PathBuf::from("src/main.rs"));
        set.insert(PathBuf::from("README.md"));
        assert!(dir_has_tracked_files(Path::new("src"), &set));
        assert!(!dir_has_tracked_files(Path::new("target"), &set));
        // `src` matches `src/main.rs` via prefix; `tar` does not
        // match `target/foo` because no tracked path is under
        // `tar/`.
        assert!(!dir_has_tracked_files(Path::new("tar"), &set));
    }

    // ----- commit_messages_in_range -----------------------------

    /// Build a temp dir into a git repo with the given list of
    /// empty commits in order (commit N is HEAD~(len-1-N)). Returns
    /// the tempdir so the caller controls its lifetime.
    ///
    /// Uses `git commit --allow-empty` so the test doesn't need to
    /// write fixture files. Disables GPG signing and sets a fixed
    /// author so the commits are deterministic.
    #[test]
    fn commit_range_rejects_dash_since_and_writes_no_file() {
        // Security regression (git arg-injection): a config-controlled
        // `since` starting with `-` (e.g. `--output=…`) must be rejected
        // before git runs — it must never write/truncate a file. (Affects
        // the released `git_commit_message` `since:` path.)
        let outdir = tempfile::tempdir().unwrap();
        let stem = outdir.path().join("sentinel");
        let would_write = outdir.path().join("sentinel..HEAD");
        let evil = format!("--output={}", stem.display());
        let err = commit_messages_in_range(Path::new("."), &evil, false).unwrap_err();
        assert!(matches!(err, CommitRangeError::BadRange { .. }), "{err:?}");
        assert!(
            !would_write.exists(),
            "git must not have written {would_write:?}"
        );
    }

    #[test]
    fn collect_changed_paths_dash_base_writes_no_file() {
        // The `--changed` / `changed_since` diff path: `--end-of-options`
        // forces a dash-leading base into the revision slot, so git never
        // parses `--output=…` and writes nothing.
        let outdir = tempfile::tempdir().unwrap();
        let stem = outdir.path().join("sentinel");
        let would_write = outdir.path().join("sentinel...HEAD");
        let evil = format!("--output={}", stem.display());
        let _ = collect_changed_paths(Path::new("."), Some(&evil));
        assert!(
            !would_write.exists(),
            "git diff must not have written {would_write:?}"
        );
    }

    fn make_repo_with_commits(subjects: &[&str]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let init_dir = tmp.path();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            let out = Command::new("git")
                .arg("-C")
                .arg(init_dir)
                .args(&args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?} failed");
        }
        for subject in subjects {
            let out = Command::new("git")
                .arg("-C")
                .arg(init_dir)
                .args(["commit", "--allow-empty", "-m", subject])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git commit failed: stderr={}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        tmp
    }

    #[test]
    fn head_commit_message_lossily_decodes_non_utf8() {
        // A non-UTF-8 HEAD commit message must NOT make `head_commit_message`
        // return `None` — that would silently no-op the `git_commit_message`
        // rule, letting a non-UTF-8 message bypass subject/pattern/length checks
        // (M6 fail-open, the sibling of the range-mode path already hardened).
        let tmp = make_repo_with_commits(&[]);
        let dir = tmp.path();
        let msg = dir.join("MSG");
        // Raw non-UTF-8 bytes (0xFF/0xFE are never valid UTF-8) in the subject.
        std::fs::write(&msg, b"fix: \xff\xfe non-utf8 subject\n").unwrap();
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["commit", "--allow-empty", "-F"])
            .arg(&msg)
            .output()
            .unwrap();
        assert!(out.status.success(), "commit failed");
        let got = head_commit_message(dir);
        assert!(
            got.is_some(),
            "a non-UTF-8 HEAD message must decode lossily, not collapse to None"
        );
        assert!(
            got.unwrap().contains("non-utf8 subject"),
            "the (lossily-decoded) message body should survive"
        );
    }

    #[test]
    fn parse_commit_log_empty_input() {
        assert_eq!(parse_commit_log(b""), Vec::new());
    }

    #[test]
    fn parse_commit_log_single_commit() {
        // sha NUL name NUL email NUL body-with-trailing-newline RS.
        let raw =
            b"abc1234\0Jane Doe\0jane@example.com\0subject line\n\nbody line one\nbody line two\n\x1e";
        let records = parse_commit_log(raw);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].sha, "abc1234");
        assert_eq!(records[0].author_name, "Jane Doe");
        assert_eq!(records[0].author_email, "jane@example.com");
        assert_eq!(
            records[0].message,
            "subject line\n\nbody line one\nbody line two"
        );
    }

    #[test]
    fn parse_commit_log_multiple_commits() {
        // Two commits, oldest first (matches --reverse). Between
        // records, git inserts a newline before the next SHA; the
        // parser strips it.
        let raw = b"a1\0A\0a@x.test\0first\n\x1e\nb2\0B\0b@x.test\0second\n\x1e";
        let records = parse_commit_log(raw);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].sha, "a1");
        assert_eq!(records[0].author_email, "a@x.test");
        assert_eq!(records[0].message, "first");
        assert_eq!(records[1].sha, "b2");
        assert_eq!(records[1].message, "second");
    }

    #[test]
    fn parse_commit_log_subject_only_no_body() {
        let raw = b"deadbef\0N\0n@x.test\0just the subject\n\x1e";
        let records = parse_commit_log(raw);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].message, "just the subject");
    }

    #[test]
    fn parse_commit_log_preserves_blank_lines_in_body() {
        // A real commit body with multiple paragraphs survives the
        // round-trip unchanged.
        let raw = b"sha7777\0N\0n@x.test\0fix: thing\n\nfirst paragraph.\n\nsecond paragraph.\n\nthird.\n\x1e";
        let records = parse_commit_log(raw);
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].message,
            "fix: thing\n\nfirst paragraph.\n\nsecond paragraph.\n\nthird."
        );
    }

    #[test]
    fn parse_commit_log_lossily_decodes_invalid_utf8_rather_than_dropping() {
        // A record whose message field is invalid UTF-8. The parser must NOT
        // drop it — silently skipping the commit would let a contributor
        // bypass commit linting (conventional-subject / author-allowlist /
        // forbidden-pattern) just by using a non-UTF-8 message or author.
        // It now lossily decodes so the commit is still linted; the sha is
        // always hex, so it survives intact.
        let mut raw: Vec<u8> = b"abc1234\0N\0n@x.test\0".to_vec();
        raw.extend_from_slice(&[0xff, 0xfe, 0xfd]); // invalid UTF-8
        raw.push(0x1e);
        let records = parse_commit_log(&raw);
        assert_eq!(records.len(), 1, "the commit must be parsed, not dropped");
        assert_eq!(records[0].sha, "abc1234");
        assert_eq!(records[0].author_name, "N");
        // The invalid bytes become U+FFFD replacement chars (not silently lost).
        assert!(records[0].message.contains('\u{FFFD}'));
    }

    #[test]
    fn commit_range_returns_none_outside_git() {
        let tmp = tempfile::tempdir().unwrap();
        // A non-Git directory produces the repository-unavailable variant,
        // distinct from a bad range inside a usable repository. Engine
        // capability routing turns this into a typed analysis failure before
        // dependent rules dispatch.
        let result = commit_messages_in_range(tmp.path(), "main", false);
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn commit_range_returns_empty_vec_for_head_to_head() {
        let repo = make_repo_with_commits(&["feat: first commit"]);
        let result = commit_messages_in_range(repo.path(), "HEAD", false).unwrap();
        // HEAD..HEAD is the empty range. Some(empty), not None.
        assert_eq!(result, Some(Vec::new()));
    }

    #[test]
    fn commit_range_enumerates_real_commits_oldest_first() {
        // Four commits. Use the root commit's full SHA as the
        // `since` base; the range then yields the three later
        // commits, oldest first.
        let repo =
            make_repo_with_commits(&["root: zero", "feat: alpha", "fix: beta", "chore: gamma"]);
        let config = Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["config", "core.abbrev", "20"])
            .output()
            .unwrap();
        assert!(config.status.success());
        let root_sha = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(["rev-parse", "HEAD~3"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        let records = commit_messages_in_range(repo.path(), &root_sha, false)
            .unwrap()
            .unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].message, "feat: alpha");
        assert_eq!(records[1].message, "fix: beta");
        assert_eq!(records[2].message, "chore: gamma");
        // The command pins seven characters instead of inheriting core.abbrev;
        // Git may extend only if a prefix is ambiguous.
        for r in &records {
            assert_eq!(r.sha.len(), 7);
            assert!(r.sha.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn commit_range_skips_merges_by_default() {
        // Build the canonical PR-CI shape: a base branch with one
        // commit, a feature branch off it with two commits, then a
        // merge commit on the base branch. The merge is what
        // actions/checkout produces at HEAD on a pull_request
        // trigger.
        let repo = make_repo_with_commits(&["init commit on main"]);
        let root = repo.path();
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap()
        };
        let base_sha = run(&["rev-parse", "HEAD"]).trim().to_string();
        run(&["checkout", "-q", "-b", "feature"]);
        run(&["commit", "--allow-empty", "-m", "feat: A"]);
        run(&["commit", "--allow-empty", "-m", "fix: B"]);
        run(&["checkout", "-q", "main"]);
        run(&["merge", "--no-ff", "--no-edit", "feature"]);

        // Range main-base..HEAD: includes feat:A, fix:B, and the
        // merge commit. Default skips the merge.
        let records = commit_messages_in_range(root, &base_sha, false)
            .unwrap()
            .unwrap();
        let subjects: Vec<&str> = records.iter().map(|r| r.message.as_str()).collect();
        assert_eq!(subjects, vec!["feat: A", "fix: B"]);

        // Same range with include_merges: true picks up the merge.
        let with_merge = commit_messages_in_range(root, &base_sha, true)
            .unwrap()
            .unwrap();
        assert_eq!(with_merge.len(), 3);
        assert!(with_merge.iter().any(|r| r.message.starts_with("Merge ")));
    }

    #[test]
    fn changed_paths_checked_none_outside_git_and_bad_range_inside() {
        // The low-level helper preserves unavailable-repository `None`; engine
        // capability routing turns it into an owning-rule analysis failure.
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(
            collect_changed_paths_checked(tmp.path(), "origin/main"),
            Ok(None)
        ));
        // Inside a repo, an unresolvable ref hard-errors.
        let repo = make_repo_with_commits(&["init"]);
        assert!(matches!(
            collect_changed_paths_checked(repo.path(), "no-such-ref"),
            Err(CommitRangeError::BadRange { .. })
        ));
    }

    #[test]
    fn verify_commit_returns_false_for_unsigned_commit() {
        // make_repo_with_commits disables gpg signing, so HEAD is
        // unsigned; verify-commit exits non-zero → Some(false).
        let repo = make_repo_with_commits(&["init: unsigned commit"]);
        let head = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        assert_eq!(verify_commit(repo.path(), &head), Some(false));
    }

    #[test]
    fn commit_range_returns_bad_range_for_unknown_ref() {
        let repo = make_repo_with_commits(&["init"]);
        let result = commit_messages_in_range(repo.path(), "does-not-exist-ref", false);
        match result {
            Err(CommitRangeError::BadRange { stderr }) => {
                // Git typically says "unknown revision or path not
                // in the working tree." We don't assert the exact
                // wording (varies across git versions); just that
                // we got a non-empty stderr.
                assert_ne!(stderr, "");
            }
            other => panic!("expected BadRange, got {other:?}"),
        }
    }
}
