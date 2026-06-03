//! Integration tests for the three remaining shell-out rule
//! kinds. Each one needs a real environment to exercise (a git
//! repo with tracked files, a real commit, or a binary on PATH)
//! that unit tests intentionally don't stand up.
//!
//! Mirrors the `git_blame_age.rs` integration-test pattern:
//! tempdir + minimal real git repo + one-rule engine + assert
//! on violation set.
//!
//! Tests skip silently (with a stderr note) when `git` isn't on
//! PATH; that's the same convention as `git_blame_age.rs` and
//! keeps these integration tests portable across CI lanes that
//! don't carry git binaries.

use std::path::Path;
use std::process::Command;

use alint_core::{Engine, RuleEntry, WalkOptions, walk};
use alint_rules::builtin_registry;

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn run_git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_init(root: &Path) {
    run_git(root, &["init", "-q", "-b", "main"]);
    run_git(root, &["config", "user.name", "alint test"]);
    run_git(root, &["config", "user.email", "test@alint.test"]);
}

fn build_engine_from_yaml(yaml: &str) -> Engine {
    let spec: alint_core::RuleSpec = serde_yaml_ng::from_str(yaml).expect("rule spec parses");
    let registry = builtin_registry();
    let rule = registry.build(&spec).expect("rule builds");
    Engine::from_entries(vec![RuleEntry::new(rule)], registry)
}

fn run_engine(engine: &Engine, root: &Path) -> alint_core::Report {
    let index = walk(
        root,
        &WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        },
    )
    .unwrap();
    engine.run(root, &index).unwrap()
}

fn collect_violations(report: &alint_core::Report) -> Vec<&alint_core::Violation> {
    report
        .results
        .iter()
        .flat_map(|r| r.violations.iter())
        .collect()
}

// ─── git_no_denied_paths ────────────────────────────────────

#[test]
fn git_no_denied_paths_fires_on_tracked_secret() {
    if !git_available() {
        eprintln!("git unavailable; skipping git_no_denied_paths test");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    std::fs::write(root.join("README.md"), b"hi\n").unwrap();
    std::fs::write(root.join(".env"), b"SECRET=hunter2\n").unwrap();
    run_git(root, &["add", "README.md", ".env"]);
    run_git(root, &["commit", "-q", "-m", "init"]);

    let engine = build_engine_from_yaml(
        "id: no-secrets\n\
         kind: git_no_denied_paths\n\
         denied: [\"*.env\", \"id_rsa\"]\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(v.len(), 1, "expected one violation on .env: {v:?}");
    assert_eq!(v[0].path.as_deref(), Some(Path::new(".env")));
}

#[test]
fn git_no_denied_paths_silent_when_secrets_untracked() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    // .env exists in working tree but isn't tracked.
    std::fs::write(root.join("README.md"), b"hi\n").unwrap();
    std::fs::write(root.join(".env"), b"SECRET=hunter2\n").unwrap();
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-q", "-m", "init"]);

    let engine = build_engine_from_yaml(
        "id: no-secrets\n\
         kind: git_no_denied_paths\n\
         denied: [\"*.env\"]\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "untracked secret must not fire git_no_denied_paths"
    );
}

#[test]
fn git_no_denied_paths_silent_outside_git() {
    // No git_init: the rule must silently no-op when there's no
    // repo, like every other git-* rule.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".env"), b"x").unwrap();

    let engine = build_engine_from_yaml(
        "id: no-secrets\n\
         kind: git_no_denied_paths\n\
         denied: [\"*.env\"]\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "no-repo must not fire git_no_denied_paths"
    );
}

#[test]
fn git_no_denied_paths_since_scopes_to_diff() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    // Base commit tracks an old secret.
    std::fs::write(root.join("old.env"), b"OLD=1\n").unwrap();
    run_git(root, &["add", "old.env"]);
    run_git(root, &["commit", "-q", "-m", "base"]);
    let base = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    // A later commit adds a new secret.
    std::fs::write(root.join("new.env"), b"NEW=1\n").unwrap();
    run_git(root, &["add", "new.env"]);
    run_git(root, &["commit", "-q", "-m", "add new.env"]);

    // Both files are tracked and match `*.env`, but only new.env
    // changed since base → `since:` scopes the rule to the diff.
    let yaml = format!(
        "id: no-secrets\n\
         kind: git_no_denied_paths\n\
         denied: [\"*.env\"]\n\
         since: \"{base}\"\n\
         level: error\n"
    );
    let engine = build_engine_from_yaml(&yaml);
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(v.len(), 1, "since should scope to the diff: {v:?}");
    assert_eq!(v[0].path.as_deref(), Some(Path::new("new.env")));
}

