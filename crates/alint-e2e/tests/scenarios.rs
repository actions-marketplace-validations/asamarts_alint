//! Versioned end-to-end policy tests. `dir-test` generates one Rust test per
//! document in the general corpus and the co-located bundled-ruleset corpus.

use std::path::Path;

use alint_testkit::{PolicyTest, Runner, RunnerOptions, TestStatus};
use dir_test::{Fixture, dir_test};

const EXPECTED_GENERAL_SCENARIOS: usize = 485;
const EXPECTED_BUNDLED_SCENARIOS: usize = 93;

fn scenario_paths(root: &Path) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
        {
            let path = entry
                .unwrap_or_else(|error| panic!("read entry below {}: {error}", directory.display()))
                .path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("yml") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
}

#[test]
fn all_578_migrated_documents_use_the_public_v1_parser() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let roots = [
        (
            manifest.join("scenarios"),
            EXPECTED_GENERAL_SCENARIOS,
            "general",
        ),
        (
            manifest.join("../alint-dsl/ruleset-tests/v1"),
            EXPECTED_BUNDLED_SCENARIOS,
            "bundled",
        ),
    ];
    let mut total = 0;
    for (root, expected, label) in roots {
        let paths = scenario_paths(&root);
        assert_eq!(
            paths.len(),
            expected,
            "{label} policy-test inventory changed; update the migration count deliberately"
        );
        total += paths.len();
        for path in paths {
            let yaml = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            let test = PolicyTest::from_yaml(&yaml)
                .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
            if let Err(diagnostics) = test.validate() {
                panic!("validate {}: {diagnostics:#?}", path.display());
            }
        }
    }
    assert_eq!(total, 578);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/scenarios",
    glob: "**/*.yml",
)]
#[allow(clippy::needless_pass_by_value)] // Required by dir-test's macro signature.
fn scenario(fixture: Fixture<&str>) {
    run_fixture(&fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../alint-dsl/ruleset-tests/v1",
    glob: "**/*.yml",
)]
#[allow(clippy::needless_pass_by_value)] // Required by dir-test's macro signature.
fn bundled_ruleset(fixture: Fixture<&str>) {
    run_fixture(&fixture);
}

fn run_fixture(fixture: &Fixture<&str>) {
    let path = fixture.path();
    let policy_test = PolicyTest::from_yaml(fixture.content())
        .unwrap_or_else(|error| panic!("policy test at {path}: parse error: {error}"));
    let runner = Runner::new(RunnerOptions {
        allow_spawn: true,
        allow_host_keyring: true,
        update_snapshots: false,
    });
    let run = runner
        .run(&policy_test, Path::new(path))
        .unwrap_or_else(|error| panic!("policy test {:?} at {path}: {error}", policy_test.name));
    if run.status == TestStatus::Skipped {
        eprintln!("skipping {path}: {:?}", run.mismatches);
        return;
    }
    assert!(
        run.passed(),
        "policy test {:?} at {path} mismatched: {:#?}",
        policy_test.name,
        run.mismatches
    );
}
