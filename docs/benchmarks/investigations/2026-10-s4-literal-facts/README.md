# S4 v0.18.4 → v0.19.0: workload reset and literal-fact rescans

Status: resolved for the next release. The published v0.18.4 and v0.19.0 S4 rows are different workload revisions, but the added workload also exposed a real avoidable O(facts × files) implementation cost. Both issues are fixed: exact-path facts use one lazy file index, and the benchmark/reporting contract now makes workload changes explicit.

## Symptom

The canonical release-host S4 series moved materially between v0.18.4 and v0.19.0:

| Mode / size | v0.18.4 | v0.19.0 | Delta |
|---|---:|---:|---:|
| full / 1k | 33.2 ms | 57.9 ms | +74.5% |
| changed / 1k | 36.6 ms | 55.3 ms | +51.2% |
| full / 10k | 156.4 ms | 203.3 ms | +30.0% |
| changed / 10k | 157.5 ms | 146.4 ms | −7.1% |
| full / 100k | 1448.9 ms | 1775.7 ms | +22.6% |
| changed / 100k | 845.6 ms | 1132.0 ms | +33.9% |
| full / 1M | 16.169 s | 18.727 s | +15.8% |
| changed / 1M | 9.561 s | 11.912 s | +24.6% |

The signal was isolated to S4. Other scenarios remained flat, and an equal-workload Callgrind comparison of the v0.18.4 and v0.19.0 binaries differed by only +0.217%. This ruled out a broad walker or engine regression.

## Diagnosis

Two independent facts were being conflated.

### 1. v0.19.0 silently changed the S4 workload

S4 revision 1 resolved 50 rules. Revision 2 added the `versioning` and `toolchain-pins` bundles, bringing the resolved count to 85. The synthetic tree did not contain their root authority files, so all 20 newly added facts were false and all 35 newly resolved rules were inactive. The active-rule count remained 47.

The published history displayed the timings without workload revisions. A reader could therefore interpret the v0.18.4 r1 → v0.19.0 r2 boundary as an equal-work regression even though config resolution and fact evaluation had changed.

### 2. False exact-path facts scanned the whole index

The 20 new facts comprise eight `any_file_exists` facts and twelve `file_content_matches` facts. Their inputs are exact root-relative paths. The evaluator nevertheless compiled them as scopes and called `index.files().any(...)`. Because every path was absent in the revision-2 fixture, every fact traversed all N entries: O(20 × N).

Callgrind on the same v0.19.0 binary and 100k-file polyglot fixture separated the bundle additions:

| Workload | Instructions | Delta vs r1 |
|---|---:|---:|
| S4 r1 bundles | 3.966 B | — |
| r1 + versioning | 4.223 B | +6.47% |
| r1 + toolchain-pins | 5.095 B | +28.46% |
| S4 r2 bundles | 5.368 B | +35.34% |

The additions were independently additive. No rule dispatch was required to incur the cost: `alint facts` reproduced it before rule evaluation.

## Remediation and result by item

### Exact-path fact lookup

`FileIndex` now lazily builds one `HashMap<Arc<Path>, usize>`. Exact existence checks use map membership; exact content facts use the indexed `FileEntry`. The map replaces the internal file-path set for normal exact lookups and is shared with changed-file indexes when whole-tree lookups are required. The public set view remains lazy for API compatibility, so normal evaluation does not build duplicate indexes.

Only fully literal pattern lists take the fast path. Real globs and leading exclusions retain `Scope` matching. The literal decoder is the existing glob-aware helper, so escaped metacharacters, ignore filtering, repository boundaries, file-versus-directory behavior, bounded reads, structured extraction, and malformed-glob-before-regex validation are unchanged.

Controlled Callgrind A/B: same generic 100k-file tree, same two configs, release binaries built with the same toolchain. `r1` is the six pre-v0.19 bundles; `r2` adds versioning and toolchain-pins.

| Workload | Before | After | Result |
|---|---:|---:|---:|
| r1 reference | 3,971,291,017 Ir | 3,974,556,592 Ir | +0.08% |
| r2 affected | 5,384,796,126 Ir | 4,109,705,441 Ir | −23.68% |
| incremental r2 − r1 | 1,413,505,109 Ir | 135,148,849 Ir | **−90.44%** |

The reference workload stayed flat while 90.44% of the newly introduced instruction overhead disappeared. The fixed r1 reference already builds the lazy index for its exact Cargo-workspace content fact; the remaining +3.40% is the cost of resolving and evaluating the 20 additional facts against that index.

A 10-run local wall-clock check agreed directionally, but the host was not quiescent: CV ranged from 11.4% to 20.2%, and the fixed r2 row was faster than its r1 control. Those values are deliberately not used as publication evidence; deterministic instructions are the controlled result.

### Permanent performance guard