// ─── git_commit_message ─────────────────────────────────────

#[test]
fn git_commit_message_fires_when_head_does_not_match() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    std::fs::write(root.join("README.md"), b"hi\n").unwrap();
    run_git(root, &["add", "README.md"]);
    // Plain message — no conventional-commit prefix.
    run_git(root, &["commit", "-q", "-m", "wip"]);

    let engine = build_engine_from_yaml(
        "id: conventional\n\
         kind: git_commit_message\n\
         pattern: \"^(feat|fix|chore): \"\n\
         level: warning\n",
    );
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(v.len(), 1, "expected one violation: {v:?}");
    assert!(
        v[0].message.contains("commit message")
            || v[0].message.contains("pattern")
            || v[0].message.contains("wip"),
        "violation message should reference the bad commit: {}",
        v[0].message
    );
}

#[test]
fn git_commit_message_silent_when_head_matches() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    std::fs::write(root.join("a.txt"), b"x\n").unwrap();
    run_git(root, &["add", "a.txt"]);
    run_git(root, &["commit", "-q", "-m", "feat: add thing"]);

    let engine = build_engine_from_yaml(
        "id: conventional\n\
         kind: git_commit_message\n\
         pattern: \"^(feat|fix|chore): \"\n\
         level: warning\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "feat: prefix must satisfy the pattern"
    );
}

#[test]
fn git_commit_message_silent_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let engine = build_engine_from_yaml(
        "id: conventional\n\
         kind: git_commit_message\n\
         pattern: \"^.*$\"\n\
         level: warning\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "no-repo must not fire git_commit_message"
    );
}

// ─── git_commit_signed_off ──────────────────────────────────

#[test]
fn git_commit_signed_off_fires_when_head_lacks_trailer() {
    if !git_available() {
        eprintln!("git unavailable; skipping git_commit_signed_off test");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    run_git(
        root,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "feat: no trailer here",
        ],
    );

    let engine = build_engine_from_yaml(
        "id: dco\n\
         kind: git_commit_signed_off\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(
        v.len(),
        1,
        "expected one violation on the un-signed commit: {v:?}"
    );
    assert!(
        v[0].message.contains("Signed-off-by"),
        "violation should mention the trailer: {}",
        v[0].message
    );
}

#[test]
fn git_commit_signed_off_silent_when_head_has_trailer() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    run_git(
        root,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "feat: thing\n\nSigned-off-by: alint test <test@alint.test>",
        ],
    );

    let engine = build_engine_from_yaml(
        "id: dco\n\
         kind: git_commit_signed_off\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "a signed-off commit must not fire"
    );
}

#[test]
fn git_commit_signed_off_silent_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = build_engine_from_yaml(
        "id: dco\n\
         kind: git_commit_signed_off\n\
         level: error\n",
    );
    let report = run_engine(&engine, tmp.path());
    assert!(
        collect_violations(&report).is_empty(),
        "no-repo must not fire git_commit_signed_off"
    );
}

// ─── git_commit_no_fixup ────────────────────────────────────

