# v0.17 → v0.20 benchmark increases: a harness artifact, gate blind spots, and real per-kind regressions

Status: diagnosed; fixes proposed and prototyped, not yet applied. The v0.20.0 bench PR (#341) passes every gate, but its 1k `changed` cells (+62% / +49%) and several micro benches (+10% to +84% since v0.17.0) looked alarming. Each increase was traced to its cause with load-immune measurements. One is a benchmark-harness artifact. Several are real regressions in specific rule kinds, config loading, and output formatting that accumulated over v0.18 to v0.20 in steps too small for any gate. Two measurement gaps let them through. Large-repo `alint check` performance has **not** regressed: macro `full` S1 to S3 at 100k and 1m are within ±3% from v0.16.0 to v0.20.0.

## Summary

| Signal | Verdict | Root cause | Proposed fix (measured) |
|---|---|---|---|
| Macro S1/S2 1k `changed` jump between ~11 and ~20 ms from run to run | **Harness artifact** | git "racily clean" index entries: tree generated and committed within one wall-clock second, so `git ls-files --modified` re-hashes up to every file on every timed run | Settle the index after commit (§1). Verified: 0 re-reads on 16 of 16 fresh trees, previously 88 to 901 |
| `det_engine` engine_run (deterministic gate) | **Gate blind spot** | Rule evaluation runs on rayon workers, which gungraun does not collect; the bench counts ~0.06% of the engine's work, most of what it reports is fixture teardown | `RAYON_NUM_THREADS=1` plus `--toggle-collect` on rayon job execution (§2.1). Verified: 99.9% of work, 0.01% repeat spread |
| Micro creep across three releases, no gate fired | **Process gap** | Micro suite is ungated; `bench-gate` compares adjacent releases only; the det gate runs on PRs only (28 of 29 v0.20 commits went to main directly) | Deterministic micro gate, cumulative-drift check, det gate on main (§2.2 to §2.4) |
| `dsl_extends/chain_depth` +84% Ir | **Real** (config load) | v0.18 memo deep-clones every loaded config; YAML DoS pre-scans; v0.19/v0.20 trust, provenance and compatibility passes | Two-touch memo + YAML prefilter: -19% (§3.1) |
| `output_formats` github +27%, human +14%, markdown +9.5% Ir | **Real** | Per-character `is_deceptive_on_display` call (v0.18.2), not inlined even under thin LTO | Printable-ASCII fast path: -27% / -16% / -12%, now **below v0.17.0** (§3.2) |
| `every_matching_has` +14.5%, `for_each_dir` +7.7% Ir | **Real** | v0.18 glob-escaping allocations and re-parse; v0.19 re-validates each nested instantiation | Allocation-free fast paths + prevalidated build: -7.2% / -5.4% (§3.3) |
| `pair` +17.7% Ir | **Real** | v0.18 `normalize_rel` always allocates; v0.19 per-violation related location | Fast path: -5.0% (§3.4) |
| `structured_query yaml_path_matches` +19% Ir | **Real** | v0.18 YAML DoS guard: two extra full lexer passes per file | Byte prefilter: -14.6%, back to v0.17.0 (§3.5) |
| `every_matching_has/1000` +14% wall-clock **at v0.20.0** | **Not code** | Ir -0.1%, simulated cycles +0.5% vs v0.19.0 | None; cross-run single-sample noise (§3.7) |

Prototype patch (9 files, +101/-9) for every fix above passes `alint-core` (791), `alint-output` (128), `alint-rules` (1150), `alint-dsl` (261) and `xtask` (160) tests, `cargo clippy --all-targets -D warnings`, and `cargo fmt --check`. Measured end to end in §4.

## 1. The 1k `changed` cells: git's racy-clean check, not alint

### Symptom

`min_ms` of the 1k `changed` cells on the canonical host (i7-6700HQ, `bench-1`):

| Version | S1 1k changed | S2 1k changed | S1 1k full | Mode |
|---|---:|---:|---:|---|
| v0.16.0 | 18.4 | 21.4 | 8.9 | slow |
| v0.17.0 | 10.4 | 14.0 | 8.2 | fast |
| v0.18.0 | 12.4 | 14.7 | 8.7 | fast |
| v0.18.1 | 19.3 | 21.1 | 8.2 | slow |
| v0.18.2 | 18.5 | 21.7 | 8.1 | slow |
| v0.18.4 | 19.8 | 21.9 | 9.3 | slow |
| v0.19.0 | 11.9 | 14.3 | 9.4 | fast |
| v0.20.0 | 19.3 | 21.2 | 8.7 | slow |

All 10 samples of a cell sit in the same mode (v0.19.0 S1: 11.9 to 16.5 ms; v0.20.0 S1: 19.3 to 21.9 ms). S1 and S2 share one generated tree and always flip together; `full` cells do not move. Code does not explain it: v0.16.0/v0.17.0 have identical `collect_changed_paths`, as do v0.19.0/v0.20.0, yet each pair straddles both modes.

### Mechanism

`alint check --changed` runs `git ls-files --modified --others --exclude-standard`. Git (built without `USE_NSEC`, its default) treats any index entry whose mtime falls in the same wall-clock **second** as `.git/index` as *racily clean*: the stat data cannot prove the content unchanged, so `ie_match_stat` re-reads and re-hashes the file. `ls-files` never rewrites the index, so the same entries are re-hashed on every invocation, forever.

The harness generates the 1k tree, then runs `git add -A` and `git commit` in roughly 0.2 to 0.3 s. Whether a second boundary falls inside that window is luck, so a run gets anywhere from none to all of the tree racy, and keeps that state for every timed sample.

Evidence, from `strace` of the git child processes and `git ls-files --debug` on fresh trees generated by the real harness (`xtask bench-scale --sizes 1k --scenarios S1 --modes changed`):

| Tree | Tracked files with mtime second == index second | Files git re-reads per run | After `sleep 1.1 && git status` |
|---|---:|---:|---:|
| 1 to 4 | 0 | 0 | 0 |
| 5 | 1001 | 901 | 0 |
| 6 | 717 | 638 | 0 |
| 7 | 313 | 280 | 0 |
| 8 | 372 | 331 | 0 |

Re-reads equal the same-second count minus the touched 10% (those changed size, which git detects without reading). The files re-read have index stat data byte-identical to the filesystem; only the same-second rule triggers the read.

Cost, as Callgrind instruction counts of the git process alone: a settled index costs 3.39 M; 126 racy entries 8.72 M; 209 racy 12.50 M; 901 racy **43.30 M** (~44 k per racy file: open, read, collision-detecting SHA-1, attributes lookup). 40 M extra instructions is about 7 ms on the bench host, the observed slow-mode penalty.

10k trees are not affected: `git add` of 10k files takes more than a second, so the files are always older than the index second (0 re-reads on 5 of 5 fresh 10k trees). The 2026-09 10k swing ([`2026-09-v0.16-changed-mode-bench-artifact`](../2026-09-v0.16-changed-mode-bench-artifact/)) is a different mechanism; this finding does not change that write-up.

Real-world `--changed` use is not affected the same way: a working index is almost never written within the same second as the files it tracks, and any `git status` rewrites it.

### Fix (harness)

In `xtask/src/bench/run.rs`, settle the index once per tree after the base commit (and for the S4 tree, which is committed during generation), before the subset is touched:

```rust
fn init_git_for_changed_mode(root: &Path) -> Result<()> {
    git(root, &["init", "-q", "-b", "main"])?;
    git(root, &["config", "gc.auto", "0"])?;
    if !has_initial_commit(root) {
        commit_bench_base(root)?;
    }
    settle_racy_index(root)
}

/// Git (built without `USE_NSEC`, its default) treats a tracked file whose
/// mtime falls in the same wall-clock second as `.git/index` as racily clean
/// and re-hashes its content on every `ls-files --modified`, which never
/// rewrites the index to clear it. ... Wait out the second, then let
/// `git status` rewrite the index.
fn settle_racy_index(root: &Path) -> Result<()> {
    std::thread::sleep(std::time::Duration::from_millis(1100));
    git(root, &["status", "--porcelain", "--untracked-files=no"])
}
```

`git status` rewrites the index whenever it holds racy entries (`repo_update_index_if_able` → `has_racy_timestamp`). Verified on 16 fresh trees (8 S1, 8 S4): 0 re-reads in every one, changed set unchanged (100 and 101 files). Cost: about 1.1 s per tree per size, negligible against a multi-hour run.

Follow-ups: the `METHODOLOGY.md` small-`changed` caveat should name this mechanism for 1k. After two clean runs with the fix, the 1k `changed` cells could become gating again.

## 2. Why no gate caught the real regressions

### 2.1 `det_engine` engine_run measures almost none of the engine

gungraun collects the benchmark function's own thread by default. `Engine::run` hands rule evaluation to a cached rayon pool via `pool.install`, so the work runs on worker threads that are never collected. The v0.20.0 `det_engine` run reports `engine_run n10k` = 2,245,079 Ir. The nearly identical criterion fixture (`rule_engine/10000`, one extra rule), with all threads counted, is **35,976,198** Ir per iteration. Collecting the same scope `det_engine` uses (toggle on `Engine::run`, default threads) counts **20,684** Ir per iteration, 0.06% of the work. Most of the 2.25 M that `det_engine` does report is dropping the 10k-entry fixture, which the benchmark function consumes by value.

Fix, verified with raw Callgrind on the equivalent fixture: run the bench with `RAYON_NUM_THREADS=1` (the cached pool honors it) and add worker-side toggles, as gungraun's docs prescribe for threaded code:

```rust
Callgrind::default().args([
    "--cache-sim=yes",
    "--branch-sim=yes",
    "--toggle-collect=*StackJob*as rayon_core::job::Job*::execute*",
    "--toggle-collect=*HeapJob*as rayon_core::job::Job*::execute*",
])
// plus RAYON_NUM_THREADS=1 in the benchmark environment
```

Result: 35,949,721 and 35,953,099 Ir per iteration on two repeats (99.9% of the full count, 0.01% apart). With more worker threads the toggles capture ~90% and become schedule-dependent, so pin to one thread. The committed baselines jump about 16x when this lands, so land it as a deliberate re-baseline.

`det_check` (whole-process binary benches) is not affected: it collects every thread.

### 2.2 The micro suite has no gate and no same-session baseline

`bench-record` runs the criterion micro suite once per release with default settings and publishes the snapshot. Nothing compares it to anything; `base/` in the snapshot is a copy of `new/`. Cross-run wall-clock comparisons on a single sample per release cannot separate a 10% step from run-to-run noise (§3.7 shows a 14% step with zero code change), so even reading the history by eye does not work.

### 2.3 Release-to-release comparison cannot see creep

`bench-gate` compares each release only with its predecessor at a +15% threshold. Config loading grew +41%, +16%, +13% over three releases (Ir); output formatting +6% then +18%; nested-rule kinds +9.5% then +4.0%. Most steps sit inside the wall-clock noise of a single micro sample, and none ran through a gate.

### 2.4 The deterministic gate never saw v0.20's engine changes

`ci/scripts/det-perf-gate.sh` runs on pull requests only. 28 of the 29 v0.19.0..v0.20.0 commits were pushed to `main` directly, so none of their engine changes was checked before release.

## 3. The real regressions

### Method

Each release's own, unmodified criterion bench sources were compiled against a counting stand-in for the `criterion` crate. It runs exactly K iterations of one named benchmark, and the build uses the same rustc for every version. Each benchmark ran under Callgrind at K = 1 and K = 11; `(Ir(11) - Ir(1)) / 10` is the exact per-iteration instruction count, with setup excluded and every thread counted (`RAYON_NUM_THREADS=1`). Repeats agree within 0.1 to 3.5% for the in-memory benches. Benches that create real files and walk them in setup are noisier; those were re-measured with K up to 2001. Per-function self-cost diffs between versions attribute each step. Raw numbers for all 98 benches at v0.17.0, v0.18.0, v0.18.4, v0.19.0 and v0.20.0 are in [`callgrind-sweep.tsv`](callgrind-sweep.tsv). The regressions also hold under release-equivalent codegen (thin LTO, one codegen unit): config load +84.2%, github output +28.9%, `pair` +17.7%, `every_matching_has` +14.8%.

Absolute scale on the bench host (wall-clock micro, v0.17.0 → v0.20.0): config load at depth 1 is 39 → 56 µs; github output for 10k violations 13.4 → 16.2 ms; `every_matching_has` over 5000 packages 28.1 → 31.5 ms; `pair` over 10k files 11.7 → 14.5 ms. None of these shows up in the macro scenarios, which are dominated by walking and content scanning.

### 3.1 Config loading (`dsl_extends`): +84% at depth 16, +54% at depth 1

Inclusive cost at depth 16 (Ir per load):

| Component | v0.17.0 | v0.18.0 | v0.19.0 | v0.20.0 |
|---|---:|---:|---:|---:|
| `load_with` total | 831,635 | 1,169,055 | 1,355,862 | 1,526,655 |
| YAML parse + deserialize | ~492 k | ~500 k | ~498 k | ~506 k |
| Memo: `RawConfig` clone on insert | 0 | 66,143 | 102,682 | 138,199 |
| Memo: insertion (hashing, table growth) | 0 | 64,002 | 63,974 | 72,547 |
| Memo: teardown of the retained clones | 0 | 46,182 | 76,567 | 100,326 |
| YAML flow-depth + alias pre-scans | 19,402 | 112,043 | 111,922 | 111,990 |
| Trust and provenance passes | 0 | 38,535 | 151,137 | 152,238 |
| `RawConfig::finalize` | 0 | 15,198 | 28,498 | 31,462 |
| `merge` (incl. `extend_compatibility`) + path canonicalization | 141,885 | 147,919 | 147,433 | 192,143 |

Rows are non-overlapping inclusive costs. The v0.20 YAML library swap (serde_yaml_ng → yaml_serde / libyaml-rs) is cost-neutral. The memo added in v0.18 to stop diamond `extends:` from exploding inserts a deep clone of every non-top-level config. That happens even in a linear chain where nothing is ever looked up again, and it costs about 311 k of the 1.53 M at v0.20.0.

Fixes:

- **Two-touch memo** (`alint-dsl/src/loader.rs`): store `Option<RawConfig>`. A key's first completed load only records it as seen; the second load stores the clone; later visits hit. A diamond still loads each shared file at most twice, so the bound stays linear, and the existing 2^depth diamond regression test passes. A linear chain never clones.
- **YAML guard byte prefilter** (`alint-core/src/yaml_depth.rs`): a flow collection needs a `[` or `{` byte and an alias needs a `*` byte, so text without them skips the corresponding lexer pass. Sound by construction; the guard is unchanged for any text that could trip it.

Measured: depth 16 -19.0%, depth 4 -15.2%, depth 1 -9.2%. At depth 16 that is 291 k Ir: about 238 k from the memo (clone and teardown; the seen-once insertion remains) and 53 k from the prefilter. The remaining +40 to +49% over v0.17.0 is the v0.19/v0.20 trust, provenance and compatibility work. Moving trust marking into deserialization is a possible follow-up, but at about 40 µs per typical load it is low priority.

### 3.2 Output formatting: per-character deceptive-text check

`sanitize_terminal` (v0.18.0) and the shared `alint_core::invisible::is_deceptive_on_display` (v0.18.2, #318) run per character over every message, path and id. `is_deceptive_on_display` is a cross-crate `const fn` without `#[inline]`; it stays an out-of-line call even under the release profile's thin LTO. It costs 28.0 M Ir per 10k violations, plus 14.5 M in the scan loop itself (release codegen).

Every character it flags is non-ASCII or a C0/DEL control, so printable ASCII cannot need escaping:

```rust
fn sanitize_terminal_impl(s: &str, preserve_newlines: bool) -> Cow<'_, str> {
    // Printable ASCII has no control or display-deceptive character (every
    // one of those is C0/DEL or non-ASCII), so skip the per-char scan.
    if s.bytes().all(|b| (0x20..0x7f).contains(&b) || (preserve_newlines && b == b'\n')) {
        return Cow::Borrowed(s);
    }
    // ... unchanged slow path
}
```

Also `#[inline]` on `is_deceptive_on_display` for the non-ASCII path. Measured: github -27.4%, human -16.1%, markdown -11.7%, which is 7.8%, 4.0% and 3.3% below v0.17.0 with the full v0.18 sanitization kept. SARIF, JSON, agent and JUnit don't use this path and are unchanged.

### 3.3 Nested-rule kinds (`every_matching_has`, `for_each_dir`, `for_each_file`)

For every matched entry, each nested rule is rendered from its template into a fresh `RuleSpec` and rebuilt through `RuleRegistry::build`.

- **v0.18.0, +9.5%:** `render_path_glob` glob-escapes every substituted token value, so `app/[slug]` matches only itself. `glob_escape` always allocates twice (`globset::escape`, then `str::replace` for `\`), and `literal_glob_path` re-parses the rendered pattern char by char. Together that's about +2.8 M Ir per 1000 entries.
- **v0.19.0, +4.0%:** each instantiation now runs `validate_configured_fields`, `validate_metadata_options`, `canonical_kind` and SipHash/IndexMap lookups. The same nested spec was already validated when the parent rule was built (`for_each_dir.rs`, build-time `validate_nested`). The checks are presence-only, on fields and option keys, and templating never changes which fields are present.

Fixes:

- `needs_glob_escape` byte check. `render_path_glob` appends unescaped values directly, and `glob_escape` / `literal_glob_path` return early when no metacharacter is present.
- `RuleRegistry::build_instantiated`, which skips the duplicate validation. It's used only by the two per-entry instantiation sites; build-time validation is unchanged.

Measured: `every_matching_has` -7.2%, `for_each_dir` -5.4%. The residual (+6.3% / +1.8% over v0.17.0) is building a whole rule per entry: rendering every option, compiling a scope, and allocating six `PathTokens` strings when the template uses two. A **compile-once nested plan** would remove most of it, and it's the structural follow-up. It means recording at build time which fields carry tokens, re-rendering only those, and resolving a literal `file_exists` / `dir_exists` child with a direct index lookup. Since v0.17 also rebuilt per entry, this should land well below the v0.17.0 cost.

### 3.4 `pair`: +17.7%

v0.18.0 added `normalize_rel` (lexical `.` / `..` resolution of the rendered partner path). It always allocates a `Vec<&str>`, a joined `String`, then a `PathBuf`. v0.19.0 added a `RelatedLocation` per violation plus sensitive-metadata sanitization. A fast path that returns the path unchanged when no segment is empty, `.` or `..` gives -5.0%. Follow-ups: borrowed or lazy `PathTokens` (six `String`s per primary file today), and reusing one render buffer across the loop.

### 3.5 YAML structured queries: +19%

The v0.18 YAML DoS guard (`yaml_depth`) tokenizes every YAML document twice before the real parse: once for flow depth, once in `contains_alias`. That's about 11 k Ir per small workflow file, 7.3 M of it in `scan_plain_scalar` per 1000 files. The §3.1 prefilter brings `yaml_path_matches` to -14.6%, 1.8% above v0.17.0. Bench documents contain none of `[`, `{`, `*`; real workflows often contain `[push, pull_request]` or a `'**/*.md'` glob. Fusing the two passes into one lexer walk halves the cost for those.

### 3.6 Smaller items

- `unique_by` at 100k: +6.6% at v0.19.0, from per-group sorting for deterministic output plus `BTreeMap<String, String>` allocations. Worth an allocation pass, not urgent.
- `single_file/reeval_vs_full/run_for_file` (the LSP path): +2.6 k Ir per call (~0.7 µs). Each call re-derives config-static engine gates (`visit_active_rule_tree_with_registry`, `evaluate_entry_gates`, `requires_git`, manifest-scope gates). Cache them at `Engine` construction.
- `fix_throughput/dry_run_only`: 9 → 34 µs at v0.18.0, from about 20 extra syscalls per created file (O_NOFOLLOW root open, `statx`, `readlink`). This is the v0.18 write-confinement hardening, fixed per created file and independent of tree size. Accept.
- Single-file content rules (`file_header`, `final_newline`, etc.): +3 to +11% Ir, but repeat spread is 4 to 14% because setup writes and walks real files. Within noise.

### 3.7 Not a regression: the v0.20.0 step in `every_matching_has/1000`

The published wall-clock rose +14.2% (6.09 → 6.96 ms) from v0.19.0 to v0.20.0. Instructions changed -0.1% and Callgrind's cache and branch simulation puts estimated cycles at +0.5% (L1 misses +7.6%, last-level misses identical). The v0.19.0 sample set spans 0.4% and v0.20.0's spans 5%. That's run-level noise in a single criterion sample per release, and a reason for §2.2.

## 4. Proposed fixes

### Phase A: measurement (no product change)

1. **Racy-index settle** in the bench harness (§1). Then update the `METHODOLOGY.md` caveat, and consider re-enabling the 1k `changed` gate after two clean runs.
2. **`det_engine` thread fix** (§2.1), landed as an explicit re-baseline. Add a floor assertion so a future blind spot fails loudly, e.g. `engine_run n10k` Ir ≥ 10 M.
3. **Deterministic micro gate.** Productize the counting criterion stand-in and K-differencing as an `xtask` command. In `bench-record` (and optionally on PRs touching engine crates) it builds the previous tag and the new one with the same toolchain, compares per-bench Ir, and soft-fails above about +3%. About 10 minutes with parallel Callgrind processes, and immune to host load.
4. **Cumulative-drift check** in `bench-gate`: also compare against an anchor release (the previous minor's `.0`, or the oldest of the last three), warning at about +10% cumulative even when every adjacent step passes.
5. **Det gate on `main`.** Run `det-perf-gate.sh` on push to `main` as well as on pull requests, comparing against the previous `main` commit, or require PRs for changes under `crates/alint-core`, `alint-rules` and `alint-dsl`.
6. **Same-session criterion A/B** in `bench-record`: bench the previous tag with `--save-baseline prev`, then the new tag with `--baseline prev`, so wall-clock deltas come from one session.

### Phase B: low-risk product fixes (prototyped)

Per-iteration Ir, all fixes applied together on v0.20.0:

| Bench | v0.17.0 | v0.20.0 | v0.20.0 + fixes | v0.17 → v0.20 | Fixes | Net vs v0.17.0 |
|---|---:|---:|---:|---:|---:|---:|
| `dsl_extends/chain_depth/16` | 832,861 | 1,531,877 | 1,240,559 | +83.9% | -19.0% | +49.0% |
| `dsl_extends/chain_depth/1` | 118,459 | 183,022 | 166,200 | +54.5% | -9.2% | +40.3% |
| `output_formats/10000_violations/github` | 100,224,910 | 127,319,377 | 92,432,375 | +27.0% | -27.4% | -7.8% |
| `output_formats/10000_violations/human` | 185,986,776 | 212,671,842 | 178,535,633 | +14.3% | -16.1% | -4.0% |
| `output_formats/1000_violations/markdown` | 19,898,390 | 21,784,820 | 19,239,029 | +9.5% | -11.7% | -3.3% |
| `cross_file/every_matching_has/1000` | 31,703,898 | 36,309,701 | 33,699,345 | +14.5% | -7.2% | +6.3% |
| `cross_file/for_each_dir/1000` | 25,907,129 | 27,904,886 | 26,385,783 | +7.7% | -5.4% | +1.8% |
| `cross_file/pair/1000` | 7,823,688 | 9,209,043 | 8,745,518 | +17.7% | -5.0% | +11.8% |
| `structured_query/yaml_path_matches/1000` | 75,132,965 | 89,588,587 | 76,500,007 | +19.2% | -14.6% | +1.8% |
| `rule_engine/10000` (control) | 36,267,169 | 35,975,138 | 35,969,108 | -0.8% | 0.0% | -0.8% |
| `glob_match/10000` (control) | 12,580,003 | 12,524,989 | 12,525,166 | -0.4% | 0.0% | -0.4% |
| `cross_file/unique_by/10000` (control) | 74,938,009 | 74,995,551 | 75,003,636 | +0.1% | 0.0% | +0.1% |

The fixes:

1. Printable-ASCII fast path in `sanitize_terminal_impl`, plus `#[inline]` on `is_deceptive_on_display`.
2. Allocation-free glob-escape and literal-path fast paths in `template.rs`.
3. `RuleRegistry::build_instantiated` for per-entry nested instantiation.
4. Fast path in `pair::normalize_rel`.
5. Byte prefilters in `yaml_depth`.
6. Two-touch config memo.

Each preserves the behavior its tests pin. The fast paths return exactly what the slow paths return for inputs without the trigger bytes.

### Phase C: structural follow-ups

1. Compile-once nested-rule plan with a direct index lookup for literal `file_exists` / `dir_exists` children (§3.3).
2. Borrowed or lazy `PathTokens` and a reused render buffer in `pair` and the nested kinds (§3.4).
3. Fused single-pass YAML guard lexer (§3.5).
4. Engine gate caching for `run_for_file` (§3.6).
5. Allocation pass on `unique_by` grouping (§3.6).
6. Optionally, trust and provenance marking during config deserialization (§3.1).

## 5. Reproducing

- Macro artifact: build the harness at `main` and run `xtask bench-scale --sizes 1k --scenarios S1 --modes changed --runs 1 --warmup 0 --json-only --alint-binary <wrapper>`. The wrapper runs `strace -f -e trace=openat git -C "$2" ls-files --modified --others --exclude-standard -z` before exec'ing alint. Count non-`.git` regular-file opens, and compare `git ls-files --debug` mtimes with `stat -c %Y .git/index`. Repeat until a tree lands inside one second, which is about half of them.
- Micro instruction counts: in a scratch worktree at each tag, swap the dev-dependency `criterion.workspace = true` for a path dependency on a stand-in crate that exports the same API. Its `Bencher::iter` runs `INV_K` iterations of the benchmark whose `group/id` equals `INV_FILTER`. Build with `cargo bench -p alint-bench --features fs-benches --no-run`, then run each binary under `valgrind --tool=callgrind` at two K values with `RAYON_NUM_THREADS=1`.
- `det_engine` blind spot: run the equivalent criterion fixture under Callgrind with `--collect-atstart=no --toggle-collect='<alint_core::engine::Engine>::run'`, then again with the rayon job toggles in §2.1, and compare both with the full-collection K-difference.
