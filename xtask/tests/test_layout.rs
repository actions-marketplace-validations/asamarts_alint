//! Guards the one-test-binary-per-crate layout
//! (`docs/design/build-dev-loop.md`, section 2).
//!
//! Every crate with integration tests sets `autotests = false` and builds them
//! as one `it` target rooted at `tests/main.rs`, which declares each test file
//! as a module. Without Cargo's discovery a new `tests/*.rs` file is silently
//! never compiled, and a crate that drops `autotests = false` goes back to one
//! binary per file; this test catches both through `cargo metadata`.
//!
//! Runs in plain `cargo test --workspace`, with no network.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask has a workspace parent")
        .to_path_buf()
}

fn workspace_packages(root: &Path) -> Vec<Value> {
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(root)
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).expect("metadata is JSON");
    metadata["packages"]
        .as_array()
        .expect("metadata lists packages")
        .clone()
}

/// The modules `tests/main.rs` must declare, each with its file: every
/// top-level `tests/*.rs` file other than `main.rs`, and every
/// `tests/<dir>/mod.rs` helper.
fn test_modules(tests_dir: &Path) -> BTreeMap<String, String> {
    let mut modules = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(tests_dir) else {
        return modules;
    };
    for entry in entries {
        let path = entry.expect("read tests dir entry").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if path.join("mod.rs").is_file() {
                modules.insert(name.clone(), format!("tests/{name}/mod.rs"));
            }
        } else if let Some(stem) = name.strip_suffix(".rs")
            && stem != "main"
        {
            modules.insert(stem.to_string(), format!("tests/{name}"));
        }
    }
    modules
}

/// Module names declared by `mod name;` lines. Attributes on earlier lines and
/// trailing comments are allowed.
fn declared_modules(main_rs: &str) -> BTreeSet<String> {
    main_rs
        .lines()
        .filter_map(|line| {
            let line = line.split("//").next().unwrap_or_default().trim();
            let rest = line.strip_prefix("pub ").unwrap_or(line);
            rest.strip_prefix("mod ")?
                .strip_suffix(';')
                .map(|name| name.trim().to_string())
        })
        .collect()
}

#[test]
fn every_crate_builds_its_integration_tests_as_one_binary() {
    let root = workspace_root();
    let mut problems = Vec::new();
    let mut checked = 0;
    for package in workspace_packages(&root) {
        let name = package["name"].as_str().unwrap();
        let manifest = PathBuf::from(package["manifest_path"].as_str().unwrap());
        let crate_dir = manifest.parent().unwrap();
        let tests_dir = crate_dir.join("tests");
        let expected = test_modules(&tests_dir);
        let test_targets: Vec<&Value> = package["targets"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|target| {
                target["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "test"))
            })
            .collect();
        if expected.is_empty() && test_targets.is_empty() {
            continue;
        }
        checked += 1;

        let main_rs = tests_dir.join("main.rs");
        let shape_ok = matches!(
            test_targets.as_slice(),
            [target] if target["name"] == "it"
                && Path::new(target["src_path"].as_str().unwrap()) == main_rs
        );
        if !shape_ok {
            let found: Vec<_> = test_targets
                .iter()
                .map(|target| target["name"].as_str().unwrap())
                .collect();
            problems.push(format!(
                "{name}: expected one test target `it` at tests/main.rs, found {found:?}; \
                 set `autotests = false` and declare it with [[test]] in {}",
                manifest.display()
            ));
            continue;
        }

        let declared = declared_modules(&std::fs::read_to_string(&main_rs).unwrap());
        for (module, file) in &expected {
            if !declared.contains(module) {
                problems.push(format!(
                    "{name}: {file} is never compiled; add `mod {module};` to {}",
                    main_rs.display()
                ));
            }
        }
    }
    assert!(
        checked >= 8,
        "only {checked} crates with integration tests were found"
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn declared_modules_reads_attributed_and_public_declarations() {
    let main_rs = "//! doc\n\n#[cfg(unix)]\nmod lsp_common;\npub mod cli;\nmod a; // note\n";
    let declared = declared_modules(main_rs);
    assert!(declared.contains("lsp_common"));
    assert!(declared.contains("cli"));
    assert!(declared.contains("a"));
    assert!(!declared.contains("doc"));
}
