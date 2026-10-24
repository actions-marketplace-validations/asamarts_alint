# Build and dev-loop hygiene

Status: Section 1 implemented in #356 (2026-10-24, unreleased); section 2 is backlog, not pinned to a release. Drafted 2026-10-21, rechecked at `61edcde97` on 2026-10-23.
Decisions: none
Roadmap: [`ROADMAP.md`](ROADMAP.md), "Future / backlog", "Build and dev-loop hygiene".

Two contributor-loop costs. Neither changes what alint ships; both make the edit, build, and
test cycle recompile or relink work it does not need to. They matter most on the shared dev box,
where several agents keep alint worktrees live next to the self-hosted CI runners and compete
for the same cores. Section 3 records a third candidate that was checked and needs no change.

Unless noted, numbers are compiler CPU-seconds measured on the dev box. The measurement
scripts and raw results are in a maintainer-local build-throughput study, outside this
repository (`~/tmp/rustbuild/PROPOSAL.md` section 3.3 and `~/tmp/rustbuild/bench/consolidate_tests.py`).
File counts and code references were rechecked at `61edcde97` (v0.20.1) on 2026-10-23.

**Precedent.** aplan, another Rust workspace on the same host, had both defects and fixed them
on 2026-10-21/22: A1-629 "Rebuild the build identity only when the checked-out commit can have
moved" and A1-630 "Merge each crate's integration tests into one test binary". They landed in
one batch with three smaller build tasks (A1-631 to A1-633: deny warnings in CI scripts without
changing `RUSTFLAGS`, bound the CI runner's debuginfo and target volume, reclaim leaked coverage
directories). aplan's compile load on the host then fell 65-88% over the next two days with
unchanged commit volume: 2,691 rustc-minutes on 10-21, 953 on 10-22, and 326 on 10-23 (about
20 hours), against 73-87 commits a day. That drop belongs to the batch, not to either change
alone; the per-change evidence is in sections 1 and 2.

## 1. Build-script rerun in git worktrees

### Problem

`crates/alint/build.rs` bakes the commit SHA and build date into `alint --version`. Line 20
tells cargo when to rerun it; this is unchanged at `61edcde97` (the file was last touched for
v0.9.17):

```rust
println!("cargo:rerun-if-changed=../../.git/HEAD");
```

- **Worktrees.** In a linked git worktree `.git` is a file (`gitdir: ...`), not a directory, so
  `../../.git/HEAD` does not exist (confirmed in a linked worktree of `61edcde97`). Cargo
  treats a missing `rerun-if-changed` path as stale, so the build script reruns on every build.
  The `alint` crate then recompiles and its 39 integration-test binaries relink, even when
  nothing changed.
- **Main checkout.** `.git/HEAD` is rewritten on every HEAD move (branch switch, rebase,
  commits in detached-HEAD state), and each move reruns the script and recompiles the crate.
  A commit on the checked-out branch updates `refs/heads/<branch>` but not `HEAD`, so
  `ALINT_GIT_SHA` in a dev build can lag the real commit anyway.

This has not been timed on alint itself. The identical pattern in aplan on the same host
(2026-10-18) printed `Dirty ...: the file .../.git/HEAD is missing` on a second, unchanged
`cargo check`, and an unchanged `cargo test --no-run` cost 99-103 CPU-s and 17-18 s wall each
time instead of roughly zero. After aplan's fix (A1-629, 2026-10-21) the same unchanged command
in a linked worktree reported both affected crates `Fresh` and finished its cargo phase in
0.82 s.

How often alint pays this depends on how many alint checkouts build. On 2026-10-23 only the
main checkout was building regularly, so the HEAD-move case is the common one today; the
worktree case hits every agent that starts a worktree, on every build there.

### Fix direction

Either option fixes the worktree case.

