//! Bake commit SHA + build date into the alint binary so
//! `alint --version` includes them. Useful for reproducing bug
//! reports — adopters can paste the full version string and a
//! maintainer can pinpoint the exact commit and build date.
//!
//! Falls back gracefully when git isn't available (e.g. when
//! the crate is built from a published tarball without a git
//! tree). In that case the SHA is set to `unknown` and
//! `--version` prints the workspace version with it.
//!
//! Rerun tracking: cargo reruns this script when a path named by
//! `rerun-if-changed` changes, and treats a path that does not exist
//! as changed on every build. A linked git worktree's `.git` is a file,
//! so a fixed `../../.git/HEAD` made every worktree build rerun the
//! script and recompile the crate. The script asks git where this
//! checkout's files live and names only paths that exist: the
//! checkout's `HEAD` and the branch ref it points to. A commit on the
//! checked-out branch reruns it; commits on other branches or in other
//! worktrees do not. `ci/scripts/test-build-script-freshness.sh` drives
//! real cargo builds through each case.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Repository-local variables (the list `git rev-parse --local-env-vars`
/// prints). Git hooks and wrapper scripts export some of them: a linked
/// worktree's hooks get an absolute `GIT_DIR`, so a hook in one checkout
/// that builds another would bake the first checkout's commit, and a
/// relative `GIT_DIR` makes git fail from the crate directory. The git
/// subprocesses below run without them and discover the repository from
/// the crate directory.
const GIT_LOCAL_ENV_VARS: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_DIR",
    "GIT_GRAFT_FILE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_OBJECT_DIRECTORY",
    "GIT_PREFIX",
    "GIT_REPLACE_REF_BASE",
    "GIT_SHALLOW_FILE",
    "GIT_WORK_TREE",
];

fn main() {
    let sha = git(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let date = build_date_utc();
    println!("cargo:rustc-env=ALINT_GIT_SHA={sha}");
    println!("cargo:rustc-env=ALINT_BUILD_DATE={date}");
    // Naming any path turns off cargo's default of rerunning on every
    // package file change; outside a git tree this is the only one.
    println!("cargo:rerun-if-changed=build.rs");
    for path in head_watch_paths() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

/// The files whose change can move `HEAD`'s commit, limited to the ones
/// that exist. Empty outside a git tree.
fn head_watch_paths() -> Vec<PathBuf> {
    let Some(dirs) = git(&["rev-parse", "--git-dir", "--git-common-dir"]) else {
        return Vec::new();
    };
    let mut dirs = dirs.lines().map(absolute);
    let (Some(git_dir), Some(common_dir)) = (dirs.next(), dirs.next()) else {
        return Vec::new();
    };

    // `HEAD` is per worktree; it changes on a branch switch and on every
    // commit while detached.
    let mut paths = vec![git_dir.join("HEAD")];
    // Branch refs are shared by every worktree, under the common dir.
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        let loose = common_dir.join(&branch);
        if loose.is_file() {
            paths.push(loose);
        } else {
            // A packed ref has no loose file to watch. A commit on it
            // appends to the branch's reflog (and writes the loose file,
            // which the rerun then picks up); `git pack-refs` rewrites
            // `packed-refs`. Never watch the whole `refs/heads`
            // directory: every worktree's commits touch it.
            paths.push(common_dir.join("logs").join(&branch));
            paths.push(common_dir.join("packed-refs"));
        }
    }
    // The reftable backend keeps every ref in one table stack, so any
    // ref update reruns the script there.
    paths.push(common_dir.join("reftable").join("tables.list"));
    paths.retain(|path| path.exists());
    paths
}

/// Run git from the crate directory with the repository-local
/// environment cleared; `None` when git is missing or fails.
fn git(args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command.args(args);
    for var in GIT_LOCAL_ENV_VARS {
        command.env_remove(var);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// git prints these paths relative to the current directory, which for
/// a build script is the crate directory.
fn absolute(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    }
}

fn build_date_utc() -> String {
    // YYYY-MM-DD UTC, derived from system clock at build time.
    // Avoids pulling in `chrono` for one date string — manual
    // computation from epoch seconds is good enough.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // 86_400 seconds/day — `now / 86_400` is bounded by the
    // current epoch-day, well below i64::MAX even centuries from
    // now. Wrapping is structurally impossible.
    #[allow(clippy::cast_possible_wrap)]
    let days_since_epoch = (now / 86_400) as i64;
    let (year, month, day) = days_to_ymd(days_since_epoch);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Convert days-since-epoch (1970-01-01) to (year, month, day).
/// Standard astronomical algorithm; valid for any year in
/// `[1970, ~25_000]`. The `as` casts below are bounded by the
/// algorithm's invariants (`doe` ∈ `[0, 146_097)`, `yoe` ∈ `[0, 400)`,
/// `m` ∈ `[1, 12]`, `d` ∈ `[1, 31]`, `y` ∈ `[1970, ~25_000]`) so the
/// truncation/wrap/sign-loss lints fire on shapes that never
/// actually occur. Suppressing at the function level keeps the
/// astronomical algorithm readable; spelling each one out as
/// `try_from(...).expect(...)` would obscure the math.
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Shift so day 0 = 1970-03-01 (start of leap-year cycle).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (if m <= 2 { y + 1 } else { y }) as i32;
    (y, m, d)
}
