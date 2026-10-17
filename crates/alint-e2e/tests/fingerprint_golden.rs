//! W6 baseline-identity gate: recompute every policy-finding fingerprint over the
//! complete scenario corpus and compare it with the checked-in golden file.
//!
//! The canonical corpus run is Linux-only because some scenarios intentionally
//! exercise Unix metadata, shell commands, and case-sensitive filenames. CI runs
//! this target on Linux. Refresh after an intentional fingerprint migration with:
//!
//! `UPDATE_SNAPSHOTS=1 cargo test -p alint-e2e --test fingerprint_golden`

#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use alint_core::baseline::fingerprint;
use alint_testkit::{CommitSpec, DetailedCommit, Scenario, Step, StepOutcome, run_scenario};
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
fn stabilize_git_dates(scenario: &mut Scenario) {
    let Some(git) = scenario.given.git.as_mut() else {
        return;
    };
    for commit in &mut git.commits {
        match commit {
            CommitSpec::Subject(message) => {
                *commit = CommitSpec::Detailed(DetailedCommit {
                    message: std::mem::take(message),
                    add: Vec::new(),
                    date: Some("2000-01-01T00:00:00Z".to_string()),
                });
            }
            CommitSpec::Detailed(commit) if commit.date.is_none() => {
                commit.date = Some("2000-01-01T00:00:00Z".to_string());
            }
            CommitSpec::Detailed(_) => {}
        }
    }
}

fn collect_scenario(scenarios_dir: &Path, path: &Path, records: &mut Vec<GoldenFingerprint>) {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read scenario {}: {error}", path.display()));
    let mut scenario: Scenario = serde_yaml_ng::from_str(&text)
        .unwrap_or_else(|error| panic!("parse scenario {}: {error}", path.display()));

    // Evaluate every corpus fixture from its declared initial tree, including
    // fix-only fixtures. This makes the gate cover the whole corpus rather than
    // only scenarios whose authored workflow happens to contain `check`.
    scenario.when = vec![Step::Check];
    // Range-rule baseline keys include commit SHAs. Pin otherwise-current fixture
    // timestamps so the same corpus produces the same commit graph on every run.
    stabilize_git_dates(&mut scenario);
    scenario.expect.clear();
    scenario.expect_tree = None;

    let run = run_scenario(&scenario)
        .unwrap_or_else(|error| panic!("run scenario {}: {error}", path.display()));
    let [StepOutcome::Check(report)] = run.steps.as_slice() else {
        panic!(
            "forced check did not return one check report: {}",
            path.display()
        );
    };
    let scenario_path = slash(
        path.strip_prefix(scenarios_dir)
            .expect("scenario path is below corpus root"),
    );
    let mut file_cache: BTreeMap<PathBuf, Option<Vec<u8>>> = BTreeMap::new();

    for result in &report.results {
        for violation in &result.violations {
            let bytes = violation.path.as_ref().and_then(|finding_path| {
                file_cache
                    .entry(finding_path.to_path_buf())
                    .or_insert_with(|| std::fs::read(run.root.join(finding_path)).ok())
                    .as_deref()
            });
            records.push(GoldenFingerprint {
                scenario: scenario_path.clone(),
                rule_id: result.rule_id.to_string(),
                path: violation.path.as_deref().map(slash),
                fingerprint: fingerprint(result.rule_id.as_ref(), violation, bytes),
            });
        }
    }
}

#[test]
fn scenario_fingerprints_match_golden() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scenarios_dir = manifest.join("scenarios");
    let mut records = Vec::new();
    for path in yml_files(&scenarios_dir) {
        collect_scenario(&scenarios_dir, &path, &mut records);
    }
    records.sort();
    assert!(
        records.len() >= 100,
        "fingerprint gate collected only {} findings; the scenario run is probably incomplete",
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
             UPDATE_SNAPSHOTS=1 cargo test -p alint-e2e --test fingerprint_golden",
            golden.display(),
        )
    });
    assert_eq!(
        actual, expected,
        "scenario fingerprints changed; related locations and presentation metadata MUST NOT alter \
         baseline identity. If this is an intentional baseline migration, review the complete diff \
         and refresh with UPDATE_SNAPSHOTS=1 cargo test -p alint-e2e --test fingerprint_golden",
    );
}
