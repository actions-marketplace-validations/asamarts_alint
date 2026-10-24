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

use regex::Regex;
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

/// What a crate's `tests/` directory requires of `tests/main.rs`.
#[derive(Default)]
struct TestDir {
    /// Module name to its file, for every top-level `tests/*.rs` file other
    /// than `main.rs` and every `tests/<dir>/mod.rs` helper.
    modules: BTreeMap<String, String>,
    /// Files that can never be a module of `tests/main.rs` as they stand.
    problems: Vec<String>,
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn scan_tests_dir(tests_dir: &Path) -> TestDir {
    let mut found = TestDir::default();
    let entries = match std::fs::read_dir(tests_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return found,
        Err(error) => {
            found
                .problems
                .push(format!("cannot read {}: {error}", tests_dir.display()));
            return found;
        }
    };
    for entry in entries {
        let path = entry.expect("read tests dir entry").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let module = if path.is_dir() {
            if path.join("main.rs").is_file() {
                found.problems.push(format!(
                    "tests/{name}/main.rs is a separate test binary, which `autotests = false` \
                     never builds; rename it to tests/{name}/mod.rs and declare `mod {name};`"
                ));
            }
            if !path.join("mod.rs").is_file() {
                continue;
            }
            (name.clone(), format!("tests/{name}/mod.rs"))
        } else if let Some(stem) = name.strip_suffix(".rs")
            && stem != "main"
        {
            (stem.to_string(), format!("tests/{name}"))
        } else {
            continue;
        };
        if is_identifier(&module.0) {
            found.modules.insert(module.0, module.1);
        } else {
            found.problems.push(format!(
                "{} cannot be a module; rename it to a valid Rust identifier",
                module.1
            ));
        }
    }
    found
}

/// `src` without `//` and `/* */` comments, enough for a list of modules.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("//") {
            rest = after.find('\n').map_or("", |at| &after[at..]);
        } else if let Some(after) = rest.strip_prefix("/*") {
            rest = after.find("*/").map_or("", |at| &after[at + 2..]);
            out.push(' ');
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// The modules `main.rs` declares as `mod name;` (any visibility, attributes
/// on the same or earlier lines), and those declared with `#[path]`, which
/// would compile a file other than the one the module is named after.
fn declared_modules(main_rs: &str) -> (BTreeSet<String>, Vec<String>) {
    let declaration = Regex::new(
        r"((?:#\[[^\]]*\]\s*)*)(?:pub(?:\s*\([^)]*\))?\s+)?\bmod\s+(?:r#)?([A-Za-z_][A-Za-z0-9_]*)\s*;",
    )
    .unwrap();
    let path_attribute = Regex::new(r"#\[\s*path\b").unwrap();
    let mut declared = BTreeSet::new();
    let mut with_path = Vec::new();
    for capture in declaration.captures_iter(&strip_comments(main_rs)) {
        let name = capture[2].to_string();
        if path_attribute.is_match(&capture[1]) {
            with_path.push(name.clone());
        }
        declared.insert(name);
    }
    (declared, with_path)
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
        let found = scan_tests_dir(&tests_dir);
        problems.extend(
            found
                .problems
                .iter()
                .map(|problem| format!("{name}: {problem}")),
        );
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
        if found.modules.is_empty() && test_targets.is_empty() {
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

        let (declared, with_path) = declared_modules(&std::fs::read_to_string(&main_rs).unwrap());
        for module in with_path {
            problems.push(format!(
                "{name}: `mod {module};` in {} uses #[path]; declare each test file by its own \
                 name",
                main_rs.display()
            ));
        }
        for (module, file) in &found.modules {
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
fn declared_modules_reads_every_declaration_form() {
    let main_rs = "//! doc mentioning mod fake;\n\n#[cfg(unix)]\nmod lsp_common;\npub mod cli;\n\
                   mod a; // note\npub(crate) mod b;\n#[cfg(unix)] mod c;\nmod\td ;\nmod r#e;\n\
                   /* mod hidden;\n mod also_hidden; */\n#[path = \"other.rs\"]\nmod f;\n";
    let (declared, with_path) = declared_modules(main_rs);
    let expected = ["a", "b", "c", "cli", "d", "e", "f", "lsp_common"];
    assert_eq!(declared, expected.iter().map(ToString::to_string).collect());
    assert_eq!(with_path, ["f"]);
}

#[test]
fn scan_flags_files_that_cannot_be_modules() {
    let dir = tempfile::tempdir().unwrap();
    let tests = dir.path();
    std::fs::write(tests.join("main.rs"), "").unwrap();
    std::fs::write(tests.join("ok_file.rs"), "").unwrap();
    std::fs::write(tests.join("bad-name.rs"), "").unwrap();
    std::fs::create_dir_all(tests.join("helper")).unwrap();
    std::fs::write(tests.join("helper/mod.rs"), "").unwrap();
    std::fs::create_dir_all(tests.join("multi")).unwrap();
    std::fs::write(tests.join("multi/main.rs"), "").unwrap();
    std::fs::create_dir_all(tests.join("fixtures/src")).unwrap();
    std::fs::write(tests.join("fixtures/src/lib.rs"), "").unwrap();
    let found = scan_tests_dir(tests);
    assert_eq!(
        found.modules.keys().map(String::as_str).collect::<Vec<_>>(),
        ["helper", "ok_file"]
    );
    assert_eq!(found.problems.len(), 2, "{:?}", found.problems);
    assert!(found.problems.iter().any(|p| p.contains("bad-name.rs")));
    assert!(found.problems.iter().any(|p| p.contains("multi/main.rs")));
    assert_eq!(
        scan_tests_dir(&tests.join("absent")).problems,
        Vec::<String>::new()
    );
}
