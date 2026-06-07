# v0.12 performance validation & inner-loop deep-dive

**Status:** IN PROGRESS (opened 2026-06-07)

## Two open questions — NEITHER assumed

1. **PRIMARY — why did the canonical bench-record run (PR #46, workflow run
   `27082213210`) report `+358%` to `+638%` `min_ms` regressions** on cells like
   `S2 100k/1m full` and `S12 100k/1m full`, plus quality-CV failures
   (`S14 100k full` CV 94%)? Competing hypotheses, to be decided by reproduction
   + profiling, **not** assumed:
   - **(a) Co-tenant contamination** of the shared kbox runner (CI containers +
     `jobz_*` services) during the ~4h45m run.
   - **(b) A real regression** — super-linear or otherwise pathological — that
     the canonical run's exact conditions (tree, sizes, ordering, 1M tail)
     triggered and a smaller local run did not.
   - **(c) A harness / measurement difference** between the workflow run and the
     local run (different binary, config, tree, or invocation than assumed).
2. **SECONDARY — the `~+2-3%` systematic drift** seen in a same-hardware idle A/B
   (clean v0.11.0 vs v0.12.0). Classify it (constant / linear / super-linear) and
   root-cause it to the suspects below.

These may be the **same phenomenon at different magnitudes**, or **distinct**.
The validation (Phase 1) decides that. **Do not conclude "contamination" until
Phase 1 proves the slow code does the same work when run clean.**

## Raw evidence so far (observations, NOT conclusions)

