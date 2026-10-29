//! Verifies the publishable policy-test runner crate's packaged surface.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask has a workspace parent")
        .to_path_buf()
}

#[test]
fn testkit_package_contains_public_docs_without_repository_corpora() {
    let root = workspace_root();
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
        .args([
            "package",
            "-p",
            "alint-testkit",
            "--allow-dirty",
            "--no-verify",
            "--offline",
            "--list",
        ])
        .current_dir(&root)
        .output()
        .expect("run cargo package --list for alint-testkit");
    assert!(
        output.status.success(),
        "cargo package --list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let files: BTreeSet<_> = String::from_utf8(output.stdout)
        .expect("package list is UTF-8")
        .lines()
        .map(str::to_string)
        .collect();
    for required in [
        "Cargo.toml",
        "README.md",
        "TREE_SPEC.md",
        "src/lib.rs",
        "src/policy_test.rs",
        "src/policy_runner.rs",
    ] {
        assert!(files.contains(required), "package is missing {required}");
    }
    for path in &files {
        assert!(
            !path.contains("/scenarios/")
                && !path.starts_with("scenarios/")
                && !path.starts_with("target/")
                && !path.starts_with(".git/"),
            "repository-only content leaked into package: {path}"
        );
    }

    let manifest = std::fs::read_to_string(root.join("crates/alint-testkit/Cargo.toml")).unwrap();
    assert!(manifest.contains("license.workspace = true"));
    assert!(!manifest.contains("publish = false"));
    let readme = std::fs::read_to_string(root.join("crates/alint-testkit/README.md")).unwrap();
    for needle in [
        "policy-test.json",
        "policy-tests.md",
        "TREE_SPEC.md",
        "Apache-2.0 OR MIT",
    ] {
        assert!(readme.contains(needle), "README is missing {needle:?}");
    }
}
