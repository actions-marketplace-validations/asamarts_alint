# 2026-05 — bench gate miscalibration (not runner instability)

Status: **Resolved — methodology fix.** Original hypothesis
(self-hosted runner degraded) was **disproven** by a full
cross-version corpus analysis. Root cause: the `RELEASING.md`
bench-record CV gate is a miscalibrated, never-enforced proxy;
the host is fine. Dir name kept (`…-runner-instability`) so the
PR #32 backlink stays valid; the runner-instability framing below
is retained only as the superseded first read.

## TL;DR

- The bench host is **not** degraded. Fingerprint
  (kernel/rustc/RAM/fs/hyperfine/CPU) is **byte-identical** across
  the cleanly-merged v0.9.21 / v0.9.22 and the "failing"
  #31 / #32.
- "Cells with within-run CV > 10%" is **chronic across the entire
  shipped history** — every published v0.9.x release had 7–16
  such cells. The `RELEASING.md` step-1 gate ("re-run if any cell
  CV > 10%") was **never met by any released run** and was never
  enforced in code (it is a human eyeball; `compare.rs` gates
  criterion micro-benches only).
- #31 (7 high-CV cells) is *better* than the median shipped
  release; #32 (12) is normal. Holding them was a misdiagnosis
  caused by taking the written gate at face value without
  checking whether it had ever held.
- Fix is methodological: gate on the statistics the corpus proves
  are reliable, and stop gating on the one it proves is not.

## Evidence

### 1. High within-run CV is chronic, not new

Cells failing the literal `stddev_ms/mean_ms > 0.10` gate, per
**published (merged)** release:

| v0.9.9 | .10 | .11 | .12 | .14 | .16 | .17 | .21 | .22 | **#31** | **#32** |
|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| 13 | 15 | 12 | 13 | 13 | 16 | 14 | 11 | 8 | **7** | **12** |

All the numbered columns shipped. The gate as written has no
historical precedent of being satisfied.

### 2. The noise is a fixed absolute floor ÷ a tiny mean

1k+10k `stddev_ms`, across clean and "failing" runs alike:

| | v0.9.20 | v0.9.21 | v0.9.22 | #31 | #32 |
|---|--:|--:|--:|--:|--:|
| median stddev (ms) | 0.91 | 1.06 | 1.00 | 0.99 | 1.03 |
| max stddev (ms) | 12.3 | 16.4 | 18.0 | 15.0 | 19.6 |

The jitter floor is constant run-to-run. CV "explodes" only
because small/fast cells (10–95 ms) divide that fixed jitter by a
tiny mean. This is a measurement-floor artifact, not host drift.

### 3. Fingerprint identical — no environmental regression

`Linux 6.1.0-42-amd64`, `AMD Ryzen 9 3900X` (24 threads, 62 GB),
`ext4`, `rustc 1.95.0`, `hyperfine 1.15.0` — **the same** for
v0.9.21, v0.9.22, #31, #32. Nothing in the host environment
changed between the cleanly-merged releases and the held runs.

### 4. `min_ms` is reliable where `mean_ms` is not

Cross-version reproducibility (stdev/mean of the per-version
series, by size):

| statistic | 1k | 10k | 100k | 1m |
|---|--:|--:|--:|--:|
| `mean_ms` | 13.4% | 8.8% | 2.7% | 3.4% |
| **`min_ms`** | 11.9% | **2.7%** | **2.7%** | **2.8%** |
| `median_ms` | 12.3% | 4.0% | 2.6% | 3.0% |

At ≥10k, `min_ms` is as reproducible cross-version as the
headline sizes (~2.7%). `mean_ms` is not (8.8% at 10k). 1k is
unreliable at *every* statistic (~12%) — below this harness's
measurement floor (per-cell synthetic-tree regen + `git
init/commit` + page-cache state per
[`METHODOLOGY.md`](../../METHODOLOGY.md); variance *between*
hyperfine invocations that more `--runs` cannot remove).

### 5. Regression detection is unaffected

`S3 1m full` `min_ms`: v0.9.4 = **726,819 ms** (the pre-fix 731 s
regression) → v0.9.9 = 13,210 → … → v0.9.22 = 11,475 → #31 =
11,501 → #32 = 11,774. Stable-era consecutive deltas: −8.2%,
−5.8%, +0.5%, +0.2%, +2.4%. A real regression is a 10–6000%
move in the rock-stable large cells; any sane gate catches it
with enormous headroom.

## Root cause

The bench review gate uses **within-run `mean`-CV on every cell**
as the publish criterion. For cells whose runtime is below the
harness's per-invocation setup-noise floor (all 1k, most 10k),
`mean` absorbs every contention/setup outlier and CV is
chronically 15–95% — independent of host health, and present in
every shipped release. The corpus's actual purpose
(cross-version regression detection) is served by `min_ms` on
≥10k cells, which the data shows is reproducible to ~2.7%. The
gate measures the wrong statistic on the wrong cells.

## Resolution: the evidence-derived two-part gate

Validated against the full corpus (v0.5.7 → v0.9.22 + #31 + #32):
passes every cleanly-merged release v0.9.13→v0.9.22 **and**
#31/#32, while still flagging the v0.9.4-era regression.

1. **Quality gate (is this run trustworthy?)** — per-cell
   within-run CV ≤ 10%, applied **only to cells with mean ≥
   150 ms (100k and 1m)**. 1k and 10k are **advisory**: always
   recorded and reported, never block. Rationale: 100k+ within-run
   CV is reliably < 4% historically (a spike there is a real
   signal); ≤10k within-run CV is chronic measurement-floor noise
   proven across 14 merged releases.
2. **Regression gate (did perf regress?)** — `min_ms`
   cross-version delta vs the previous published release, on
   headline cells of size ≥ 10k, threshold **+15%** (regressions
   only; improvements never gate). Silent across the entire
   stable era; +98%/−98% on the v0.9.4↔v0.9.5 regression.

The published `HISTORY.md` / alint.org trajectory **stays
`mean ± stddev`** for historical continuity (every existing row
and the hardcoded v0.5.6 baseline are mean-based; restating the
public corpus on `min_ms` is a separable, externally-visible
change, deliberately out of scope here and documented in
`METHODOLOGY.md`). The *gate* uses `min_ms`; the *published
table* keeps mean — a standard split (gate on the robust
statistic, publish the full distribution).

Implemented as `xtask bench gate` (replaces the unenforced human
eyeball); see `RELEASING.md` step 1.

## Host checklist (real items, none causal)

Not the cause of the noise, but surfaced and worth fixing:

1. **hyperfine pin not actually applied (bug).** Fingerprint
   reports `1.15.0`; `bench-record.yml` pins `1.20.0` but installs
   it only `if ! command -v hyperfine` — the runner has a
   pre-existing `1.15.0`, so the pin is skipped. Fix: force the
   pinned version unconditionally (reproducibility defect).
2. **Runner EOL 2026-05-20.** Both runs logged `Runner Version
   2.332.0 will no longer be able to run jobs on May 20, 2026`.
   Hard forward deadline; update the agent independent of this.
3. **S10/S9 1k `changed` (advisory-only, noted):** even `min_ms`
   moved ~35% between #31 and #32 (same binary). Localised to the
   1k `changed` path (tiny changed-set, git-diff/FS-dominated).
   Documented; excluded from the gate once 1k is advisory; not
   worth blocking.
4. *(Optional)* governor=`performance` + core-pinning the bench
   would tighten advisory small cells; not required for the gate.

## Disposition

- **#32 is mergeable** under the new gate (quality: zero 100k+
  CV failures; regression: max `min_ms` headline delta vs v0.9.22
  is +2.4%). v0.9.23 bench numbers are as publishable as every
  prior release's.
- The v0.9.23 *release* was never affected (binaries / crates.io /
  npm / Docker / Homebrew shipped via `release.yml 25978547475`);
  only the *benchmark publish* was deferred by the misdiagnosis,
  now resolved.

## Superseded first read (kept for the trail)

The initial commit of this file (98c2c8aa) hypothesised a
contended/degraded self-hosted host from two consecutive
gate-failing runs. That hypothesis was disproven by the
cross-version corpus analysis above: the failures are chronic and
methodological, the fingerprint is invariant, and the host is in
the same state it was for every cleanly-merged release. The
lesson: a written gate that has never been met is evidence about
the gate, not the system under test — check the corpus before
trusting the threshold.