1. **Resolve the real git dir.** Ask git for `git rev-parse --git-dir` (this worktree's `HEAD`)
   and `--git-common-dir` (shared refs and `packed-refs`). Emit `rerun-if-changed` only for
   paths that exist: the worktree's `HEAD`, the ref file it points to, and `packed-refs`. A
   commit on the checked-out branch then refreshes the SHA, and commits on other branches do
   not rebuild. aplan chose this option and added three details worth copying: when the
   checked-out ref is packed, watch that ref's reflog plus `packed-refs`, never the whole
   `refs/heads` directory (which every worktree's commits touch); remove repository-local
   `GIT_*` variables from the git subprocess, so a hook that exports a relative `GIT_DIR`
   cannot redirect it; and test the result by driving real cargo freshness in small
   main-checkout and linked-worktree fixtures rather than by checking path strings.
2. **Read the revision only for release builds.** Compute the SHA and date when `PROFILE` is
   `release`, or when an explicit opt-in variable is set. Otherwise set them to `unknown` and
   emit only `rerun-if-changed=build.rs` plus `rerun-if-env-changed` for the opt-in variable.

Option 2 is smaller and also removes the HEAD-move rebuilds in the main checkout. Release
tooling must still produce a SHA-stamped `--version`. Option 1 keeps a dev build's
`--version` exact and still rebuilds only when the checked-out commit can have moved.

### Acceptance

- In a fresh linked worktree, after one `cargo test -p alint --no-run`, a second unchanged
  `cargo test -p alint --no-run` compiles nothing: no `Compiling alint` line, and
  `CARGO_LOG=cargo::core::compiler::fingerprint=info` reports no `Dirty alint`.
- In the main checkout, a commit on a different branch (or in another worktree) does not
  rebuild `alint`.
- A release build still prints the commit SHA and build date in `alint --version`. A build
  without git (a published tarball) still falls back to `unknown`.

### Resolution (#356)

Option 1, with aplan's three details. The script asks git for `--git-dir` and
`--git-common-dir` and watches only paths that exist: the checkout's `HEAD`, the loose ref it
points to, or for a packed ref that branch's reflog plus `packed-refs`, and the reftable table
list when that backend is in use. Its git subprocesses drop the repository-local `GIT_*`
variables. A linked worktree's hooks export an absolute `GIT_DIR`, so without this a hook in one
checkout that builds another would bake the first checkout's commit. (Plain commit and pre-push
hooks in the main checkout export no `GIT_DIR`; commit hooks export a relative
`GIT_INDEX_FILE`, which `rev-parse` ignores.)

`ci/scripts/test-build-script-freshness.sh` drives a fixture crate whose build script is a copy
of alint's through 20 cases in real checkouts. It covers this section's acceptance criteria
plus packed refs, a detached HEAD, an inherited `GIT_DIR` and a build without git. The original
script fails 8 of them. Two defects beyond the worktree case surfaced. Outside git, the missing
path made every build rerun. In the main checkout, a commit on the checked-out branch did not
rerun the script, so a dev build's SHA lagged.

Measured on the real crate in a linked worktree at `6a75eb4d1`: before the fix, an unchanged
second `cargo build -p alint` reported `Dirty alint: the file crates/alint/../../.git/HEAD is
missing`. After it, an unchanged second `cargo test -p alint --no-run` compiled nothing and
finished in 0.14 s.

### Effort

About 0.5 day, including checking the release workflow's `--version` output.

## 2. One integration-test binary per crate

### Problem

No crate sets `autotests = false`, so every top-level `tests/*.rs` file is its own test binary.
Each binary links the crate under test and its dependency graph again, so one edit to a low
crate (for example `alint-core`) relinks dozens of executables.

At `61edcde97` there are 87 top-level `tests/*.rs` files: alint 39, alint-e2e 33, alint-rules
6, alint-core 2, alint-dsl 2, alint-output 2, xtask 2, alint-testkit 1. (There were 86 at
`165732f16` on 2026-10-21; v0.20.1 added `crates/alint/tests/pattern_hints_cli.rs`.) Shared
helpers live in `crates/alint/tests/lsp_common/` (pulled in by `mod lsp_common;` in
`lsp_audit.rs`, `lsp_protocol.rs`, and `lsp_smoke.rs`) and `crates/alint-e2e/tests/support/`.

The main checkout's `target/` reached 299 GB on 2026-10-16 (deps 196 GB in 26,874 files,
incremental 88 GB) and 263 GB on 2026-10-17. Since 2026-10-16 a host-side artifact GC evicts any
target dir over 60 GB that has been idle for 2 hours (it evicted this one on 2026-10-18); on
2026-10-23 it held 18 GB.

### Measurement (2026-10-14, `635b6a91`)

A prototype generated one `tests/it/main.rs` per crate that includes every former `tests/*.rs`
via `#[path]` modules, without moving any test source. The only source edit turned
`mod lsp_common;` in the 3 LSP test files into `use crate::lsp_common;`. Integration-test
binaries went from 77 to 7, and all test executables (unit-test harnesses included) from 87
to 17.

| Scenario | Original | One binary per crate | Change |
|---|---:|---:|---:|
| Edit `alint-core`, `cargo test --no-run`, full debuginfo, CPU-s | 158 | 50 | -69% |
| Same, peak memory | 9.8 GiB | 5.4 GiB | -45% |
| Edit `alint-core`, `cargo test --no-run`, trimmed debuginfo, CPU-s | 67 | 33 | -51% |
| Clean test build, full debuginfo, CPU-s | 736 | 444 | -40% |
| Clean test build, trimmed debuginfo, CPU-s | 433 | 350 | -19% |
| Target dir, full / trimmed debuginfo | 13 / 5.3 GB | 6.7 / 3.3 GB | -48% / -38% |
| `cargo nextest run --workspace` | 3,303 tests, 5 failed | 3,303 tests, 1 failed | no new failures |