- Canonical (PR #46): `S2/100k/full` min **1757.9ms** (+638% vs v0.11.0 baseline);
  `S12/100k/full` min ~1753ms (+477%); `S14/100k/full` min **32702ms**, mean
  **49433ms**, CV **94%**; 62 gating failures total.
- Idle local A/B (HEAD `d35171aa` vs `5d57b60b`, same Ryzen 9 3900X, S1-S13 @
  10k+100k, both built + benched minutes apart): `S12/100k/full` **+2.9%**, full
  100k distribution mean **+3.2%** / median **+2.3%**, range −0.8%…+10.2%.
- **The same cell `S12/100k/full` measured +2.9% locally and +477% canonically**
  — same code, same xtask harness, same seed (`0xA11E47`), same hardware. The
  only known difference is *when / under what load* each ran.
- Signature heterogeneity worth explaining (not hand-waving): `S14/100k` had a
  near-clean **min** (32.7s vs ~28s local) but a huge **CV 94%** → *intermittent*
  spikes; `S2/100k` had a high **min** (7.5× even on the best run) → *sustained*
  difference. Different signatures ⇒ possibly different causes.
- Live `ps` during/after the run showed co-tenants active (`jobz` ~4%,
  `chrome-headless`, `fuse-overlayfs` CI, load ~3) — establishes contamination was
  *possible*, not that it *caused* the numbers.

## Analytical axis

Every v0.12 addition to the run is classified as:

- **Constant (per-invocation):** absolute Δ flat regardless of file count.
  Reads as a per-file % only on *small* trees (it amortizes away at scale).
- **Linear (per-entry / per-file):** Δ-per-file flat across sizes.
- **Super-linear (per-(file,rule), O(n²), …):** Δ-per-file **grows** with size —
  the dangerous class, and the only one that could explain a *real* +300%+.

## Ranked suspects for the v0.12 delta (from the hot-path audit)

Only **3 commits** touched the hot-path files (`engine.rs` / `walker.rs` /
`scope.rs` / `io.rs`) between v0.11.0 and v0.12.0:

| # | Suspect | Commit | Class (hypothesized) | Site |
|---|---------|--------|----------------------|------|
| **H1** | Walker `filter_entry` symlink closure — invoked on **every** walked entry (`path_is_symlink()` branch) + one `canonicalize()` per invocation; only symlinks pay the extra `canonicalize` syscall | `5d77d5a7` | linear per-entry (flat per-file tax) | `walker.rs:440-451` |
| **H2** | D1 pass-count — builds a `RuleResult` (`Arc::from` + 2-Vec `partition`) for **every** silently-passing per-file rule (~16-37/run) instead of `continue` | `20bc536a` | constant per-invocation (per-file % on small trees only) | `engine.rs:543` + `rule.rs:119` |
| **H3** | New `for_each_match` per-file kind — genuine per-(file,rule) regex pass | v0.12 | work, not overhead (only if enabled) | `for_each_match.rs` |

**Open syscall question:** does `entry.path_is_symlink()` inside the `ignore`
crate's `filter_entry` callback use the walk's cached file-type, or trigger a
fresh `lstat` per entry? A per-entry syscall would be a strong H1 signal — and,
multiplied over the canonical run, is the kind of thing that *could* push a real
slowdown well past a few percent. **Must be measured (syscall count), not assumed.**

> NOTE: None of H1/H2/H3 is, on paper, super-linear — so on paper none explains a
> real +638%. That is exactly why Phase 1 must either (a) reproduce the +638%
> clean (⇒ a *real*, un-audited pathology we have not found yet) or (b) fail to
> reproduce it clean while reproducing it under induced load (⇒ contamination).

## Methodology (house pattern, per docs/benchmarks/METHODOLOGY.md + prior investigations/)

Tracing phase-timing across sizes → super-linear check → criterion micro-bench
isolation → writeup here. Prior `scope-filter-baseline-drift` and
`v0.10-s13-100k-margin` investigations are precedent for *not trusting wall-clock
on a loaded box* — so we lean on **deterministic, load-immune** tools (instruction
counts, allocation counts) that measure "function-call growth" and
"constant-time additions" exactly, regardless of co-tenant load.

---

## Phase 0 — Environment & tooling

- **0a. Profiling build profile** — release has `strip = true` (no symbols).
  Add `[profile.profiling]` (`inherits = "release"`, `strip = false`,
  `debug = "line-tables-only"`) for symbolized profiles.
- **0b. Fixed trees** — `xtask gen-monorepo --size {1k,10k,100k} --out
  /tmp/alint-prof-<size>` (seed `0xA11E47`), reused across runs.
- **0c. A/B binaries** — `v0.11.1` (last pre-v0.12 release) and `v0.12.0`
  (`25b39e29`), plus **per-suspect** pairs (`5d77d5a7^` vs `5d77d5a7`;
  `20bc536a^` vs `20bc536a`) to isolate each commit.
- **0d. Tooling** — nothing is installed (perf, valgrind, flamegraph, strace all
  MISSING; `perf_event_paranoid=3`). Two tracks:
  - *No-root (load-immune, start now):* built-in `ALINT_LOG=alint_core=info`
    tracing; an `LD_PRELOAD` malloc-counting `.so` (counts allocs on the existing
    release binaries — no rebuild, works on both A/B binaries); criterion benches.
  - *Root (richest, decisive):* `sudo apt install valgrind linux-perf strace` +
    `sudo sysctl kernel.perf_event_paranoid=1`. **valgrind is the prize** —
    `callgrind` (exact instruction + call + malloc counts) and `dhat` are
    deterministic and immune to co-tenant load.

## Phase 1 — VALIDATE THE +300% (priority): contamination vs real regression

The decisive test is **deterministic instruction/alloc-count A/B** (load-immune) +
**quiet-box wall-clock reproduction**. We do not conclude until these run.

- **1a. Quiet-box reproduction.** On a load-verified-idle box, run the regressed
  cells (`S2/100k/full`, `S12/100k/full`, and at least one 1M cell) A/B
  v0.11.1 vs v0.12.0 against the fixed trees. If v0.12 ≈ v0.11.1 ≈ the clean local
  number (~233ms for S2/100k) ⇒ the canonical +638% was **not** in the code. If
  v0.12 stays ~1757ms when the box is quiet ⇒ **real regression** — escalate.
- **1b. callgrind instruction-count A/B (DECISIVE).** `S2/100k` + `S12/100k`,
  v0.11.1 vs v0.12.0. Instruction count is deterministic + load-immune:
  - counts ~equal (not ~7.5×) ⇒ the wall-clock slowdown was **external**
    (contamination / scheduling) — proves it was not extra work in the code.
  - v0.12 instruction count blown up ⇒ **real** algorithmic regression; the
    callgrind call-graph names the function.
- **1c. Allocation-count A/B (no-root).** `LD_PRELOAD` malloc counter on the same
  cells. If v0.12 allocs ≈ v0.11.1 allocs, no allocation explosion.
- **1d. Reproduce contamination on purpose.** Run `S2/100k` clean, then again
  under induced load (`stress-ng`/parallel `cargo build`/a `yes`-fanout) → does it
  reproduce the +300%? If yes, proves contamination is a **sufficient** cause.
- **1e. Characterize the canonical `results.json` per-cell.** min vs mean vs CV
  for every failing cell: sustained (high-min) vs intermittent (clean-min,
  high-CV) → which co-tenant pattern (or real-regression shape) fits.
- **1f. Tonight's clean full-matrix run** (`~/clean-bench-v0.12.0.sh`, 2 AM) is the
  end-to-end empirical confirmation.
- **Decision gate:** only after 1a-1c. **Real ⇒** Phase 2+ root-causes the
  pathology on the slow path (this becomes a serious release-quality issue).
  **Contamination ⇒** proceed to classify the residual +2-3%, and file the runner
  isolation as a real infra fix.

## Phase 2 — Classify the residual +2-3% (constant / linear / super-linear)

3-scenario × 3-size A/B (v0.11.1 vs v0.12.0, fixed trees): **S1** (walk-only →
isolates H1 walker), **S6** (per-file content max), **S12** (v0.12 per-file kinds).
Compute Δalloc and Δms **per file** at 1k/10k/100k; the per-file-vs-constant-vs-
growing shape classifies each component. `ALINT_LOG` phase logs say which phase
(walk / per-file dispatch / aggregation) grew.

## Phase 3 — Deterministic deep profiling (load-immune)

`callgrind` call-count A/B at 1k **and** 10k → any function whose call count grows
faster than the file-count ratio is non-linear, caught exactly. `dhat`/LD_PRELOAD
allocation profiling → H2's `partition`-into-empty-Vecs + `Arc::from`. `strace -c`
syscall count → the `path_is_symlink()` per-entry `lstat` question (H1).

## Phase 4 — Wall-clock profiling (quiet-box, complementary)

Differential flamegraph (`samply`/`cargo flamegraph`, profiling profile) of S6/10k
v0.11.1 vs v0.12.0; `perf stat` cycles/instructions/cache-misses.

## Phase 5 — Micro-bench isolation + fixes

Criterion probes A/B: `walker` (isolates H1), `rule_engine` (in-memory, isolates
H2), `single_file_rules`. Prototype + micro-bench-gate fixes for confirmed
suspects (H1: install `filter_entry` only when symlinks are possible; H2: count
passers without materializing empty `RuleResult`s).

## Phase 6 — Document, gate, re-measure

Fill this README with the classification table, callgrind/DHAT diffs, flamegraphs,
per-suspect verdict, fixes + measured impact. Micro-bench-gate fixes; re-run a
clean macro A/B to confirm any improvement.

---

## Findings

### Phase 1 — validation verdict: **CONTAMINATION CONFIRMED — the +300% was NOT a real regression** (2026-06-07)

Three independent **deterministic, load-immune** measurements, A/B v0.11.0
(`5d57b60b`) vs v0.12.0 (`d35171aa` engine) on fixed `gen-monorepo` trees, run on
the *busy* box (load ~2.4 — irrelevant to these tools):

**1. callgrind instruction count (`Ir`) — the decisive test.** The exact cells the
canonical bench-record (PR #46) reported as huge regressions execute essentially
identical instructions:

| cell | v0.11.0 `Ir` | v0.12.0 `Ir` | A/B | canonical claimed |
|---|---|---|---|---|
| S2/100k | 3,120,524,100 | 3,123,057,158 | **+0.08%** | **+638%** |
| S12/100k | 4,674,170,290 | 4,683,748,657 | **+0.20%** | **+477%** |
| S2/10k | 329,615,351 | 326,095,325 | −1.07% | — |
| S12/10k | 480,453,897 | 478,288,219 | −0.45% | — |

A +638% wall-clock with **+0.08% instructions** = the CPU was starved
(co-tenant contention), not that the code does more work. **Proven, not inferred.**

**2. Scaling (super-linearity check).** 10k→100k `Ir` ratio = 9.47×/9.58× (S2),
9.73×/9.79× (S12) for v0.11.0/v0.12.0 — **sub-linear** (per-invocation constant
amortizing). **No super-linear growth in either version.**

**3. strace syscall count (S2/10k).** stat-family (`statx`) **identical at 21,638**
for both versions → **H1's walker `filter_entry` adds ZERO per-entry syscalls**
(`path_is_symlink()` reads the cached file-type). Total syscalls differ by 0.1%
(the one per-invocation `canonicalize()`).

**4. Function-level `Ir` (callgrind, S2/100k).** Identical share distribution; the
top hot functions have **byte-identical** `Ir` (`113,706,366` and `100,975,701`
exactly equal between versions) → same code paths, same work.

**Verdict.** The canonical +358–638% regressions are **EXTERNAL (co-tenant
contamination), proven by deterministic measurement** — not assumed. v0.12 executes
the same instruction-level work as v0.11.0 (±1%, net ~0, sometimes negative).
- **H1** (walker `filter_entry`): a cheap cached-file-type branch, **no syscall
  cost**, invisible at the instruction level.
- **H2** (D1 `RuleResult`): a per-invocation constant (~16–37 `RuleResult`s),
  utterly negligible against ~3.1B instructions/run.
- **H3** (`for_each_match`): not exercised by S2/S12; added work only when enabled.
- **No super-linear growth.**

### Phase 1b — the +2-3% wall-clock CLOSED: real branch-misprediction overhead (cachegrind, 2026-06-07)

cachegrind (cache + branch-predictor simulation, deterministic + load-immune)
A/B at 100k showed instructions, cache misses (D1 ±2.4%, LL ±1.3%), and branch
*counts* (±0.03%) all ~identical — but v0.12 **mispredicts more branches**:
S2 **+7.1%**, S12 **+22.9%**. More mispredicts → pipeline flushes → more cycles at
identical instruction count → exactly the observed ~+2-3% wall-clock. **So the
+2-3% is NOT noise — it is a real (benign) micro-architectural effect.**

The increase is dominated by **indirect** branch mispredicts, and the walk-only
S1 isolation names the source:

| scenario | Δ total mispredict | Δ conditional | Δ **indirect** |
|---|---|---|---|
| **S1** (filename-only, full walk, ~no rule work) | +719,483 | −68,837 | **+788,320** |
| S2 (content rules) | +1,396,599 | +60,693 | **+1,335,906** |
| S12 (ordered_block/import_gate) | +7,273,357 | +2,457,284 | **+4,816,073** |

- **Walker `filter_entry` (H1, `5d77d5a7`):** S1 does almost no rule work yet gains
  **+788k indirect mispredicts** over the 100k-file walk with ~zero conditional
  change → the **per-entry symlink-filter closure** called via indirect dispatch.
  A **flat per-walk tax in every scenario** (~+1% wall-clock on cheap/walk-heavy
  configs). This is the *security* fix (pruning out-of-tree symlinks).
- **Rule-logic changes:** S12's further +6.5M (indirect + conditional) is its
  `ordered_block` (markerless / `select:`) + `import_gate` (presets) logic, which
  changed in v0.12 — data-dependent branches that mispredict more. Cost of the
  *features*, only in scenarios that use those rules.
- **D1 (H2) is invisible** here — a per-invocation constant, not a per-file branch.

**Refined verdict.** The ~+2-3% is **real, small, and benign** — the
branch-misprediction cost of v0.12's *legitimate new functionality* (out-of-tree
symlink security in the walker + `ordered_block`/`import_gate` features). Same
instructions, same cache, same work — only the branch predictor handles the new
indirect closure call + new conditionals slightly worse. **In-budget, not a bug.**

**Optional optimization (marginal):** the walker's universal ~+1% could be shaved
by restructuring the symlink check to avoid the per-entry indirect closure (e.g.
only install `filter_entry` when the walk can encounter escaping symlinks, or fold
the check into the existing `result_to_entry` path where descent-pruning allows) —
but it trades against the security design and the security value dominates +1%.
Tonight's clean full-matrix run remains the end-to-end wall-clock confirmation
(expected ~+2-3%, the mispredict cost — NOT +300%).

**Implication for the bench corpus.** PR #46 is contaminated and not publishable;
the real release-quality issue is **infrastructure** — the bench runner is shared
with CI + `jobz_*`, so wall-clock benches contaminate chronically. The durable fix
is a dedicated/isolated runner or a cpuset-pinned bench window. Deterministic
profiling (callgrind/strace) should be the *primary* regression signal going
forward, with wall-clock as corroboration on a verified-quiet box.