#[test]
fn git_commit_no_fixup_fires_on_leftover_fixup() {
    if !git_available() {
        eprintln!("git unavailable; skipping git_commit_no_fixup test");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    run_git(
        root,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "fixup! feat: original",
        ],
    );

    let engine = build_engine_from_yaml(
        "id: no-fixup\n\
         kind: git_commit_no_fixup\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(
        v.len(),
        1,
        "expected one violation on the fixup! commit: {v:?}"
    );
    assert!(
        v[0].message.contains("fixup") || v[0].message.contains("autosquash"),
        "violation should reference the fixup shape: {}",
        v[0].message
    );
}

#[test]
fn git_commit_no_fixup_silent_on_normal_commit() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    run_git(
        root,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "feat: a normal commit",
        ],
    );

    let engine = build_engine_from_yaml(
        "id: no-fixup\n\
         kind: git_commit_no_fixup\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "a normal commit must not fire git_commit_no_fixup"
    );
}

#[test]
fn git_commit_no_fixup_silent_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = build_engine_from_yaml(
        "id: no-fixup\n\
         kind: git_commit_no_fixup\n\
         level: error\n",
    );
    let report = run_engine(&engine, tmp.path());
    assert!(
        collect_violations(&report).is_empty(),
        "no-repo must not fire git_commit_no_fixup"
    );
}

// ─── git_commit_author_allowlist ────────────────────────────

#[test]
fn git_commit_author_allowlist_fires_on_outside_author() {
    if !git_available() {
        eprintln!("git unavailable; skipping git_commit_author_allowlist test");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root); // commits as test@alint.test
    run_git(
        root,
        &["commit", "-q", "--allow-empty", "-m", "feat: a change"],
    );

    let engine = build_engine_from_yaml(
        "id: org-authors\n\
         kind: git_commit_author_allowlist\n\
         email_pattern: '^.+@example\\.com$'\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(
        v.len(),
        1,
        "expected one violation on the outside author: {v:?}"
    );
    assert!(
        v[0].message.contains("allowlist") && v[0].message.contains("test@alint.test"),
        "violation should name the disallowed author: {}",
        v[0].message
    );
}

#[test]
fn git_commit_author_allowlist_silent_when_author_matches() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    run_git(
        root,
        &["commit", "-q", "--allow-empty", "-m", "feat: a change"],
    );

    // Pattern matches the harness author (test@alint.test).
    let engine = build_engine_from_yaml(
        "id: org-authors\n\
         kind: git_commit_author_allowlist\n\
         email_pattern: '^.+@alint\\.test$'\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "an allowed author must not fire"
    );
}

#[test]
fn git_commit_author_allowlist_silent_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = build_engine_from_yaml(
        "id: org-authors\n\
         kind: git_commit_author_allowlist\n\
         email_pattern: '^.+@example\\.com$'\n\
         level: error\n",
    );
    let report = run_engine(&engine, tmp.path());
    assert!(
        collect_violations(&report).is_empty(),
        "no-repo must not fire git_commit_author_allowlist"
    );
}

// ─── git_commit_gpg_signed ──────────────────────────────────

#[test]
fn git_commit_gpg_signed_fires_on_unsigned_commit() {
    if !git_available() {
        eprintln!("git unavailable; skipping git_commit_gpg_signed test");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    // git_init does not configure signing, so this commit is unsigned.
    run_git(
        root,
        &["commit", "-q", "--allow-empty", "-m", "feat: unsigned"],
    );

    let engine = build_engine_from_yaml(
        "id: signed-commits\n\
         kind: git_commit_gpg_signed\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(
        v.len(),
        1,
        "expected one violation on the unsigned commit: {v:?}"
    );
    assert!(
        v[0].message.contains("not signed") || v[0].message.contains("verify"),
        "violation should reference the missing signature: {}",
        v[0].message
    );
}

#[test]
fn git_commit_gpg_signed_silent_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = build_engine_from_yaml(
        "id: signed-commits\n\
         kind: git_commit_gpg_signed\n\
         level: error\n",
    );
    let report = run_engine(&engine, tmp.path());
    assert!(
        collect_violations(&report).is_empty(),
        "no-repo must not fire git_commit_gpg_signed"
    );
}

// ─── scope_filter.changed_since (engine end-to-end) ─────────

