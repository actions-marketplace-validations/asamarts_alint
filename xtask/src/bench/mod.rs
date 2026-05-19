//! `bench-scale` — the v0.5 scale-ceiling benchmark.
//!
//! Two orthogonal dimensions:
//!
//! - **size**: 1k / 10k / 100k / 1m files. The 1m size is opt-in
//!   via `--include-1m` because it generates ~3-5 GB of synthetic
//!   data and runs in minutes, not seconds.
//! - **mode**: `full` (every file evaluated) and `changed` (a
//!   deterministic subset modified post-commit, then `alint check
//!   --changed` measures the v0.5.0 incremental path).
//!
//! Each (size, mode, scenario) triple becomes one hyperfine row.
//! Scenarios live in `scenarios/*.yml` — three configs spanning
//! filename hygiene (S1), existence + content (S2), and the
//! full workspace bundle (S3).
//!
//! Output: a per-platform, per-version directory under
//! `docs/benchmarks/macro/results/<os>-<arch>/<workspace-version>/`
//! containing a `results.json` (machine-readable) plus per-size
//! `results.md` files and an `index.md` summary. Cross-machine
//! comparisons always require like-for-like (same fingerprint) —
//! see `docs/benchmarks/METHODOLOGY.md`. Cross-version comparisons
//! walk per-version dirs; see `docs/benchmarks/HISTORY.md` for
//! the headline cross-release table.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub mod compare;
pub mod docker;
mod fingerprint;
pub mod gate;
pub mod tools;

pub use tools::Tool;

/// Embedded scenario YAMLs. Each ships in the xtask binary so
/// running on any cloned checkout produces byte-identical
/// configs without depending on workspace-relative path resolution.
const SCENARIO_S1: &str = include_str!("scenarios/s1_filename.yml");
const SCENARIO_S2: &str = include_str!("scenarios/s2_existence_content.yml");
const SCENARIO_S3: &str = include_str!("scenarios/s3_workspace.yml");
const SCENARIO_S4: &str = include_str!("scenarios/s4_agent_hygiene.yml");
const SCENARIO_S5: &str = include_str!("scenarios/s5_fix_pass.yml");
const SCENARIO_S6: &str = include_str!("scenarios/s6_per_file_content.yml");
const SCENARIO_S7: &str = include_str!("scenarios/s7_cross_file_relational.yml");
const SCENARIO_S8: &str = include_str!("scenarios/s8_git_overlay.yml");
const SCENARIO_S9: &str = include_str!("scenarios/s9_nested_polyglot.yml");
const SCENARIO_S10: &str = include_str!("scenarios/s10_scope_filter_outside_per_file.yml");
const SCENARIO_S11: &str = include_str!("scenarios/s11_v010_cross_file.yml");
const SCENARIO_S12: &str = include_str!("scenarios/s12_v010_per_file.yml");
const SCENARIO_S13: &str = include_str!("scenarios/s13_v010_single_shot.yml");

/// Parameters parsed from CLI flags. Defaults pick the
/// "publish-grade run" — full size matrix (excluding 1m), all
/// scenarios, both modes — so a bare `xtask bench-scale`
/// produces a committable result.
#[derive(Debug, Clone)]
pub struct ScaleArgs {
    pub sizes: Vec<Size>,
    pub scenarios: Vec<Scenario>,
    pub modes: Vec<Mode>,
    pub tools: Vec<Tool>,
    pub warmup: u32,
    pub runs: u32,
    pub seed: u64,
    pub diff_pct: f64,
    pub out: Option<PathBuf>,
    pub quick: bool,
    pub json_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Size {
    /// 1,000 files — small repo / smoke test.
    K1,
    /// 10,000 files — small-to-mid monorepo.
    K10,
    /// 100,000 files — workspace-tier upper bound.
    K100,
    /// 1,000,000 files — Bazel territory; opt-in.
    M1,
}

impl Size {
    /// Parse the `--sizes` flag's comma-separated values.
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "1k" => Ok(Self::K1),
            "10k" => Ok(Self::K10),
            "100k" => Ok(Self::K100),
            "1m" => Ok(Self::M1),
            other => bail!("unknown size {other:?}; expected one of 1k, 10k, 100k, 1m"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::K1 => "1k",
            Self::K10 => "10k",
            Self::K100 => "100k",
            Self::M1 => "1m",
        }
    }

    pub fn file_count(self) -> usize {
        match self {
            Self::K1 => 1_000,
            Self::K10 => 10_000,
            Self::K100 => 100_000,
            Self::M1 => 1_000_000,
        }
    }

