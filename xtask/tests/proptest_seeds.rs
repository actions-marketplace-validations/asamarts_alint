//! Every committed proptest regression file must sit where proptest reads it.
//!
//! proptest saves a failing case's seed and replays it before generating new
//! cases, but only from the path it derives from the test's source file. With
//! the default persistence, `<crate>/src/a/b.rs` reads
//! `<crate>/proptest-regressions/a/b.txt`; an integration test pinned to
//! `WithSource("proptest-regressions")` reads `tests/<name>.proptest-regressions`.
//! When a test moves (a module split, a new `tests/main.rs`), its seeds stop
//! replaying and nothing says so. This test fails instead.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask has a workspace parent")
        .to_path_buf()
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn has_proptest(source: &Path) -> bool {
    std::fs::read_to_string(source).is_ok_and(|text| text.contains("proptest!"))
}

#[test]
fn every_committed_seed_file_belongs_to_a_proptest_source() {
    let root = workspace_root();
    let mut crates: Vec<PathBuf> = std::fs::read_dir(root.join("crates"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .collect();
    crates.push(root.join("xtask"));
    let mut problems = Vec::new();
    let mut seen = 0;
    for crate_dir in crates {
        let seeds = crate_dir.join("proptest-regressions");
        let mut files = Vec::new();
        files_under(&seeds, &mut files);
        for seed in files {
            seen += 1;
            let relative = seed.strip_prefix(&seeds).unwrap();
            let source = crate_dir.join("src").join(relative).with_extension("rs");
            if !has_proptest(&source) {
                problems.push(format!(
                    "{} is never replayed: {} has no proptest! (move the file next to where the \
                     test now lives)",
                    seed.strip_prefix(&root).unwrap().display(),
                    source.strip_prefix(&root).unwrap().display()
                ));
            }
        }
        let mut tests = Vec::new();
        files_under(&crate_dir.join("tests"), &mut tests);
        for seed in tests
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "proptest-regressions"))
        {
            seen += 1;
            let source = seed.with_extension("rs");
            let pinned = std::fs::read_to_string(&source).is_ok_and(|text| {
                let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                compact.contains("WithSource(\"proptest-regressions\"")
            });
            if !has_proptest(&source) || !pinned {
                problems.push(format!(
                    "{} is never replayed: {} must contain proptest! and pin \
                     FileFailurePersistence::WithSource(\"proptest-regressions\")",
                    seed.strip_prefix(&root).unwrap().display(),
                    source.strip_prefix(&root).unwrap().display()
                ));
            }
        }
    }
    assert!(
        seen > 0,
        "no committed seed files found; is the workspace root right?"
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