#[test]
fn changed_since_scopes_a_per_file_rule_to_the_diff() {
    if !git_available() {
        eprintln!("git unavailable; skipping changed_since test");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);

    // Base commit: old.rs (lacks the required header).
    std::fs::write(root.join("old.rs"), b"fn old() {}\n").unwrap();
    run_git(root, &["add", "old.rs"]);
    run_git(root, &["commit", "-q", "-m", "base"]);
    let base = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    // Later commit adds new.rs (also lacks the header). Only new.rs
    // is in `<base>...HEAD`.
    std::fs::write(root.join("new.rs"), b"fn new_thing() {}\n").unwrap();
    run_git(root, &["add", "new.rs"]);
    run_git(root, &["commit", "-q", "-m", "add new.rs"]);

    // file_content_matches fires when the file does NOT contain the
    // pattern; both files lack `// SPDX`, but changed_since scopes the
    // rule to the PR diff (new.rs only).
    let yaml = format!(
        "id: spdx-on-changed\n\
         kind: file_content_matches\n\
         paths: \"**/*.rs\"\n\
         pattern: \"^// SPDX\"\n\
         scope_filter:\n  changed_since: \"{base}\"\n\
         level: error\n"
    );
    let engine = build_engine_from_yaml(&yaml);
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(
        v.len(),
        1,
        "only the changed file should be in scope: {v:?}"
    );
    assert_eq!(
        v[0].path.as_deref(),
        Some(Path::new("new.rs")),
        "the violation must be on the file added since base"
    );
}

#[test]
fn changed_since_outside_git_matches_nothing() {
    // No repo: the diff resolves to nothing, so the rule silently
    // checks zero files (does not fire on every file).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("a.rs"), b"fn a() {}\n").unwrap();

    let engine = build_engine_from_yaml(
        "id: spdx-on-changed\n\
         kind: file_content_matches\n\
         paths: \"**/*.rs\"\n\
         pattern: \"^// SPDX\"\n\
         scope_filter:\n  changed_since: \"origin/main\"\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "outside a git repo, changed_since matches nothing"
    );
}

// ─── command ────────────────────────────────────────────────

// Linux-only because the macOS GitHub runner's `/bin/true`
// returns a non-success outcome here (root cause unconfirmed —
// possibly stripped userland or sandboxing). The same pathology
// is documented on the matching e2e scenario at
// `crates/alint-e2e/scenarios/check/plugin/command_pass_on_zero_exit.yml`
// (tagged `linux-only`).
#[cfg(target_os = "linux")]
#[test]
fn command_passes_when_wrapped_cli_exits_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.rs"), b"fn main() {}\n").unwrap();

    let engine = build_engine_from_yaml(
        "id: trivial-pass\n\
         kind: command\n\
         paths: \"src/**/*.rs\"\n\
         command: [\"/bin/true\"]\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "/bin/true must produce no violations"
    );
}

#[cfg(unix)]
#[test]
fn command_fires_when_wrapped_cli_exits_nonzero() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.rs"), b"fn main() {}\n").unwrap();

    let engine = build_engine_from_yaml(
        "id: always-fails\n\
         kind: command\n\
         paths: \"src/**/*.rs\"\n\
         command: [\"/bin/false\"]\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(v.len(), 1, "/bin/false must produce one violation: {v:?}");
    assert_eq!(v[0].path.as_deref(), Some(Path::new("src/a.rs")));
}

#[cfg(unix)]
#[test]
fn command_passes_alint_path_via_env() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), b"fn main() {}\n").unwrap();

    // Exits 0 only when ALINT_PATH was set to the relative file
    // path. Confirms the env-var bridge documented on the
    // `command` rule.
    let engine = build_engine_from_yaml(
        "id: env-check\n\
         kind: command\n\
         paths: \"src/**/*.rs\"\n\
         command:\n  \
           - /bin/sh\n  \
           - -c\n  \
           - '[ \"$ALINT_PATH\" = src/main.rs ] || exit 1'\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "ALINT_PATH should be set to the rel path"
    );
}