`benches/facts.rs` evaluates the same eight-existence/twelve-content absent-literal mix against a fresh 1k, 10k, and 100k index. The timed operation includes the lazy index build, guarding the intended O(files + facts) shape. `det_engine` adds a load-immune 10k Callgrind cell; its fixed baseline is 9,667,081 instructions. Future PR comparisons can now catch a return to per-fact scans without relying on S4 wall-clock noise.

Correctness coverage separately pins present and absent literals, escaped literals, mixed include/exclude lists, literal-plus-glob `all_files_exist`, content exclusions, walked-index/ignore behavior, and validation precedence.

### S4 workload revision 3

The git-aware polyglot generator now commits realistic root authorities for repository version, Node, Rust, Python, Ruby, JVM, and .NET. Their versions agree across package metadata, `.tool-versions`, Docker stages, and CI setup actions. This turns S4 into the workload it claimed to represent rather than timing 35 fact-gated no-ops.

| Contract | r2 | r3 | Result |
|---|---:|---:|---|
| Resolved rules | 85 | 85 | unchanged |
| True facts | 3 / 24 | 23 / 24 | 20 newly true; only Cargo workspace remains false |
| Active rules | 47 / 85 | 82 / 85 | all 35 version/toolchain rules execute |

The three intentionally inactive rules require a root Cargo `[workspace]`; the nested polyglot fixture is deliberately not a Cargo workspace. A scenario contract asserts the exact false-fact set, inactive-rule set, and counts. Any future fixture or bundle change must update that contract and bump the workload revision.

A local 100k r3 end-to-end run exercised both `full` and `changed` CLI paths, but failed the publication quality gate (48.5% and 32.9% CV). The gate correctly emitted `RESET S4: workload r2 → r3` and skipped cross-revision regression comparison. These timings characterize execution only; they are not committed as release data and are not used to claim an r2 → r3 speedup.

### Revision-aware history and trajectory data

Every generated per-scenario table now has a workload column. The newer side of a revision boundary is rendered as `rN ← reset from rM`; the headline table labels S4 the same way. The website trajectory JSON carries a per-row `workload_revisions` map. End-to-end renderer tests pin the report-to-trajectory revision value, the visible reset marker, and byte-for-byte agreement with committed `HISTORY.md`.

Release guidance now requires reviewers to verify the revision, resolved-rule count, fact truth profile, active-rule profile, and visible reset boundary. The gate continues to enforce within-run quality on a new revision but never compares unlike workloads.

## Validation

The implementation was validated with:

- focused fact and `FileIndex` tests, including exact, glob, exclusion, content, ignore/index, and error-precedence cases;
- the S4 r3 fact/active-rule contract;
- actual optimized `alint facts` and `bench-scale` CLI runs;
- deterministic Callgrind A/B and the new gungraun cell;
- the Criterion scaling benchmark;
- renderer regeneration and trajectory end-to-end tests;
- the complete workspace test, documentation-generation, schema/facts/model, and repository dogfood gates.

### Native macOS cross-check

An immutable reviewed snapshot (`90e4cd6ff944bbe82014e5ac51ca1f501b5babef`)
was streamed to a private Apple Silicon machine under the repository's isolated
validation runbook. Rust 1.99.0 ran `cargo test --workspace --locked`, workspace
Clippy with all targets and warnings denied, a release build, repository dogfood,
the focused maximum-file-length check, the JSON list contract, the actual
100k-file facts CLI workload, and the Criterion fact benchmark. All passed. The
release binary reports `platform MACOS` and minimum OS 11.0. The ephemeral source,
home, Cargo home, target, fixture, and temporary directories were removed, and a
second SSH connection independently verified cleanup.

The fixed 100k facts workload was run ten times per config after two warmups,
alternating config order:

| Apple Silicon facts run | r1 mean ± sd | r2 mean ± sd | r2 overhead |
|---|---:|---:|---:|
| Pre-fix diagnostic | 252 ms | 353 ms | +40.1% |
| Fixed snapshot | 477.2 ± 2.1 ms | 484.7 ± 2.4 ms | **+1.57%** |

The runs used separately isolated source/build/fixture trees, so their absolute
times are not an A/B and are not compared. The within-run r2/r1 control is
comparable: the added-bundle gap fell by 96.1%, with fixed-run CVs of 0.43% and
0.50%. The native Criterion smoke measured 66.6 µs, 399.3 µs, and 3.95 ms at 1k,
10k, and 100k respectively, with stable 10k/100k throughput near 25 million
indexed files/s.

Machine-readable measurement aggregates, including quality labels for rejected
wall-clock runs, are in [`measurements.csv`](measurements.csv).

## Publication

Do not rewrite the v0.18.4 or v0.19.0 rows: they are valid measurements of r1 and r2 respectively. `HISTORY.md` now labels their reset boundary. The next release records S4 r3 on the canonical benchmark host; only later r3 releases are comparable to it. Development-host and private-host measurements remain investigation evidence, not published release baselines.
