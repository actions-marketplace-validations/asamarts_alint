//! `xtask bench-release` — the quick / publish-grade local
//! benchmark runner. See main.rs for the CLI surface.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

// ---- bench-release ---------------------------------------------------------

const RULES_CONFIG_YAML: &str = include_str!("bench_config.yml");

pub(crate) fn bench_release(quick: bool, out: Option<PathBuf>, seed: u64) -> Result<()> {
    ensure_hyperfine()?;

    let binary = build_release_binary()?;
    let sizes: &[usize] = if quick {
        &[500]
    } else {
        &[1_000, 10_000, 100_000]
    };

    // Write the shared config file once to a tempdir and point every run at it.
    let config_dir = tempfile::tempdir()?;
    let config_path = config_dir.path().join(".alint.yml");
    fs::write(&config_path, RULES_CONFIG_YAML)?;

    let mut report = String::new();
    write_header(&mut report, quick, seed)?;

    for &size in sizes {
        eprintln!("[xtask] generating tree of {size} files (seed={seed})...");
        let tree = alint_bench::tree::generate_tree(size, 4, seed)?;
        // hyperfine doesn't care about CWD; we pass the tree path as an argument.
        let target_path = tree.root();
        // Copy the config into the tree so `alint check <path>` discovers it.
        fs::copy(&config_path, target_path.join(".alint.yml"))?;

        eprintln!("[xtask] running hyperfine against {size}-file tree...");
        let md = run_hyperfine(&binary, target_path, size, quick)?;
        writeln!(&mut report, "\n### {size} files\n")?;
        writeln!(&mut report, "{md}")?;
    }

    match out {
        Some(path) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, report)?;
            eprintln!("[xtask] wrote {}", path.display());
        }
        None => print!("{report}"),
    }
    Ok(())
}

pub(crate) fn ensure_hyperfine() -> Result<()> {
    match Command::new("hyperfine").arg("--version").output() {
        Ok(out) if out.status.success() => Ok(()),
        _ => bail!(
            "hyperfine not found in PATH. Install it with:\n  \
             cargo install hyperfine\n  # or apt/brew/choco install hyperfine"
        ),
    }
}

pub(crate) fn build_release_binary() -> Result<PathBuf> {
    eprintln!("[xtask] cargo build --release -p alint");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "-p", "alint"])
        .status()
        .context("invoking cargo")?;
    if !status.success() {
        bail!("release build failed");
    }
    let workspace_root = workspace_root()?;
    let bin = workspace_root
        .join("target")
        .join("release")
        .join(if cfg!(windows) { "alint.exe" } else { "alint" });
    if !bin.is_file() {
        bail!("expected binary at {}", bin.display());
    }
    Ok(bin)
}

pub(crate) fn workspace_root() -> Result<PathBuf> {
    // xtask is inside the workspace; CARGO_MANIFEST_DIR = alint/xtask; parent = workspace root.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(manifest)
        .parent()
        .context("xtask has no parent directory")?;
    Ok(root.to_path_buf())
}

pub(crate) fn run_hyperfine(
    binary: &Path,
    tree_root: &Path,
    size: usize,
    quick: bool,
) -> Result<String> {
    let warmup = if quick { "2" } else { "5" };
    let min_runs = if quick { "3" } else { "10" };

    let tmp_md = tempfile::NamedTempFile::new()?;
    let md_path = tmp_md.path().to_path_buf();

    let status = Command::new("hyperfine")
        .args(["--warmup", warmup, "--min-runs", min_runs])
        .arg("--command-name")
        .arg(format!("alint check (synthetic, {size} files)"))
        .arg("--export-markdown")
        .arg(&md_path)
        .arg(format!(
            "{} check {}",
            shell_quote(binary.to_str().unwrap()),
            shell_quote(tree_root.to_str().unwrap())
        ))
        .status()
        .context("invoking hyperfine")?;
    if !status.success() {
        bail!("hyperfine exited non-zero");
    }
    Ok(fs::read_to_string(&md_path)?)
}

pub(crate) fn shell_quote(s: &str) -> String {
    if s.chars().any(|c| c == ' ' || c == '\t') {
        format!("\"{s}\"")
    } else {
        s.to_string()
    }
}

pub(crate) fn write_header(report: &mut String, quick: bool, seed: u64) -> Result<()> {
    writeln!(
        report,
        "# alint bench-release results\n\n\
         **Mode:** {mode}  \n\
         **Seed:** `{seed:#x}`  \n\
         **OS:** `{os}/{arch}`  \n\
         **rustc:** `{rustc}`  \n\
         **alint git SHA:** `{sha}`  \n\
         **Generated:** {ts}  \n\n\
         Results measured with `hyperfine` on this machine. Cross-machine \
         variance is expected; see `docs/benchmarks/METHODOLOGY.md` for the \
         reproduction recipe. Do not compare absolute numbers across \
         rows in different files — compare like-for-like.",
        mode = if quick { "quick (smoke)" } else { "full" },
        seed = seed,
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        rustc = rustc_version().unwrap_or_else(|| "unknown".to_string()),
        sha = git_sha().unwrap_or_else(|| "unknown".to_string()),
        ts = now_iso(),
    )?;
    Ok(())
}

pub(crate) fn rustc_version() -> Option<String> {
    let out = Command::new("rustc").arg("--version").output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

pub(crate) fn git_sha() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

pub(crate) fn now_iso() -> String {
    // Minimal ISO-ish timestamp without pulling in chrono.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("unix:{secs}")
}