    /// `(packages, files_per_package)` for the monorepo
    /// generator that hits this size's file count exactly.
    /// Each package contributes `2 + files_per_package` files
    /// (Cargo.toml + README + N source files); plus the
    /// workspace root Cargo.toml. Tunes the package count to
    /// keep `files_per_package` in a reasonable range
    /// (10-100), so per-package work matches realistic
    /// monorepos.
    pub fn monorepo_shape(self) -> (usize, usize) {
        match self {
            Self::K1 => (50, 18),     // 50 * 20 + 1 = 1001
            Self::K10 => (200, 48),   // 200 * 50 + 1 = 10001
            Self::K100 => (1000, 98), // 1000 * 100 + 1 = 100001
            Self::M1 => (5000, 198),  // 5000 * 200 + 1 = 1000001
        }
    }

    pub fn is_opt_in(self) -> bool {
        matches!(self, Self::M1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scenario {
    S1,
    S2,
    S3,
    S4,
    S5,
    S6,
    S7,
    S8,
    S9,
    S10,
    /// v0.10 cross-file dispatch class — `registry_paths_resolve`,
    /// `cross_file_value_equals`, `pair_hash` on the regular tree
    /// + a `manifest.sha256` overlay (see `setup_overlay`).
    S11,
    /// v0.10 per-file dispatch class — `ordered_block`,
    /// `import_gate`, `xml_path_*` on the regular tree + one
    /// root-level `.csproj` overlay (see `setup_overlay`).
    S12,
    /// v0.10 single-shot dispatch class — `generated_file_fresh`
    /// + `command_idempotent` declared with `command: ["true"]`
    ///   so the row measures `crate::spawn::run_capturing`, not
    ///   the user's tool. Needs a `.gff_target` overlay file.
    S13,
}

impl Scenario {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_uppercase().as_str() {
            "S1" => Ok(Self::S1),
            "S2" => Ok(Self::S2),
            "S3" => Ok(Self::S3),
            "S4" => Ok(Self::S4),
            "S5" => Ok(Self::S5),
            "S6" => Ok(Self::S6),
            "S7" => Ok(Self::S7),
            "S8" => Ok(Self::S8),
            "S9" => Ok(Self::S9),
            "S10" => Ok(Self::S10),
            "S11" => Ok(Self::S11),
            "S12" => Ok(Self::S12),
            "S13" => Ok(Self::S13),
            other => bail!("unknown scenario {other:?}; expected one of S1..S13"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::S1 => "S1",
            Self::S2 => "S2",
            Self::S3 => "S3",
            Self::S4 => "S4",
            Self::S5 => "S5",
            Self::S6 => "S6",
            Self::S7 => "S7",
            Self::S8 => "S8",
            Self::S9 => "S9",
            Self::S10 => "S10",
            Self::S11 => "S11",
            Self::S12 => "S12",
            Self::S13 => "S13",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::S1 => "Filename hygiene (8 rules)",
            Self::S2 => "Existence + content (8 rules)",
            Self::S3 => "Workspace bundle (oss-baseline + rust + monorepo + cargo-workspace)",
            Self::S4 => "Agent-era hygiene (5 rules: backup/scratch/debug/affirmation/model-TODO)",
            Self::S5 => "Fix-pass throughput (4 content-editing fix ops)",
            Self::S6 => "Per-file content fan-out (13 content rules over `**/*.rs`)",
            Self::S7 => {
                "Cross-file relational (pair / unique_by / for_each_dir / for_each_file / dir_only_contains / every_matching_has)"
            }
            Self::S8 => {
                "Git-tracked overlay (S3 + git_no_denied_paths + git_tracked_only over a real git repo)"
            }
            Self::S9 => {
                "Nested polyglot monorepo (rust + node + python rulesets over crates/ + packages/ + apps/)"
            }
            Self::S10 => {
                "scope_filter on rules outside the PerFileRule path (file_max_size / no_empty_files / no_symlinks / filename_case / filename_regex with has_ancestor narrowing)"
            }
            Self::S11 => {
                "v0.10 cross-file (registry_paths_resolve / cross_file_value_equals / pair_hash)"
            }
            Self::S12 => "v0.10 per-file (ordered_block / import_gate / xml_path_*)",
            Self::S13 => {
                "v0.10 single-shot (generated_file_fresh / command_idempotent, command=[\"true\"])"
            }
        }
    }

    pub fn config_yaml(self) -> &'static str {
        match self {
            Self::S1 => SCENARIO_S1,
            Self::S2 => SCENARIO_S2,
            Self::S3 => SCENARIO_S3,
            Self::S4 => SCENARIO_S4,
            Self::S5 => SCENARIO_S5,
            Self::S6 => SCENARIO_S6,
            Self::S7 => SCENARIO_S7,
            Self::S8 => SCENARIO_S8,
            Self::S9 => SCENARIO_S9,
            Self::S10 => SCENARIO_S10,
            Self::S11 => SCENARIO_S11,
            Self::S12 => SCENARIO_S12,
            Self::S13 => SCENARIO_S13,
        }
    }

    /// True for scenarios whose tree must be the v0.9.6
    /// nested-polyglot shape (rust + node + python packages
    /// distributed across `crates/` + `packages/` + `apps/`).
    /// Drives `bench-scale`'s tree-gen path: the v0.9.6
    /// `scope_filter:` primitive only fires meaningfully when
    /// per-rule rules from different ecosystems compete for the
    /// same files, which the standard Cargo-workspace tree
    /// doesn't exercise.
    pub fn requires_polyglot_tree(self) -> bool {
        matches!(self, Self::S9 | Self::S10)
    }

    /// True for scenarios whose tree must be a real git repo
    /// (`.git/` initialised, every file `git add`'d + commit
    /// at generation time). Drives `bench-scale`'s tree-gen
    /// path: `Engine::collect_git_tracked_if_needed` +
    /// `BlameCache` only fire inside a real repo, so the
    /// dispatch shape they produce is invisible without one.
    pub fn requires_git_repo(self) -> bool {
        matches!(self, Self::S8)
    }

    /// Every value of the enum, in declaration order. Drives
    /// the `xtask bench-scale` "all scenarios" default + the
    /// parse-validation unit test in this module.
    #[allow(dead_code)] // exercised by `#[cfg(test)]` only today; retained for the publish-grade default.
    pub fn all() -> &'static [Scenario] {
        &[
            Self::S1,
            Self::S2,
            Self::S3,
            Self::S4,
            Self::S5,
            Self::S6,
            Self::S7,
            Self::S8,
            Self::S9,
            Self::S10,
            Self::S11,
            Self::S12,
            Self::S13,
        ]
    }

    /// Materialise this scenario's fixture overlay into the
    /// generated tree (a sibling of `tool.setup_config` — that
    /// writes the per-tool config, this writes the per-scenario
    /// data files the config references). Called once per
    /// scenario per size, paired with [`Scenario::teardown_overlay`]
    /// so the overlay never persists across scenarios that share
    /// the regular tree. No-op for S1..S10; S11/S12/S13 each
    /// write a tiny, deterministic fixture.
    pub fn setup_overlay(self, root: &Path) -> Result<()> {
        match self {
            Self::S11 => std::fs::write(
                root.join("manifest.sha256"),
                // 64-char all-zeros sha256 + a path token that
                // matches no real file in the synthetic tree;
                // pair_hash (format: contains) finds every source's
                // hash absent and flags it. Cost is deterministic
                // per row.
                "0000000000000000000000000000000000000000000000000000000000000000  fixture\n",
            )
            .with_context(|| format!("writing S11 manifest.sha256 to {}", root.display()))?,
            Self::S12 => std::fs::write(
                root.join("sample.csproj"),
                concat!(
                    "<Project Sdk=\"Microsoft.NET.Sdk\">",
                    "<PropertyGroup>",
                    "<TargetFramework>net8.0</TargetFramework>",
                    "</PropertyGroup>",
                    "</Project>\n",
                ),
            )
            .with_context(|| format!("writing S12 sample.csproj to {}", root.display()))?,
            Self::S13 => std::fs::write(root.join(".gff_target"), b"")
                .with_context(|| format!("writing S13 .gff_target to {}", root.display()))?,
            _ => {}
        }
        Ok(())
    }

    /// Remove this scenario's fixture overlay so the next
    /// scenario on the shared tree sees a pristine state.
    /// Missing files are ignored — calling teardown without a
    /// matching setup must not error.
    pub fn teardown_overlay(self, root: &Path) -> Result<()> {
        let path = match self {
            Self::S11 => Some(root.join("manifest.sha256")),
            Self::S12 => Some(root.join("sample.csproj")),
            Self::S13 => Some(root.join(".gff_target")),
            _ => None,
        };
        if let Some(p) = path {
            match std::fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(e).with_context(|| format!("removing overlay {}", p.display()));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Full,
    Changed,
}

impl Mode {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "full" => Ok(Self::Full),
            "changed" => Ok(Self::Changed),
            other => bail!("unknown mode {other:?}; expected `full` or `changed`"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Changed => "changed",
        }
    }
}

/// One hyperfine row in the report. Times are in milliseconds
/// (hyperfine reports seconds; we convert at parse time so
/// the output schema stays fixed at "ms").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    /// Tool name (`alint`, `ls-lint`, …). Identifies which
    /// implementation produced this row.
    pub tool: String,
    pub size_files: usize,
    pub size_label: String,
    pub scenario: String,
    pub mode: String,
    pub mean_ms: f64,
    pub stddev_ms: f64,
    pub median_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub samples: usize,
    pub command: String,
}

/// Top-level result document — one per `bench-scale`
/// invocation. Serialised to `results.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub fingerprint: fingerprint::Fingerprint,
    pub args: ReportArgs,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportArgs {
    pub seed: String,
    pub diff_pct: f64,
    pub warmup: u32,
    pub runs: u32,
    pub sizes: Vec<String>,
    pub scenarios: Vec<String>,
    pub modes: Vec<String>,
    pub tools: Vec<String>,
}

mod output;
mod run;

pub use run::bench_scale;

// ─── Helpers ─────────────────────────────────────────────────────────

fn ensure_hyperfine() -> Result<()> {
    match Command::new("hyperfine").arg("--version").output() {
        Ok(out) if out.status.success() => Ok(()),
        _ => bail!(
            "hyperfine not found in PATH. Install:\n  cargo install hyperfine\n  \
             # or apt/brew/choco install hyperfine"
        ),
    }
}

fn build_release_binary() -> Result<PathBuf> {
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

fn workspace_root() -> Result<PathBuf> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(manifest)
        .parent()
        .context("xtask has no parent directory")?;
    Ok(root.to_path_buf())
}

#[allow(dead_code)] // re-exported by main.rs but the linter doesn't see across mods.
pub(crate) fn now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("unix:{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every scenario's embedded YAML must load cleanly
    /// through `alint-dsl::load` AND every rule it declares
    /// must build through the alint-rules registry. A typo
    /// in an `include_str!` path, in a rule kind name, or in a
    /// per-kind option fails at `cargo test` time, BEFORE the
    /// publish-grade `xtask bench-scale` invocation tries to
    /// write the broken file as `.alint.yml` and hyperfine
    /// reports a runtime error halfway through.
    ///
    /// Uses `load` rather than `parse` so `extends:` resolves
    /// (S3 needs it); the registry-build pass is the same
    /// chain `alint check` walks at startup.
    #[test]
    fn every_scenario_yaml_loads_and_every_rule_builds() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let registry = alint_rules::builtin_registry();
        for &s in Scenario::all() {
            let path = tmp.path().join(format!("{}.alint.yml", s.label()));
            std::fs::write(&path, s.config_yaml())
                .unwrap_or_else(|e| panic!("writing {}: {e}", s.label()));
            let config = alint_dsl::load(&path)
                .unwrap_or_else(|e| panic!("scenario {} failed to load: {e}", s.label()));
            for spec in &config.rules {
                registry.build(spec).unwrap_or_else(|e| {
                    panic!(
                        "scenario {} rule {:?} failed to build: {e}",
                        s.label(),
                        spec.id
                    )
                });
            }
        }
    }

    /// `Scenario::all()` must enumerate every variant — the
    /// `parse` / `label` match arms cover S1..S13, so `all()`
    /// must too. Detects "added an enum variant, forgot to
    /// update `all()`".
    #[test]
    fn all_covers_every_parsed_label() {
        for &s in Scenario::all() {
            let parsed = Scenario::parse(s.label())
                .unwrap_or_else(|e| panic!("label {} fails to round-trip: {e}", s.label()));
            assert_eq!(parsed, s, "round-trip mismatch for {}", s.label());
        }
    }

    /// Overlay setup / teardown must be tolerant of being
    /// called on a no-op scenario AND of teardown running
    /// without a prior setup (which is what happens if a run
    /// is interrupted between the two).
    #[test]
    fn overlay_hooks_are_idempotent_and_no_op_for_legacy_scenarios() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for &s in Scenario::all() {
            s.setup_overlay(tmp.path()).expect("setup");
            // calling teardown twice must succeed (the second
            // call hits the NotFound branch).
            s.teardown_overlay(tmp.path()).expect("first teardown");
            s.teardown_overlay(tmp.path()).expect("second teardown");
        }
    }
}