#[cfg(unix)]
#[test]
fn command_reports_spawn_failure_as_violation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.rs"), b"x").unwrap();

    let engine = build_engine_from_yaml(
        "id: missing-bin\n\
         kind: command\n\
         paths: \"src/**/*.rs\"\n\
         command: [\"/nonexistent/bin/zzzzz\"]\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(v.len(), 1, "missing bin must produce one violation");
    assert!(
        v[0].message.contains("spawn") || v[0].message.contains("PATH"),
        "spawn-failure message should reference the binary or PATH: {}",
        v[0].message
    );
}

// ─── changeset_requires_path ────────────────────────────────
//
// The e2e testkit's `git: { commits }` block makes *empty* commits,
// so a diff that *adds* files can't be expressed there. These native
// tests stand up a real two-commit repo and exercise the firing /
// silent / gated paths (the firing case is referenced from
// `coverage_audit_pass_fail`'s NATIVE_FIRES_ALLOWLIST).

fn git_base_commit(root: &Path) {
    std::fs::write(root.join("README.md"), b"# base\n").unwrap();
    run_git(root, &["add", "README.md"]);
    run_git(root, &["commit", "-q", "-m", "base"]);
}

const CHANGELOG_RULE: &str = "id: needs-changelog\n\
     kind: changeset_requires_path\n\
     add_glob: \".changeset/*.md\"\n\
     since: HEAD~1\n\
     level: error\n";

#[test]
fn changeset_requires_path_fires_when_no_matching_file_added() {
    if !git_available() {
        eprintln!("git unavailable; skipping changeset_requires_path test");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    git_base_commit(root);
    // Second commit changes src/ but adds no changeset entry.
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), b"code\n").unwrap();
    run_git(root, &["add", "src/lib.rs"]);
    run_git(root, &["commit", "-q", "-m", "feat: change"]);

    let engine = build_engine_from_yaml(CHANGELOG_RULE);
    let report = run_engine(&engine, root);
    let v = collect_violations(&report);
    assert_eq!(v.len(), 1, "no changeset entry added: {v:?}");
    assert!(v[0].message.contains(".changeset/*.md"), "{}", v[0].message);
}

#[test]
fn changeset_requires_path_silent_when_matching_file_added() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    git_base_commit(root);
    // Second commit adds a changeset entry alongside the change.
    std::fs::create_dir(root.join(".changeset")).unwrap();
    std::fs::write(root.join(".changeset/cool.md"), b"bump\n").unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), b"code\n").unwrap();
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-q", "-m", "feat + changeset"]);

    let engine = build_engine_from_yaml(CHANGELOG_RULE);
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "a changeset entry was added; rule must stay silent"
    );
}

#[test]
fn changeset_requires_path_when_changed_gates_the_requirement() {
    if !git_available() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git_init(root);
    git_base_commit(root);
    // A docs-only change, no changeset; the `when_changed: src/**`
    // gate is not met, so the requirement does not apply.
    std::fs::create_dir(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/guide.md"), b"docs\n").unwrap();
    run_git(root, &["add", "docs/guide.md"]);
    run_git(root, &["commit", "-q", "-m", "docs: tweak"]);

    let engine = build_engine_from_yaml(
        "id: needs-changelog\n\
         kind: changeset_requires_path\n\
         add_glob: \".changeset/*.md\"\n\
         when_changed: \"src/**\"\n\
         since: HEAD~1\n\
         level: error\n",
    );
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "src/ did not change; the changelog requirement must not apply"
    );
}

#[test]
fn changeset_requires_path_silent_outside_git() {
    // No git_init: the diff-scoped rule no-ops without a repo.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("README.md"), b"x").unwrap();
    let engine = build_engine_from_yaml(CHANGELOG_RULE);
    let report = run_engine(&engine, root);
    assert!(
        collect_violations(&report).is_empty(),
        "no repo: changeset_requires_path must no-op"
    );
}
