//! Deterministic (Callgrind) BINARY benchmark — end-to-end `alint check` over
//! fixed `gen-monorepo` trees, measuring the REAL release `alint` binary.
//!
//! Unlike the in-process `det_engine` library bench, this runs the actual CLI
//! as a separate process under Valgrind — no toggle/inlining concerns — so it
//! catches walker / IO / dispatch regressions end to end (e.g. the walker
//! `filter_entry` indirect-mispredict class from
//! `docs/benchmarks/investigations/2026-06-v0.12-perf-validation/`). This is the
//! load-bearing regression gate.
//!
//! Build the release binary first, then run:
//!
//! ```sh
//! cargo build --release -p alint
//! cargo bench -p alint-bench --bench det_check
//! ```
//!
//! Gate: `Ir` soft-limited at +2% vs baseline; branch mispredicts (`Bcm`/`Bim`)
//! at a +50% advisory ceiling. Design: `docs/design/deterministic-perf-gating.md`.

use std::path::{Path, PathBuf};

use gungraun::{
    BinaryBenchmarkConfig, Callgrind, Command, EventKind, binary_benchmark, binary_benchmark_group,
    main,
};

/// 0xA11E47 — the canonical bench seed (byte-identical trees across runs).
const SEED: u64 = 10_559_047;

// The real scenario configs, shared with the wall-clock bench (minimal drift —
// one source of truth). A spread of dispatch classes that exercise the regular
// gen-monorepo tree: S1 = filename-only (isolates the walker); S2 = existence +
// content; S6 = dense per-file content; S7 = cross-file relational; S12 = the
// v0.10 per-file dispatch class.
const S1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s1_filename.yml"
));
const S2: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s2_existence_content.yml"
));
const S6: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s6_per_file_content.yml"
));
const S7: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s7_cross_file_relational.yml"
));
const S12: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../xtask/src/bench/scenarios/s12_v010_per_file.yml"
));

fn workspace_target() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target")
}

fn alint_bin() -> PathBuf {
    workspace_target().join("release/alint")
}

/// `(packages, files_per_package)` for a size, matching the bench tree shapes.
fn shape(n: usize) -> (usize, usize) {
    match n {
        10_000 => (200, 48),
        100_000 => (1_000, 98),
        _ => (50, 18), // 1k
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dst = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dst);
        } else {
            std::fs::copy(entry.path(), &dst).unwrap();
        }
    }
}

/// The deterministic fixed path for a `(scenario, n)` tree. A fixed path (not
/// the gen'd `TempDir`) keeps the absolute paths — and thus `Ir` — byte-stable
/// run to run. Shared by `materialize` (setup) and `check` (the bench fn), which
/// both receive the same `#[bench::…]` args.
fn tree_path(scenario: &str, n: usize) -> PathBuf {
    workspace_target()
        .join("det-trees")
        .join(format!("{scenario}-{n}"))
}

/// setup: materialize the fixed tree + drop the scenario config in as
/// `.alint.yml`. A pure side effect — the bench fn recomputes the same path.
fn materialize(scenario: &str, config: &str, n: usize) {
    let (packages, fpp) = shape(n);
    let tree = alint_bench::tree::generate_monorepo(packages, fpp, SEED).unwrap();
    let dest = tree_path(scenario, n);
    let _ = std::fs::remove_dir_all(&dest);
    copy_dir(tree.root(), &dest);
    std::fs::write(dest.join(".alint.yml"), config).unwrap();
}

// Per-PR gate runs 1k + 10k (seconds under valgrind). The 100k tier is heavier
// (~100s/cell) and gated behind the `det-100k` feature for release-time runs
// (`cargo bench -p alint-bench --bench det_check --features det-100k`).
#[binary_benchmark(setup = materialize)]
#[bench::s1_1k("s1", S1, 1_000)]
#[bench::s1_10k("s1", S1, 10_000)]
#[cfg_attr(feature = "det-100k", bench::s1_100k("s1", S1, 100_000))]
#[bench::s2_1k("s2", S2, 1_000)]
#[bench::s2_10k("s2", S2, 10_000)]
#[cfg_attr(feature = "det-100k", bench::s2_100k("s2", S2, 100_000))]
#[bench::s6_1k("s6", S6, 1_000)]
#[bench::s6_10k("s6", S6, 10_000)]
#[cfg_attr(feature = "det-100k", bench::s6_100k("s6", S6, 100_000))]
#[bench::s7_1k("s7", S7, 1_000)]
#[bench::s7_10k("s7", S7, 10_000)]
#[cfg_attr(feature = "det-100k", bench::s7_100k("s7", S7, 100_000))]
#[bench::s12_1k("s12", S12, 1_000)]
#[bench::s12_10k("s12", S12, 10_000)]
#[cfg_attr(feature = "det-100k", bench::s12_100k("s12", S12, 100_000))]
fn check(scenario: &str, config: &str, n: usize) -> Command {
    let _ = config; // consumed by `materialize` (setup); not needed to build the command
    Command::new(alint_bin())
        .arg("check")
        .arg(tree_path(scenario, n))
        .build()
}

binary_benchmark_group!(name = check_grp, benchmarks = check);

main!(
    config = BinaryBenchmarkConfig::default().tool(
        Callgrind::default()
            .args(["--cache-sim=yes", "--branch-sim=yes"])
            .soft_limits([
                (EventKind::Ir, 2.0),
                (EventKind::Bcm, 50.0),
                (EventKind::Bim, 50.0),
            ]),
    ),
    binary_benchmark_groups = check_grp
);
