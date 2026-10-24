//! W6 baseline-identity gate: recompute every policy-finding fingerprint over the
//! complete scenario corpus and compare it with the checked-in golden file.
//!
//! The canonical corpus run is Linux-only because some scenarios intentionally
//! exercise Unix metadata, shell commands, and case-sensitive filenames. CI runs
//! this target on Linux. Refresh after an intentional fingerprint migration with:
//!
//! `UPDATE_SNAPSHOTS=1 cargo test -p alint-e2e --test it -- fingerprint_golden::`

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};

use alint_testkit::strategies::with_steps;
use alint_testkit::{PolicyTest, Runner, RunnerOptions, TestStep, TreeExpectationMode};
use serde::Serialize;

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Serialize)]
struct GoldenFingerprint {
    scenario: String,
    rule_id: String,
    path: Option<String>,
    fingerprint: String,
}

fn yml_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("read scenario directory {}: {error}", dir.display()))
        {
            let path = entry.expect("read scenario directory entry").path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("yml") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn collect_policy_test(corpus_root: &Path, path: &Path, records: &mut Vec<GoldenFingerprint>) {
    let yaml = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read policy test {}: {error}", path.display()));
    let policy_test = PolicyTest::from_yaml(&yaml)
        .unwrap_or_else(|error| panic!("parse policy test {}: {error}", path.display()));

    // Evaluate every corpus fixture from its declared initial tree, including
    // fix-only fixtures. The public runner supplies deterministic Git dates.
    let mut policy_test = with_steps(policy_test, vec![TestStep::Check]);
    policy_test.expect_tree = None;
    policy_test.expect_tree_mode = TreeExpectationMode::default();
    policy_test.snapshot = None;
    let runner = Runner::new(RunnerOptions {
        allow_spawn: true,
        allow_host_keyring: true,
        update_snapshots: false,
    });
    let run = runner
        .run(&policy_test, path)
        .unwrap_or_else(|error| panic!("run policy test {}: {error}", path.display()));
    let [observation] = run.observations.as_slice() else {
        panic!(
            "forced check did not return one observation: {}",
            path.display()
        );
    };
    let scenario_path = slash(
        path.strip_prefix(corpus_root)
            .expect("policy-test path is below corpus root"),
    );
    for violation in &observation.violations {
        records.push(GoldenFingerprint {
            scenario: scenario_path.clone(),
            rule_id: violation.rule.clone(),
            path: violation.path.clone(),
            fingerprint: violation.fingerprint.clone(),
        });
    }
}

#[test]
fn policy_test_fingerprints_match_golden() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scenarios_dir = manifest.join("scenarios");
    let ruleset_tests_dir = manifest
        .parent()
        .expect("crates directory")
        .join("alint-dsl/ruleset-tests/v1");
    let mut records = Vec::new();
    for path in yml_files(&scenarios_dir) {
        collect_policy_test(&scenarios_dir, &path, &mut records);
    }
    for path in yml_files(&ruleset_tests_dir) {
        collect_policy_test(&ruleset_tests_dir, &path, &mut records);
    }
    records.sort();
    assert!(
        records.len() >= 100,
        "fingerprint gate collected only {} findings; the policy-test run is probably incomplete",
        records.len(),
    );

    let mut actual = serde_json::to_string_pretty(&records).expect("serialize fingerprints");
    actual.push('\n');
    let golden = manifest.join("tests/snapshots/scenario-fingerprints.json");
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(golden.parent().expect("snapshot parent"))
            .expect("create snapshot directory");
        std::fs::write(&golden, &actual).expect("write fingerprint snapshot");
        eprintln!("wrote {}", golden.display());
        return;
    }

    let expected = std::fs::read_to_string(&golden).unwrap_or_else(|error| {
        panic!(
            "read fingerprint snapshot {}: {error}; regenerate with \
             UPDATE_SNAPSHOTS=1 cargo test -p alint-e2e --test it -- fingerprint_golden::",
            golden.display(),
        )
    });
    assert_eq!(
        actual, expected,
        "scenario fingerprints changed; related locations and presentation metadata MUST NOT alter \
         baseline identity. If this is an intentional baseline migration, review the complete diff \
         and refresh with UPDATE_SNAPSHOTS=1 cargo test -p alint-e2e --test it -- \
         fingerprint_golden::",
    );
}
