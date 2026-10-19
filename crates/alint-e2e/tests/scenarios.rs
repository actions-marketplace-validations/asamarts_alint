//! Versioned end-to-end policy tests. `dir-test` generates one Rust test per
//! document in the general corpus and the co-located bundled-ruleset corpus.

use std::path::Path;

use alint_testkit::{PolicyTest, Runner, RunnerOptions, TestStatus};
use dir_test::{Fixture, dir_test};

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
