//! Shared process-spawn helper for the single-shot,
//! command-declaring rule kinds (`generated_file_fresh`,
//! `command_idempotent`).
//!
//! One spawn, stdout + stderr captured **concurrently** on
//! reader threads (so the full output is preserved — the
//! generator's stdout is diffed; the checker's offender list
//! must not be truncated — and a large output cannot deadlock on
//! a full pipe buffer), while a poll loop enforces a timeout and
//! kills a hung child.
//!
//! This is deliberately *not* `command::run_one`: that rule
//! drains *after* exit with a fixed cap (right for its
//! per-file error snippet, wrong here — it would truncate the
//! diff/offender list and risk a pipe-fill deadlock).
//! `command.rs` keeps its own loop; this helper is only for the
//! two single-shot twins.

use std::io::Read as _;
use std::path::Path;
use std::process::{Command as StdCommand, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Default child timeout (seconds) when the rule does not set
/// `timeout:`. Generous for a single-shot whole-repo generator /
/// checker, yet bounded so a deadlocked child cannot hang CI
/// indefinitely.
pub(crate) const DEFAULT_SPAWN_TIMEOUT_SECS: u64 = 120;

/// Poll granularity of the wait loop. Short enough that a fast
/// child is not noticeably delayed; long enough to keep the CPU
/// idle while the child runs.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Outcome of [`run_capturing`].
pub(crate) enum SpawnOutcome {
    /// Child exited; full stdout/stderr captured.
    Exited {
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    /// `spawn()` (or the post-spawn wait) failed.
    SpawnError(std::io::Error),
    /// Killed after exceeding the timeout.
    TimedOut { secs: u64 },
}

/// Spawn `argv` in `cwd` with stdin closed and `env` pairs set,
/// draining stdout+stderr concurrently while enforcing
/// `timeout`. On exit returns the status + full captured output;
/// on a spawn/wait error returns [`SpawnOutcome::SpawnError`];
/// past `timeout` the child is killed and
/// [`SpawnOutcome::TimedOut`] is returned (captured output
/// discarded).
pub(crate) fn run_capturing(
    argv: &[String],
    cwd: &Path,
    env: &[(&str, String)],
    timeout: Duration,
) -> SpawnOutcome {
    let Some((program, rest)) = argv.split_first() else {
        return SpawnOutcome::SpawnError(std::io::Error::other(
            "run_capturing called with an empty argv",
        ));
    };
    let mut cmd = StdCommand::new(program);
    cmd.args(rest)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return SpawnOutcome::SpawnError(e),
    };

    // Concurrent drain: the child can produce more than a pipe
    // buffer's worth before exiting, so reading only after exit
    // (capped or not) would deadlock or truncate.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_h = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let err_h = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = out_h.join().unwrap_or_default();
                let stderr = err_h.join().unwrap_or_default();
                return SpawnOutcome::Exited {
                    status,
                    stdout,
                    stderr,
                };
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out_h.join();
                    let _ = err_h.join();
                    return SpawnOutcome::TimedOut {
                        secs: timeout.as_secs(),
                    };
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out_h.join();
                let _ = err_h.join();
                return SpawnOutcome::SpawnError(e);
            }
        }
    }
}