The single failure in the consolidated run
(`alint-core git::tests::commit_range_rejects_dash_since_and_writes_no_file`) also fails in
the original layout; it is specific to that environment.

### Precedent (aplan A1-630, 2026-10-21)

aplan made the same change: `autotests = false` and one `[[test]]` target per crate, with the
files kept in place behind `#[path]` modules. Integration-test binaries went from 134 to 9, and
all 1,151 tests were kept under a module-name prefix. Its in-tree measurement on the loaded
shared host (touch `aplan-core`, then `test --workspace --no-run`) was smaller than its isolated
prototype's -76%: aggregate CPU -20%, system CPU -60%, wall -12%. The change surfaced one real
process-boundary dependency: a test helper installed a process-wide recording sink and never
cleared it, which was safe only while its file was its own executable. It became a scoped
thread-local override.

### Fix direction

- For each crate with integration tests, add `autotests = false` to `[package]` and one
  `[[test]]` target (for example `name = "it"`, `path = "tests/it/main.rs"`) whose `main.rs`
  declares one module per former file.
- Either move the files under `tests/it/` or keep them in place and include them with
  `#[path]`, as the prototype and aplan did. Moving them breaks the relative `include_str!`
  paths in `explain_trace.rs`, `default_options_snapshot.rs`, `schema.rs`,
  `fix_report_schema.rs`, and `cross_formatter.rs`, so a move must update those too.
- Declare `mod lsp_common;` once in the alint crate's `main.rs`, and make the 3 LSP test files
  `use crate::lsp_common;`. The 10 alint-e2e files that declare `mod support;` worked unchanged
  in the prototype. Hoisting that declaration into `main.rs` the same way compiles the helper
  once instead of ten times.
- Update every `--test <name>` reference to the new target plus a name filter (for example
  `cargo test -p alint --test it -- cli::`): the doc comments in `crates/alint/tests/cli.rs`
  and `crates/alint-e2e/tests/fingerprint_golden.rs` (including its failure messages),
  `crates/alint-e2e/fixtures/smoke/README.md`, `docs/benchmarks/RUNNING.md`, and
  `docs/development/rule-authoring.md`. Under nextest, select with
  `-E 'binary(it) & test(/^cli::/)'`.
- Risk: tests that touch process-global state (environment variables, the working directory,
  global loggers, process-wide hooks) now share one process under `cargo test`. nextest still
  runs each test in its own process. At `61edcde97` a grep of the test trees finds no
  `set_var`, `remove_var`, `set_current_dir`, or global subscriber `init()` calls (one comment
  in `interp_integration.rs` mentions `set_var`). The only process-wide static is the read-only
  rule registry in `crates/alint-e2e/tests/support/mod.rs`, which a merged binary shares
  harmlessly, and `pty_color.rs` opens its own pty per test. aplan's case shows that a grep
  can miss a global that is set once and never cleared, so run `cargo test --workspace`
  several times after the change.

### Acceptance

- `cargo test --workspace --no-run` builds exactly one integration-test executable per crate
  that has integration tests, and no crate's `tests/` directory produces a second one.
- `cargo nextest run --workspace` runs the same number of tests as before the change, with the
  same pass and fail set.
- `cargo test --workspace` (shared process) passes, repeatedly.
- No `--test <old-name>` reference remains in the repository.
- The PR records `cargo test --no-run` CPU time after editing `alint-core`, before and after.

### Effort

About 1 day: a mechanical move, the reference updates, and one before/after measurement.

## 3. Checked, nothing to file: `RUSTFLAGS` in CI scripts

Several Rust repositories on the host export `RUSTFLAGS="-D warnings"` from their
`ci/scripts/*.sh`. A different `RUSTFLAGS` is a different artifact set, so running such a
script locally builds a second full copy of every crate next to plain `cargo` (aplan replaced
it with `CARGO_BUILD_WARNINGS=deny` in A1-631). alint does not have this: at `61edcde97` no
script under `ci/scripts/` sets `RUSTFLAGS`, and `audit.sh` and `deny.sh` only clear it for
their `cargo install` of the tool. `-D warnings` comes from the workflow-level `env:` in
`ci.yml`, `coverage.yml`, `release.yml`, and `cross-platform.yml`, so it applies uniformly on
CI, and the pre-push hook's `ci/scripts/preflight.sh` reuses a contributor's plain `cargo`
artifacts. Clippy and rustdoc take `-D warnings` on their own command lines (`clippy.sh`, and
`RUSTDOCFLAGS` in `docs.sh`), which leaves rustc's flags alone.
