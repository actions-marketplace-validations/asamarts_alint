# v0.17 → v0.20 benchmark increases: a harness artifact, gate blind spots, and real per-kind regressions

Status: fixed. The measurement fixes are merged (#345, with a follow-up for the new micro A/B in #349), and the product fixes (#347) are in review; [§6](#6-resolution) has what landed and the measured outcome. The v0.20.0 bench PR (#341) passes every gate, but its 1k `changed` cells (+62% / +49%) and several micro benches (+10% to +84% since v0.17.0) looked alarming. Each increase was traced to its cause with load-immune measurements. One is a benchmark-harness artifact. Several are real regressions in specific rule kinds, config loading, and output formatting that accumulated over v0.18 to v0.20 in steps too small for any gate. Two measurement gaps let them through. Large-repo `alint check` performance has **not** regressed: macro `full` S1 to S3 at 100k and 1m are within ±3% from v0.16.0 to v0.20.0.

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

## 6. Resolution

Every Phase A item and every Phase B fix was applied. Phase C items 1, 2, 3, 5 and 6 were applied, item 1 in re-scoped form. Item 4 was declined.

| PR | Contents | State |
|---|---|---|
| [#345](https://github.com/asamarts/alint/pull/345) | Phase A 1 to 6: racy-index settle with a regression test; `det_engine` rayon fix with a 10 M floor check on `engine_run n10k`; `xtask micro-det`, the deterministic micro gate (`pr` subset on PRs and pushes to `main`, advisory; `full` suite in `bench-record`, gating); `bench-gate --anchor` with [`docs/benchmarks/ANCHOR`](../../ANCHOR) at v0.20.0 and +10%, failing; det gate on pushes to `main` (advisory); same-session criterion A/B in `bench-record` (report-only) | merged |
| [#346](https://github.com/asamarts/alint/pull/346) | tokio 1.54.1 for RUSTSEC-2026-0384, which was failing Audit and Deny on every PR | merged |
| [#347](https://github.com/asamarts/alint/pull/347) | Phase B 1 to 6; Phase C 1, 2, 3, 5, 6 | open |
| [#349](https://github.com/asamarts/alint/pull/349) | The `bench-record` criterion A/B builds the previous release in its own target directory (see [Measurement lessons](#measurement-lessons)) | merged |

### Changes from the proposal

- **C1, re-scoped.** A compile-once nested plan would re-implement each child kind's evaluation. Profiling the B-phase build showed most of a nested entry's remaining cost was compiling the rendered literal path into a `GlobSet` (18.6k of 30.9k Ir per entry in `every_matching_has/1000`). `Scope::from_patterns` now recognizes a sole literal include pattern and matches it by byte comparison, which is globset's literal strategy. Where `\` is a separator (Windows), globset rewrites a pattern's `\` to `/` outside character classes, so the literal is normalized the same way and a pattern containing a class keeps the `GlobSet`. The first push missed this and failed `windows-latest`. An exhaustive test checks every pattern up to length 3 against a real `GlobSet` on every CI platform.
- **Found during validation: one glob candidate per scope check.** The same-session criterion A/B showed `glob_match` +2 to +5% wall-clock with flat instruction counts. The single-literal branch had tipped LLVM's inliner: one of the two `GlobSet::is_match` calls in `Scope::matches` was no longer inlined into per-file loops. `is_match` prepares a fresh `Candidate` (normalization, basename, extension) on every call, so every check was preparing the path twice, even for an empty `exclude` set. One shared candidate with `is_match_candidate` removes that cost and the inlining sensitivity: `rule_engine` -23%, `glob_match` -12%, every other rule kind -2 to -4% (Ir).
- **C2.** `PathTokens::from_path_with` derives only the tokens a template uses, and every token when the path needs glob escaping, so message unescaping is unchanged.
- **C3.** `scan_guards` answers both YAML guards (flow depth and alias expansion) from one prefiltered lexer pass at all ten call sites. The expansion budget pass still runs only when an alias was seen.
- **C4, declined.** `run_for_file` is +26% against v0.17.0, about 2.6k instructions per call (under a microsecond per LSP re-evaluation). Caching the gate would add an invalidation path for that saving.
- **C5.** `unique_by` groups in a `HashMap` and sorts only the colliding keys, so report order is unchanged; a test pins it.
- **C6, narrow.** Extends provenance and field-source markers are updated in place in one pass per mapping. Moving the marking into deserialization was not needed for the gain.
- **1k `changed` cells stay advisory.** On kbench, 5 interleaved fresh trees per harness: S1 spread 79% → 12%, S2 65% → 12%, S4 7% → 2%. The bimodality is gone, but ±6 to 9% residual on a 10 to 20 ms cell is still too wide to gate.

### Deterministic outcome

One `xtask micro-det --suite full` run (Callgrind, K-differenced, all threads, one toolchain), v0.17.0 as anchor. micro-det runs each bench with `TMPDIR` under its scratch directory, so path-heavy benches (config loading) count more instructions than the §3 sweep, which used `/tmp`. Compare within this table, not across tables.

| Bench | v0.17.0 | v0.20.0 | #347 | vs v0.20.0 | vs v0.17.0 |
|---|---:|---:|---:|---:|---:|
| `cross_file/every_matching_has/1000` | 31,520,420 | 36,194,332 | 11,051,321 | -69.5% | -64.9% |
| `cross_file/for_each_dir/1000` | 25,923,136 | 27,654,990 | 8,797,134 | -68.2% | -66.1% |
| `cross_file/unique_by/10000` | 74,912,758 | 74,967,026 | 39,639,006 | -47.1% | -47.1% |
| `cross_file/unique_by/100000` | 959,882,102 | 1,002,553,740 | 677,513,743 | -32.4% | -29.4% |
| `output_formats/10000_violations/github` | 100,333,521 | 124,763,137 | 89,876,137 | -28.0% | -10.4% |
| `cross_file/pair/1000` | 7,737,125 | 8,839,194 | 6,495,614 | -26.5% | -16.0% |
| `rule_engine/10000` | 36,265,523 | 35,973,916 | 27,674,255 | -23.1% | -23.7% |
| `single_file/pinned_references/1000` | (new in v0.19) | 18,511,956 | 14,362,533 | -22.4% | |
| `dsl_extends/chain_depth/16` | 961,320 | 1,791,685 | 1,406,868 | -21.5% | +46.3% |
| `output_formats/10000_violations/human` | 186,147,735 | 212,773,914 | 178,637,704 | -16.0% | -4.0% |
| `structured_query/yaml_path_matches/1000` | 76,367,470 | 89,995,333 | 75,496,797 | -16.1% | -1.1% |
| `glob_match/10000` | 12,846,607 | 12,791,608 | 11,284,941 | -11.8% | -12.2% |
| `output_formats/1000_violations/markdown` | 19,875,730 | 21,766,756 | 19,220,964 | -11.7% | -3.3% |
| `dsl_extends/chain_depth/1` | 127,532 | 198,716 | 182,064 | -8.4% | +42.8% |
| `single_file/reeval_vs_full/run_for_file/1000` | 10,099 | 12,694 | 12,187 | -4.0% | +20.7% |

None of the 61 gated benches regressed against v0.20.0; the largest increase is +0.8%. Three things stay above v0.17.0, all accepted:
- Config loading (+43 to +46%), the cost of v0.19/v0.20 trust, provenance and compatibility tracking.
- `run_for_file` (+21%, C4).
- `fix_throughput/dry_run_only` (the v0.18 write confinement, §3.6).

The single-file content rules move by up to ±12% between refs in this run, but repeat runs of the same ref overlap within that range (setup writes and walks real files), so they stay report-only.

### Wall-clock on the canonical host

Four macro measurements on kbench (i7-6700HQ, `ALINT_BENCH_DROP_CACHES=1`) at bench-record's flags: 1k to 1m, S1 to S4 and SFIX, 3 warmups and 10 runs per cell (1 and 3 at 1m). All four used the racy-fixed harness.

- **A/B 1:** one session, v0.20.0 first, then #347 at `3d4784649`, before the shared-candidate commit.
- **A/B 2:** one session, v0.20.0 first, then #347's final code at `2e9854c17`.
- **bench-record 1:** the workflow itself, dispatched on a validation branch of #347 at `8c7b5a9a1` and compared with the published v0.20.0 run (results PR #348, closed unmerged).
- **bench-record 2:** the workflow again, with #349's fix, on a validation branch carrying #347's final code (`042db8f4f`), compared with the published v0.20.0 run (results PR #350, closed unmerged).

`bench-gate` passes in every case, and in both directions for the two A/Bs. A/B 2 and bench-record 2 measure the final code. Change against v0.20.0, median / min (the gate uses min):

| Cell | A/B 1 | A/B 2 | bench-record 1 | bench-record 2 |
|---|---:|---:|---:|---:|
| S2 10k full | -4.2 / -3.8% | -0.3 / -3.0% | -5.7 / -4.6% | -4.4 / -4.5% |
| S2 100k full | -4.0 / -3.6% | -2.8 / -3.0% | -4.7 / -4.3% | -4.5 / -4.5% |
| S2 1m full | -2.8 / -2.7% | -4.2 / -3.9% | -3.4 / -3.3% | -3.1 / -3.0% |
| S4 10k full | -5.7 / -4.2% | -2.1 / -3.7% | -4.0 / -3.5% | -2.5 / -3.9% |
| S4 100k full | -4.6 / -3.8% | -4.2 / -4.5% | -3.4 / -3.9% | -2.3 / -4.2% |
| S4 1m full | -3.0 / -2.9% | -7.6 / -4.5% | -5.1 / -4.4% | -4.5 / -4.5% |
| S1 10k full | -1.9 / -1.3% | +0.4 / -0.5% | -0.9 / -1.7% | +0.2 / -2.5% |
| S1 100k full | -1.6 / +0.1% | +0.6 / -0.6% | -5.3 / -2.1% | -3.6 / -2.2% |
| S1 1m full | +0.5 / +0.0% | +3.1 / +3.3% | -4.2 / -1.5% | -2.6 / -0.3% |
| S3 10k full | +0.1 / +0.6% | +0.2 / +0.5% | +0.5 / +0.0% | +0.6 / -0.1% |
| S3 100k full | -3.0 / -0.1% | +0.2 / +0.3% | -1.0 / +0.3% | -1.5 / -0.3% |
| S3 1m full | +0.1 / -0.1% | +5.7 / -0.1% | +0.1 / +0.3% | +0.2 / +0.4% |
| SFIX 10k fix | -1.5 / -1.0% | -1.7 / -2.8% | -1.2 / -0.9% | -1.6 / -1.1% |
| SFIX 100k fix | -1.0 / -0.8% | -1.8 / -2.5% | -1.0 / -0.6% | -1.7 / -1.6% |
| SFIX 1m fix | -0.7 / -1.1% | -3.7 / -1.4% | -3.1 / -2.2% | -3.0 / -2.4% |

S2 (structured queries) and S4 (nested and cross-file kinds) are faster from 10k to 1m by min in every measurement: 3.0 to 4.5% for the final code in both of its sessions, 2.7 to 4.6% across all four.

S1 (walking) and S3 (content scanning) move within ±3% between sessions, in both directions. In A/B 2, S1 1m full read +3.3% by min. That figure rests on 3 timed runs, with v0.20.0's whole matrix run first. To check it, the same two binaries ran on the same generated S1 trees under a deterministic count. #347 executes 6.1% (100k) and 6.3% (1m) fewer instructions (Callgrind, all threads). Its syscall counts are the same: 219,625 vs 219,140 at 100k, 2,086,629 vs 2,086,941 at 1m. The output is identical. bench-record 2, the same code in a later session, measured S1 1m full at -0.3% by min (-2.6% median). The S3 1m median of +5.7% in A/B 2 has a min of -0.1%.

The 1k `changed` cells that prompted this investigation were published at 20.7 ms (S1) and 22.5 ms (S2) for v0.20.0. With the settled index, v0.20.0 runs them in 12.4 to 14.1 ms (S1) and 15.7 to 15.8 ms (S2). #347 runs them in 12.2 to 13.6 ms and 15.0 to 15.4 ms, and bench-record 2 published 13.0 ms and 14.8 ms. So almost all of the published increase was the harness artifact.

### Micro wall-clock

The criterion suite is wall-clock and report-only; the instruction counts above are what gate. Three findings from kbench:

- **The first same-session criterion A/B** ran v0.20.0, then #347 at `be5bc60f3`. It agreed with the instruction counts for every changed kind: nested kinds -69 to -72%, `unique_by` -23 to -42%, github -21 to -23%, human -14 to -21%, `pair` -11 to -15%, YAML queries -11%. It also moved benches whose instruction counts did not change: junit +7%, gitlab/10000 +12.6%, `rule_engine` and `glob_match` +2 to +3.5%.
- **`glob_match` was real.** It led to the shared glob candidate described above.
- **Interleaving cancels session drift.** In the interleaved check, both refs' bench binaries alternate order over 6 rounds (ABBA), and each round is compared on its own. The final code against v0.20.0:

| Bench | v0.20.0 | #347 | Per-round change | Mean |
|---|---:|---:|---|---:|
| `rule_engine/10000` | 2.290 ms | 1.865 ms | -18.4 -17.8 -19.0 -18.2 -16.8 -19.7 | -18.3% |
| `glob_match/1000` | 0.162 ms | 0.145 ms | -11.0 -10.9 -10.9 -8.7 -8.9 -10.6 | -10.2% |
| `glob_match/100000` | 16.402 ms | 14.870 ms | -7.5 -10.1 -10.6 -8.0 -8.5 -9.1 | -9.0% |
| `output_formats/10000_violations/json` | 5.577 ms | 5.354 ms | -4.2 -3.9 -3.9 -4.0 -4.1 -2.9 | -3.8% |
| `output_formats/10000_violations/gitlab` | 32.105 ms | 32.018 ms | -0.4 -1.4 -0.4 -0.2 +0.3 -2.0 | -0.7% |
| `output_formats/100_violations/junit` | 0.124 ms | 0.127 ms | +2.0 +3.1 +5.0 +3.9 +3.8 +4.3 | +3.7% |
| `output_formats/10000_violations/junit` | 12.253 ms | 12.879 ms | +5.5 +5.9 +4.1 +5.0 +9.2 +4.1 | +5.7% |

junit's code is unchanged, and Callgrind counts the same instructions for both refs (111,282,547 at 10000 violations). Its +4 to +6% on kbench holds in every interleaved round. The same interleaved check on a Zen 2 machine measured -1.3% and +0.3%; that run used `be5bc60f3`, whose junit code is the same. The same instructions sit at different addresses, so this is a code-layout effect on the Skylake host, not added work.

**After #349, bench-record's own A/B agrees on the large moves.** [Run 39926639804](https://github.com/asamarts/alint/actions/runs/39926639804) ran the workflow, with #349's fix, on #347's final code (`042db8f4f`, results PR #350, closed unmerged). Each ref compiled its own sources (2 m 36 s and 2 m 35 s), and 89 of 98 benches changed significantly. The large moves match the instruction counts that the same run's micro-det job measured:

| Bench | A/B wall-clock | micro-det Ir |
|---|---:|---:|
| `cross_file/every_matching_has/5000` | -71.2% | -71.0% |
| `cross_file/for_each_dir/1000` | -68.1% | -69.5% |
| `cross_file/unique_by/10000` | -45.4% | -45.4% |
| `output_formats/1000_violations/github` | -28.8% | -23.8% |
| `cross_file/pair/1000` | -20.7% | -28.0% |
| `rule_engine/10000` | -17.7% | -25.2% |
| `structured_query/yaml_path_matches/100` | -14.7% | -14.6% |
| `glob_match/10000` | -9.4% | -12.9% |

The benches that rose in it did not rise in instruction counts:
- agent and sarif output rose +9.0 to +9.5% at 10000 violations, and json and junit +3.2 to +5.3%. `literal_facts/20_absent/100000` rose +6.1%. All of these are within ±0.5% Ir.
- `final_newline/100` rose +5.0% at -7.5% Ir, and `dsl_extends/chain_depth/1` +4.4% at -8.4% Ir.

An interleaved run of the output formats on kbench (6 rounds, as above) does not reproduce the rises:

| Bench | v0.20.0 | #347 | Per-round change | Mean |
|---|---:|---:|---|---:|
| `output_formats/100_violations/agent` | 0.137 ms | 0.139 ms | +2.5 +0.5 -5.5 +8.4 +2.0 +0.1 | +1.3% |
| `output_formats/1000_violations/agent` | 1.344 ms | 1.342 ms | -9.3 -7.5 +2.5 +9.4 -1.8 +0.6 | -1.0% |
| `output_formats/10000_violations/agent` | 15.209 ms | 13.864 ms | +1.7 -24.7 +5.2 +7.5 -19.3 -19.5 | -8.2% |
| `output_formats/100_violations/sarif` | 0.181 ms | 0.182 ms | +1.3 +0.9 -0.3 +0.6 +0.6 +0.3 | +0.6% |
| `output_formats/1000_violations/sarif` | 1.839 ms | 1.847 ms | +0.7 +0.0 +0.3 +8.4 +0.5 +0.0 | +1.7% |
| `output_formats/10000_violations/sarif` | 18.433 ms | 18.679 ms | +1.1 -7.3 +1.6 +8.7 +1.7 +1.2 | +1.2% |
| `output_formats/1000_violations/json` | 0.561 ms | 0.540 ms | -3.8 -4.0 -10.4 -3.9 -6.1 -3.6 | -5.3% |
| `output_formats/10000_violations/json` | 5.568 ms | 5.352 ms | -3.9 -4.1 -3.9 +4.2 -5.5 -3.8 | -2.8% |

Two things separate the interleaved run from the ordered one:
- **Drift lands on one side.** In round 4 every #347 output bench ran 5 to 9% slower than in its other rounds. An ordered A/B charges such a slow stretch to whichever ref ran then.
- **`agent` at 10000 violations is bimodal on v0.20.0 itself.** Its rounds take either 13.4 to 13.7 ms or 16.7 to 18.3 ms; #347's stay at 13.4 to 14.5 ms. Against v0.20.0's fast rounds, #347 is +0.3% best to best and +2.6% median to median.

So the output formats with unchanged instruction counts move -5% to +3% on this host: sarif +0.6 to +1.7%, agent -1.0 to +2.6%, json -2.8 to -5.3%. json flipped sign from the ordered A/B, where it read +3.5 to +5.3%.

### Measurement lessons

- **Cargo shares artifacts across worktrees at the same package version.** Cargo names a workspace package's artifacts by name, version and workspace-relative path. Two refs of the same version built into one target directory therefore get the same file names, and the second build can judge the first one's artifacts fresh. One early micro-det comparison measured identical binaries this way, so `xtask micro-det` now runs `cargo clean -p` for every workspace package before each ref's build. The criterion A/B that #345 added to `bench-record` had the same flaw. In the validation run the new ref's build finished in 0.24 s, and the A/B compared v0.20.0 with itself. Tag runs were never affected, because the release bump changes the version. [#349](https://github.com/asamarts/alint/pull/349) gives the previous release its own target directory, and a shell test pins the separation.
- **Benches that read real files are noisy in Ir too.** On hosted runners, glibc allocator behavior moved `xml_path_equals` and `yaml_path_matches` 1.8 to 2.8% between identical refs, so they report but do not gate in the PR subset.
- **An ordered micro A/B shows layout and drift, not just code.** bench-record's criterion A/B runs the previous release first, then the new ref. Benches with identical instruction counts can move several percent in it: junit +7% and gitlab/10000 +12.6% in one run, agent and sarif +9 to +9.5% in another (-1 to +3% interleaved), and json +3.5 to +5.3% in the second where interleaving gives -2.8 to -5.3%. Read it for large changes, and confirm small ones with an interleaved run. The instruction counts are what gate.
- **The first push-to-`main` gate after #345 reports a one-time step.** It compares the new `det_engine` with the base's blind one: `engine_run` n1k +1634%, n10k +2533% (advisory). From the next push on, both sides count the rayon workers.
