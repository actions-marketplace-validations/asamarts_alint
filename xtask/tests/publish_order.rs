//! Guards the crates.io publish list/order in
//! `ci/scripts/publish-crates.sh` against the real workspace dependency
//! graph (via `cargo metadata`).
//!
//! Catches the v0.8.1 / v0.11 class of release-blocker: a workspace
//! dependency of the published `alint` binary that is **missing from
//! `CRATES`** (so `cargo publish -p alint` can't resolve it, mid-pipeline
//! and irreversibly), is `publish = false` (can't be a dep of a published
//! crate), or is listed **out of dependency order**.
//!
//! Runs in plain `cargo test --workspace` — no network, no publishing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// The workspace crate that actually ships to crates.io as a binary;
/// every one of its transitive workspace deps must be publishable and
/// listed before it.
const PUBLISHED_BINARY: &str = "alint";

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is .../xtask; the workspace root is its parent.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask has a parent dir")
        .to_path_buf()
}

fn cargo_metadata() -> Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(workspace_root())
        .output()
        .expect("run cargo metadata");
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("parse cargo metadata json")
}

/// Parse the ordered `CRATES=( ... )` array out of the publish script.
fn parse_crates_list() -> Vec<String> {
    let path = workspace_root().join("ci/scripts/publish-crates.sh");
    let script = std::fs::read_to_string(&path).expect("read publish-crates.sh");
    let mut crates = Vec::new();
    let mut in_block = false;
    for line in script.lines() {
        let trimmed = line.trim();
        if !in_block {
            if trimmed.starts_with("CRATES=(") {
                in_block = true;
            }
            continue;
        }
        if trimmed.starts_with(')') {
            break;
        }
        // Strip trailing comments; keep the bare crate name token.
        let token = trimmed.split('#').next().unwrap_or("").trim();
        if !token.is_empty() {
            crates.push(token.to_string());
        }
    }
    assert!(
        !crates.is_empty(),
        "could not parse a CRATES=( ... ) block from {}",
        path.display()
    );
    crates
}

#[test]
fn publish_list_covers_and_orders_the_published_binarys_workspace_deps() {
    let meta = cargo_metadata();
    let packages = meta["packages"].as_array().expect("metadata.packages");

    // With `--no-deps`, `packages` is exactly the workspace members.
    let members: BTreeSet<String> = packages
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect();

    // name -> publishable? (`publish: null` = yes; `[]` = publish=false;
    // a registry list = restricted, treated as not-public)
    let mut publishable: BTreeMap<String, bool> = BTreeMap::new();
    // Internal dep edges, split by whether they feed the "must publish"
    // closure (normal/build) vs. ordering-only (dev). cargo publish
    // validates dev-deps too, so they constrain order even though a
    // dev-only dep needn't be reachable from the binary.
    let mut closure_deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut all_deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for p in packages {
        let name = p["name"].as_str().unwrap().to_string();
        publishable.insert(name.clone(), p["publish"].is_null());
        let (mut closure, mut all) = (BTreeSet::new(), BTreeSet::new());
        for d in p["dependencies"].as_array().unwrap() {
            let dep = d["name"].as_str().unwrap();
            if !members.contains(dep) {
                continue; // external crate
            }
            all.insert(dep.to_string());
            // kind is null for normal, "dev" / "build" otherwise.
            let kind = d["kind"].as_str();
            if kind.is_none() || kind == Some("build") {
                closure.insert(dep.to_string());
            }
        }
        closure_deps.insert(name.clone(), closure);
        all_deps.insert(name, all);
    }

    // Transitive normal/build deps reachable from the published binary.
    let mut required: BTreeSet<String> = BTreeSet::new();
    let mut stack = vec![PUBLISHED_BINARY.to_string()];
    while let Some(crate_name) = stack.pop() {
        for dep in closure_deps.get(&crate_name).into_iter().flatten() {
            if required.insert(dep.clone()) {
                stack.push(dep.clone());
            }
        }
    }

    let crates = parse_crates_list();
    let index: BTreeMap<&str, usize> = crates
        .iter()
        .enumerate()
        .map(|(i, c)| (c.as_str(), i))
        .collect();

    // (a) every required dep must be in CRATES (the alint-lsp bug), and
    // (b) must itself be publishable.
    for dep in &required {
        assert!(
            index.contains_key(dep.as_str()),
            "`{dep}` is a workspace dependency of the published `{PUBLISHED_BINARY}` binary \
             but is missing from CRATES in ci/scripts/publish-crates.sh — \
             `cargo publish -p {PUBLISHED_BINARY}` would fail to resolve it on the tag. \
             Add `{dep}` to CRATES, before `{PUBLISHED_BINARY}`."
        );
        assert!(
            *publishable.get(dep).unwrap_or(&false),
            "`{dep}` is a dependency of the published `{PUBLISHED_BINARY}` but is \
             `publish = false`; a workspace dep of a published crate must be publishable."
        );
    }

    // The binary itself must be listed.
    assert!(
        index.contains_key(PUBLISHED_BINARY),
        "`{PUBLISHED_BINARY}` is missing from CRATES in publish-crates.sh"
    );

    // (c) ordering: every internal dep edge (any kind) among listed
    // crates must put the dependency before its dependent.
    for (pkg, deps) in &all_deps {
        let Some(&pkg_i) = index.get(pkg.as_str()) else {
            continue;
        };
        for dep in deps {
            if let Some(&dep_i) = index.get(dep.as_str()) {
                assert!(
                    dep_i < pkg_i,
                    "publish order in publish-crates.sh: `{dep}` must appear before `{pkg}` \
                     (it is a dependency of it)."
                );
            }
        }
    }

    // (d) no typos: every CRATES entry is a real workspace member.
    for c in &crates {
        assert!(
            members.contains(c),
            "CRATES lists `{c}`, which is not a workspace member (typo?)"
        );
    }
}
