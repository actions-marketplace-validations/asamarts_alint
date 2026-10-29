//! Fix-op spawn-gate drift audit (R-SPAWNGATE, part 1 of 2).
//!
//! A fixer that shells out is the file-writing/RCE analogue of a spawning rule
//! kind. Runtime trust decisions derive from `FixOpMeta::trust`; the former
//! [`alint_dsl::SPAWNING_FIX_OPS`] list remains a one-release equality
//! cross-check. This audit also scans fixer sources for subprocess primitives
//! and requires the actual modules to match the metadata-derived operation set.

use std::path::{Path, PathBuf};

/// `crates/alint-rules/src/fixers`, resolved relative to this test crate.
fn fixers_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ parent of alint-e2e")
        .join("alint-rules/src/fixers")
}

/// Does this source line reach a subprocess? A fixer spawns iff it constructs a
/// `Command` directly (`Command::new(` — also matches `StdCommand::new(` etc.),
/// calls the shared spawn chokepoint (`crate::spawn::<fn>(`), or touches
/// `alint_core::git` (whose readers and mutators all shell out to the `git`
/// binary — this is how `GitUntrackFixer` reaches `git rm --cached`). Line
/// comments are skipped so prose naming a primitive can't trip it.
///
/// Heuristic by design (a bespoke alias could evade it); it exists to catch the
/// realistic regression — a new fixer shelling out the ordinary way — not an
/// adversary editing alint's own source to hide a spawn from its own tests.
fn line_reaches_subprocess(line: &str) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    code.contains("Command::new(")
        || code.contains("crate::spawn::")
        || code.contains("alint_core::git::")
}

/// Every `.rs` under `fixers/` (recursively) that reaches a subprocess, as a
/// path relative to `fixers/` (e.g. `git_ops.rs`). Sorted.
fn spawning_fixer_modules() -> Vec<String> {
    let src = fixers_src_dir();
    let mut found = Vec::new();
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read fixers/ dir") {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read fixer source");
            if text.lines().any(line_reaches_subprocess) {
                let rel = path
                    .strip_prefix(&src)
                    .expect("under fixers/")
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push(rel);
            }
        }
    }
    found.sort();
    found
}

/// The documented `fix op -> fixer module` map for metadata-classified spawning
/// ops. A fix op's module name is arbitrary, so the mapping remains explicit.
const SPAWNING_FIX_OP_MODULES: &[(&str, &str)] =
    &[("git_untrack", "git_ops.rs"), ("command", "command_ops.rs")];

#[test]
fn spawning_fix_op_allowlist_matches_the_fixers_that_actually_spawn() {
    use std::collections::BTreeSet;

    // (1) Metadata is operational; the old list is only a one-release equality
    // cross-check. The explicit op-to-module map must equal the derived set.
    let registry = alint_rules::builtin_registry();
    let catalog = registry.catalog();
    let derived: BTreeSet<&str> = alint_core::FixSpec::ALL_OP_NAMES
        .iter()
        .copied()
        .filter(|op| {
            catalog
                .fix_op(op)
                .is_some_and(|meta| meta.trust == alint_core::FixTrustClass::Spawning)
        })
        .collect();
    let transition: BTreeSet<&str> = alint_dsl::SPAWNING_FIX_OPS.iter().copied().collect();
    assert_eq!(
        derived, transition,
        "FixOpMeta spawning classifications and the one-release SPAWNING_FIX_OPS list disagree"
    );
    let mapped_ops: BTreeSet<&str> = SPAWNING_FIX_OP_MODULES.iter().map(|(op, _)| *op).collect();
    assert_eq!(
        mapped_ops, derived,
        "\n\nFIX-OP SPAWN-GATE DRIFT — the op->module map does not match \
         FixOpMeta::trust == Spawning.\n  mapped ops : {mapped_ops:?}\n  derived ops: {derived:?}\n\
         Add or drop a row in SPAWNING_FIX_OP_MODULES to match metadata."
    );

    // (2) Modules that actually reach a subprocess must exactly match the
    // metadata-derived operation map. A new spawning fixer without metadata is
    // a security gap and makes `found` a superset.
    let found: BTreeSet<String> = spawning_fixer_modules().into_iter().collect();
    let expected: BTreeSet<String> = SPAWNING_FIX_OP_MODULES
        .iter()
        .map(|(_, module)| (*module).to_string())
        .collect();
    assert_eq!(
        found, expected,
        "\n\nFIX-OP SPAWN-GATE DRIFT — fixer modules launching a subprocess do not \
         match FixOpMeta spawning classifications.\n  modules that actually spawn : {found:?}\n\
         metadata-mapped modules      : {expected:?}\n\
         If a fixer now shells out, classify its op as Spawning in FixOpMeta and \
         add its module here; otherwise untrusted rulesets could execute code."
    );
}

#[test]
fn spawning_fix_op_map_is_non_empty_and_each_module_exists() {
    // A typo'd or emptied map is itself a security regression. Pin every mapped
    // module to a real fixer source.
    assert!(
        !SPAWNING_FIX_OP_MODULES.is_empty(),
        "SPAWNING_FIX_OP_MODULES is empty — the source audit would be blind"
    );
    let src = fixers_src_dir();
    for (op, module) in SPAWNING_FIX_OP_MODULES {
        let path = src.join(module);
        assert!(
            path.exists(),
            "SPAWNING_FIX_OP_MODULES maps {op:?} to {module:?}, but {} does not exist",
            path.display()
        );
    }
}

#[test]
fn subprocess_detection_is_precise() {
    // Guards the scan's own robustness: the real spawn reaches are caught, and
    // prose / a bare module reference is not a false positive.
    assert!(line_reaches_subprocess(
        "    use alint_core::git::{untrack_path, collect_tracked_paths};"
    ));
    assert!(line_reaches_subprocess(
        "    let mut cmd = Command::new(\"git\");"
    ));
    assert!(line_reaches_subprocess(
        "    crate::spawn::run_capturing(argv, cwd, &[], t)?;"
    ));
    // A line comment naming the primitive is ignored.
    assert!(!line_reaches_subprocess(
        "        // eventually calls Command::new( under the hood"
    ));
    // An unrelated fixer line is not a spawn.
    assert!(!line_reaches_subprocess(
        "        std::fs::set_permissions(&abs, perms)?;"
    ));
}
