//! Deterministic (Callgrind) library benchmark of the rule engine.
//!
//! Unlike the wall-clock `rule_engine` criterion bench, this measures exact
//! instruction / cache / branch counts via Valgrind — load-immune, so it is the
//! reproducible REGRESSION GATE that runs in CI on every PR (no quiet box
//! needed). Design: `docs/design/deterministic-perf-gating.md`.
//!
//! Gate: `Ir` (instruction count, +2%) and `EstimatedCycles` (net work + cache +
//! branch penalties, +5%) are hard-gated vs the committed baseline. Branch
//! mispredicts (`Bcm`/`Bim`) are diagnostic-only — collected + printed, but not
//! gated (they false-positive on benign branch-pattern shifts; the +2-3% v0.12
//! drift was a benign ~+7-23% mispredict delta at <1% net cycles).

use std::path::PathBuf;

use alint_core::{Engine, FileEntry, FileIndex, Rule};
use gungraun::{
    Callgrind, EventKind, LibraryBenchmarkConfig, library_benchmark, library_benchmark_group, main,
};

const CONFIG_YAML: &str = r#"
version: 1
rules:
  - id: rs-snake
    kind: filename_case
    paths: "**/*.rs"
    case: snake
    level: warning
  - id: tsx-pascal
    kind: filename_case
    paths: "**/*.tsx"
    case: pascal
    level: warning
  - id: no-bak
    kind: file_absent
    paths: "**/*.bak"
    level: error
  - id: readme
    kind: file_exists
    paths: "README.md"
    root_only: true
    level: warning
  - id: md-names
    kind: filename_regex
    paths: "**/*.md"
    pattern: "[a-zA-Z0-9_.-]+"
    level: info
  - id: no-huge
    kind: file_max_size
    paths: "**"
    max_bytes: 10485760
    level: warning
"#;

fn build_rules() -> Vec<Box<dyn Rule>> {
    let config = alint_dsl::parse(CONFIG_YAML).expect("bench config parses");
    let registry = alint_rules::builtin_registry();
    config
        .rules
        .iter()
        .map(|spec| registry.build(spec).expect("bench rule builds"))
        .collect()
}

fn build_index(n: usize) -> FileIndex {
    let mut entries = Vec::with_capacity(n + 1);
    entries.push(FileEntry {
        path: PathBuf::from("README.md").into(),
        is_dir: false,
        size: 2048,
    });
    for i in 0..n {
        let (path, size) = match i % 5 {
            0 => (format!("src/mod_{}/file_{i}.rs", i % 16), 1024u64),
            1 => (format!("components/Widget{i}.tsx"), 2048),
            2 => (format!("docs/page_{i}.md"), 512),
            3 => (format!("tests/test_{i}.rs"), 800),
            _ => (format!("misc/data_{i}.yaml"), 256),
        };
        entries.push(FileEntry {
            path: PathBuf::from(path).into(),
            is_dir: false,
            size,
        });
    }
    FileIndex::from_entries(entries)
}

struct Fixture {
    engine: Engine,
    index: FileIndex,
    root: PathBuf,
}

fn fixture(n: usize) -> Fixture {
    Fixture {
        engine: Engine::new(build_rules(), alint_rules::builtin_registry()),
        index: build_index(n),
        root: PathBuf::from("/bench/root"),
    }
}

#[library_benchmark(setup = fixture)]
#[bench::n1k(1_000)]
#[bench::n10k(10_000)]
fn engine_run(fix: Fixture) {
    // Destructure (consuming `fix`) so the gungraun-required by-value arg isn't
    // a `needless_pass_by_value` clippy warning.
    let Fixture {
        engine,
        index,
        root,
    } = fix;
    std::hint::black_box(engine.run(&root, &index).unwrap());
}

library_benchmark_group!(name = engine, benchmarks = engine_run);

main!(
    config = LibraryBenchmarkConfig::default().tool(
        Callgrind::default()
            .args(["--cache-sim=yes", "--branch-sim=yes"])
            // Gate on Ir (work, +2%) and EstimatedCycles (net work + cache +
            // branch penalties, +5%). Branch mispredicts (Bcm/Bim) are
            // diagnostic-only — reported always, but NOT gated: they false-positive
            // on benign branch-pattern shifts (v0.12's walker symlink-security
            // closure moved them +73..217% at <1% net cycles). See the design doc.
            .soft_limits([(EventKind::Ir, 2.0), (EventKind::EstimatedCycles, 5.0)]),
    ),
    library_benchmark_groups = engine
);
