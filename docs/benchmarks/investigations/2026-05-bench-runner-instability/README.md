# 2026-05 — bench-runner instability (v0.9.23 publish held)

Status: **Open.** v0.9.23 macro bench numbers are **held from the
published corpus** pending a clean run. Opened per
[`RELEASING.md`](../../../../RELEASING.md) bench-record review,
step 4 (drift/quality hand-off before merge).

## Summary

Two consecutive publish-grade `bench-record.yml` runs for the
**same tag** (`v0.9.23` = `42be8a7a`, the released binary) both
fail the RELEASING.md step-1 CV gate (`stddev_ms / mean_ms ≤
0.10`). The second run, on a verified-idle runner, was **worse**
than the first. The fingerprint is valid on both (correct
machine), and the noisy cells are **largely disjoint between
runs** — the signature of a variable/contended *host*, not a bad
scenario or a code regression. v0.9.23 changed **zero engine or
rule code** (action.yml + CI + docs only), so there is no
plausible real perf delta to explain the variance.

## Evidence

| Run | Trigger | Run ID | PR | Wall | Cells CV>10% | Worst |
|---|---|---|---|---|---:|---|
| 1 | tag push `v0.9.23` | `25978547468` | #31 (closed) | ~42 min | **7** | S7 1k full 96.5%, S4 1k full 94.4% |
| 2 | `workflow_dispatch` `ref=v0.9.23 label=v0.9.23` (idle runner) | `25980921624` | #32 (open) | ~41 min | **12** | S10 1k full 74.3%, S6 1k chg 71.7%, S4 10k full 61.3% |

Fingerprint, both runs: `AMD Ryzen 9 3900X 12-Core Processor`,
`os=linux`, `arch=x86_64`, `alint_version=0.9.23` — the canonical
baseline. Not a wrong-machine run.

Archived raw results (local, not committed):
`/tmp/claude-1000/-home-kaminsod-projects-alint/v0923-results.json`
(run 1) and `…/v0923-rerun.json` (run 2).

### Run 2 CV>10% cells (12 of 80)

```
74.3% S10 1k full     71.7% S6  1k changed   61.3% S4  10k full
46.1% S2  10k full    38.7% S1  1k changed   36.4% S10 10k full
32.7% S4  1k changed  30.8% S7  10k changed  19.5% S9  10k full
17.6% S6  10k changed 15.9% S3  10k changed  10.7% S5  10k full
```

Run 1 noisy set: S7 1k full, S4 1k full, S1 10k full, S3 1k full,
S7 10k full, S4 1k changed, S7 10k changed. **Overlap with run 2:
only S4 1k-changed and S7 10k-changed** (2 of 7 / 12). Random
noisy-cell membership across runs ⇒ host-level jitter, not a
deterministic scenario problem.

### The decisive proof: "clean" cells are unreliable run-to-run

Filtering run 2 to cells with per-cell CV ≤ 10% in **both**
v0.9.22 and v0.9.23, two still show a >20% cross-version delta:

- **S10 1k changed: −34.3%** (v0.9.22 19.7 ms → v0.9.23 12.9 ms)
- **S9 1k changed: −30.9%** (v0.9.22 21.7 ms → v0.9.23 15.0 ms)

v0.9.23 has no engine/rule change vs v0.9.22, so a real 30%+
speedup is impossible. These cells pass the *per-run* CV gate yet
are wrong by a third cross-run. Conclusion: on this host right
now, the per-cell CV<10% check on a single run is **not
sufficient** to trust a number — the run-to-run environmental
variance exceeds the within-run variance. Publishing run 2 would
inject a spurious "−34% improvement" into the permanent
cross-version trajectory for a no-op release.

### What is NOT affected

All failures are in **small sizes (1k/10k)**, absolute times
10–95 ms, where host jitter dominates. **No 100k or 1m cell
exceeds 10% CV in either run** — the headline S3 large-size
trajectory (the load-bearing perf-regression signal) is clean
both times. The instability is a fast-cell measurement-floor
problem on a contended host, not a regression in alint.

## Hypotheses (to triage on the host)

1. **Non-CI load on the self-hosted box.** No other GitHub
   Actions run was active during run 2, but the runner may be a
   shared/dev machine with non-Actions load. Highest prior.
2. **CPU frequency scaling / thermal.** Fast cells finish before
   the governor settles; boost-clock variance swamps a 13 ms
   measurement. Check governor = `performance`, disable boost for
   bench, pin to isolated cores.
3. **Runner near EOL.** Both runs logged `Runner Version 2.332.0
   will no longer be able to run jobs on May 20, 2026`. Possibly
   a degraded/transitional host; update the runner and retest.
4. **Warmup/runs insufficient for sub-30 ms cells.** `--warmup 3
   --runs 10` may be too few for the fastest scenarios on this
   host's current state; a small-size-specific higher-runs
   profile may be needed (harness change — last resort, only if
   1–3 are ruled out).

## Decision

- **HOLD**: do not merge PR #32; do not add a `v0.9.23` row to
  `docs/benchmarks/HISTORY.md`. The cross-version corpus stays
  v0.9.22-latest until a clean v0.9.23 run exists.
- **Do not blind-re-run.** Two attempts; the second (idle runner)
  was worse. A third without addressing hypotheses 1–3 is wasted
  runner time.
- Next action is **host diagnosis** (maintainer), then a fresh
  `workflow_dispatch` (`ref=v0.9.23 label=v0.9.23`) and
  re-review against the same gate. When a clean run lands, the
  `HISTORY.md` v0.9.23 row links back to this file per
  RELEASING.md step 4, and this Status flips to Resolved with the
  resolving run ID.

## Note

This does not block the v0.9.23 *release* — binaries, crates.io,
npm, Docker, Homebrew all shipped (release.yml run
`25978547475`). Only the published *benchmark numbers* for
v0.9.23 are deferred; that is the intended decoupling
(`bench-record.yml` is off the `release.yml` dependency graph by
design).
