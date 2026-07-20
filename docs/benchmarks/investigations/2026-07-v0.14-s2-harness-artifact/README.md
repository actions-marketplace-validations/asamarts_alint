# 2026-07 — v0.14.0 S2 "+17.6%" bench-gate failure was a harness artifact, not code

Status: **Resolved — the deterministic Valgrind gate (`det_check`) shows Ir AND
EstimatedCycles flat (±0.4%) between v0.13.0 and v0.14.0 for every scenario,
including S2. The wall-clock inflation is a measurement-harness difference (the
v0.14.0 corpus is the first run through the self-hosted GitHub Actions runner;
the v0.10–v0.13 baselines were measured over a plain SSH session). No code fix.
Remediation is to re-measure the trajectory on one consistent harness.**

## TL;DR

The v0.14.0 `bench-record` PR (#131) failed `xtask bench-gate`: **S2 (existence +
content) `10k full` regressed +17.6% `min_ms`** vs v0.13.0, past the +15% gate,
with the other content-scanning scenarios (S3/S5/S6/S8/S9) up ~+6.5–7.3% and the
filename-only S1 / cross-file S7 flat.

The content-scanning correlation looked like a real per-file cost, and a code
scan even surfaced a plausible smoking gun (a v0.14 read-path change that *looked*
like it dropped a buffer preallocation). **The deterministic gate overturned
both.** `det_check` (the same `alint` CLI over the same synthetic trees, measured
under Valgrind, so load- and harness-immune) shows the **instruction count and
estimated cycles for S2 are identical between v0.13.0 and v0.14.0** (−0.2% at
10k). Same work, same instructions, +17.6% wall-clock → the wall-clock delta is
environmental, not v0.14 code.

The environmental difference is the harness: v0.14.0 is the first release benched
through the newly-registered self-hosted runner (`kbench-bench`), while
v0.10–v0.13 were backfilled by hand over SSH. A runner agent doing concurrent
work (log streaming, job bookkeeping) adds a steady background load that inflates
wall-clock most on the longer, CPU-heavier scenarios and barely touches the short
ones — which is exactly the "content-correlated" pattern that looked like code.

**Consequences:** no v0.14.1 perf fix is warranted (the binary is
instruction-for-instruction identical on the hot path). PR #131's numbers are not
comparable to the v0.10–v0.13 baselines and should not be published as a
regression; re-baseline the trajectory on one harness. A separate real bug was
found on the way: `ci/scripts/det-perf-gate.sh` pins `gungraun-runner` at a
version older than the workspace's `gungraun` library, which breaks the CI
deterministic gate on any post-bump PR (see "Secondary finding").

## Symptom

`xtask bench-gate --results <v0.14.0> --baseline <v0.13.0>` (both on
`linux-x86_64` = kbench):

```
[regression] min_ms vs baseline (gate: ≥10k, +15%)
  FAIL alint S2 10k full   min_ms +17.6%
  ok   alint S2 100k full  min_ms +13.9%
  ok   alint S2 1m full    min_ms +13.5%
  ok   alint S12 1m full   min_ms +8.8%
  ...
bench gate: 1 gating failure(s) — not publishable
```

Quality (within-run CV) PASSED — 100k/1m cells at 0.6% mean CV. So the run was
*stable*, just uniformly shifted up on the content scenarios.

## Full per-scenario wall-clock delta (v0.14.0 vs v0.13.0, `min_ms`, full mode)

The gate only prints the flagged cells; computing every cell is what first
suggested "harness" over "code" (the lift is broad and tracks run length, not any
single rule path):

| Scenario | 10k | 100k | 1M | reads content? |
|---|--:|--:|--:|:--:|
| S1 filename hygiene | +1.9% | −1.2% | −0.1% | no (control) |
| **S2 existence + content** | **+17.6%** | **+13.9%** | **+13.5%** | yes |
| S3 workspace bundle | +6.1% | +6.9% | +6.7% | yes |
| S4 agent hygiene | +7.0% | +0.1% | +0.3% | yes |
| S5 fix pass | +7.5% | +6.8% | +6.5% | yes |
| S6 per-file content | +6.9% | +7.3% | +7.3% | yes |
| S7 cross-file relational | −1.7% | −2.0% | +0.6% | yes (diff path) |
| S8 git overlay | +6.5% | +6.9% | +6.6% | yes |
| S9 nested polyglot | +6.8% | +7.0% | +6.7% | yes |
| S10 scope_filter | +3.6% | +0.7% | +0.7% | mild |
| S11 v0.10 cross-file | +2.8% | +2.2% | +1.6% | mild |
| S12 v0.10 per-file | +8.1% | +8.6% | +8.8% | yes |
| S13 v0.10 single-shot | +2.4% | −0.1% | +0.3% | mild |
| S14 v0.12 featureset | −1.6% | −2.0% | −1.8% | mild |

Pooled mean +4.4%, median +6.3%. A real code regression concentrated in one path
would spike a couple of related cells and leave the rest at ~0; instead a broad
band of unrelated content scenarios sits at ~+6.5% with S2 highest. That is the
shape of a steady background cost, not a hot-path change.

## Diagnosis (what was tested, in order)

| # | Hypothesis | Verdict / killed by |
|---|---|---|
| 1 | Transient contamination on kbench during the run | Partly — but kbench was idle at diagnosis time (load 0.00), CV was low (0.6%), and the lift was reproducible in the committed numbers, so not a one-off blip. |
| 2 | The kbench ACPI GPE storm (a known contamination source, ~1630 int/s burning a core) was firing during the bench | **Ruled out.** `gpe61` reads `enabled masked` and is frozen (0/s) — the kernel auto-masked it after an early-boot storm; it was not firing at bench time. |
| 3 | A real per-content-scan code regression from the W-series security cycle | **Killed by the deterministic gate (row 5).** Looked strong: the lift tracks content scanning, and a code scan found a read-path change (below). |
| 4 | The subagent's "smoking gun": v0.14's `c845f7d3` (crash/FIFO hardening) switched every content read from `std::fs::read(&abs)` to `read_capped_or_skip` → `read_bounded`, which reads into a zero-capacity `Vec::new()` via `Take::read_to_end` — *looks* like it drops `std::fs::read`'s size preallocation | **Wrong.** Plausible on paper, but `Read::read_to_end` on a `File`/`Take<File>` is *specialized* in std to reserve capacity from the file's remaining length, so `Vec::new()` costs no extra reallocations. Proven by row 5: if it added reallocs/memcpys the instruction count would rise; it did not. |
| 5 | **Harness / environment difference, not code** | **Confirmed.** `det_check` under Valgrind (load- and harness-immune) shows Ir and EstimatedCycles flat ±0.4% across all scenarios incl. S2 (see Evidence). Same instructions + same estimated cycles + +17.6% wall-clock ⟹ the wall-clock delta is not in the binary. |

The load- and harness-immunity of the deterministic gate is the whole point: it
measures instructions executed, not time, so a busier runner or a different
measurement session cannot move it. It is the ground truth the wall-clock
`bench-gate` cannot be (this is the design premise in
[`../../../design/deterministic-perf-gating.md`](../../../design/deterministic-perf-gating.md),
and the same call the [`2026-06-v0.12-perf-validation/`](../2026-06-v0.12-perf-validation/)
investigation made).

## Evidence: deterministic `det_check`, v0.14.0 vs v0.13.0

`det_check` runs the real release `alint` CLI over `gen-monorepo` trees for
S1/S2/S6/S7/S12 at 1k/10k under Valgrind. Measured v0.13.0 (`9a341559`, with
`gungraun-runner` 0.19.1) and v0.14.0 (`e77a0074`, with 0.19.3) separately and
diffed the absolute counts (raw data in [`det-check-ir.md`](det-check-ir.md)):

| Scenario | Ir Δ | EstimatedCycles Δ | note |
|---|--:|--:|---|
| s1_1k | −0.2% | −0.2% | control (no content read) |
| s1_10k | −0.1% | −0.1% | control |
| **s2_1k** | **+0.4%** | **+0.4%** | content; wall-clock was elevated |
| **s2_10k** | **−0.2%** | **−0.2%** | content; **wall-clock was +17.6%** |
| s6_1k | −0.1% | −0.1% | content |
| s6_10k | −0.2% | −0.2% | content |
| s7_1k | −0.4% | −0.3% | |
| s7_10k | +0.3% | +0.4% | |
| s12_1k | +0.2% | +0.2% | |
| s12_10k | +0.1% | +0.1% | |

Everything inside measurement noise. In particular **S2 at 10k — the failing cell
— is −0.2% Ir and −0.2% EstimatedCycles.** Ir scales linearly in file count, so a
flat 10k result implies flat 100k/1M as well. There is no per-file, per-byte, or
cache/memory regression in v0.14.0.

## Root cause of the artifact

The v0.10–v0.13 macro corpus was **backfilled by hand over SSH** on kbench (the
`$SCRATCH/laptop-*.sh` scripts), with nothing else running on the box. v0.14.0 is
the **first release benched through the self-hosted GitHub Actions runner**
(`kbench-bench`), registered as part of the same re-baseline. The runner's agent
processes (`Runner.Listener` + the job's log/upload machinery) do steady
concurrent work throughout the ~3.5 h run. On a 4-core box that steady load
inflates wall-clock, and it inflates the **longer, more CPU-bound scenarios most**
(S2/S3/S5/S6/S8/S9/S12) while barely touching the short filename-only scan (S1) —
producing the "content-correlated" pattern that impersonates a hot-path
regression. The instruction count is untouched, which is why the deterministic
gate is flat.

(A compounding environmental factor cannot be fully excluded after the fact: if
the auto-masked GPE storm fired for part of the v0.14 run but not the earlier
manual runs, it would add the same kind of uniform CPU-bound inflation. Either
way the remedy is the same — measure the whole trajectory under one verified-quiet
harness.)

## Remediation

1. **Do not publish PR #131 as a regression.** Its numbers are not comparable to
   the manual-SSH baselines. Either:
   - Re-run the v0.14.0 macro matrix and re-baseline v0.10–v0.13 through the *same*
     harness (the GitHub Actions runner, now canonical) so the trajectory is
     internally like-for-like again; or
   - characterize #131 with a note that its absolute numbers carry a harness offset
     vs the hand-measured predecessors, and let the next release (measured the same
     way) restore a clean cross-version comparison.
2. **Reduce the runner's own footprint during a bench** so the GHA-runner harness
   matches the quiet-box assumption: give the bench job the box to itself and/or
   `nice`/`ionice` the runner agent while a `bench` job runs. Documented as a
   follow-up in [`../../../design/v0.14/bench-host-migration.md`](../../../design/v0.14/bench-host-migration.md).
3. **Trust the deterministic gate as the release regression signal.** The
   wall-clock `bench-gate` is characterization on a verified-quiet box only; a
   wall-clock "regression" it flags is contamination-until-proven — confirm with
   `det_check`/`det_engine` (Ir) before treating it as real. This is already the
   documented policy in `RELEASING.md`; this investigation is the worked example.

## Secondary finding: `det-perf-gate.sh` gungraun-runner pin is stale (real CI bug)

Running the deterministic gate exposed a genuine defect. `ci/scripts/det-perf-gate.sh`
hardcodes `GUNGRAUN_VERSION=0.19.1` and installs that `gungraun-runner`, but the
workspace bumped the `gungraun` *library* to 0.19.3. The runner refuses to drive a
newer library (`gungraun-runner (0.19.1) is older than gungraun (0.19.3)`), so the
`det_check`/`det_engine` bench exits non-zero — and the script interprets any
non-zero exit as **"Ir/branch regression vs base"**. On CI this means the
deterministic perf-gate (advisory today) mis-reports a *tooling* failure as a perf
regression on every PR after the gungraun bump, and would hard-fail if
`DET_PERF_ADVISORY=0` were ever set.

Two fixes, both worth doing:
- Bump `GUNGRAUN_VERSION` to track the `gungraun` library version (0.19.3), ideally
  derived from `Cargo.lock` rather than hardcoded so it can't drift again.
- Distinguish a bench *tooling* error (non-zero exit with no comparison produced)
  from an actual regression, so a broken runner never masquerades as a regression.

Note for anyone reproducing this locally: because v0.13.0 needs runner 0.19.1 and
v0.14.0 needs 0.19.3, no single runner version drives both checkouts through
`det-perf-gate.sh`'s save-baseline/compare flow. Measure each tag with its own
matching runner and diff the *absolute* `det_check` counts (what this
investigation did), rather than relying on gungraun's built-in baseline compare
across the bump.

## Files

- [`det-check-ir.md`](det-check-ir.md) — the raw `det_check` absolute Ir +
  EstimatedCycles for v0.13.0 and v0.14.0 (S1/S2/S6/S7/S12 × 1k/10k), the numbers
  the Evidence table is computed from.

## Reuse

- Compute the *full* per-scenario delta before trusting a single flagged cell:
  `xtask bench-gate` prints only the failures, but the uniform-vs-targeted shape of
  the whole matrix is the fastest triage for code-vs-environment.
- A kbench GPE storm **auto-masks** — the counter freezes, so a low current reading
  doesn't prove it was quiet earlier. Check `cat /sys/firmware/acpi/interrupts/gpe61`
  for the `masked` flag and whether the kernel cmdline (not just `/etc/default/grub`)
  carries `acpi_mask_gpe=0x61`; the mask only applies after a reboot.
- A plausible code diff is not proof. `read_to_end` on a `File`/`Take<File>`
  preallocates via a std specialization, so `Vec::new()` there is not the
  regression it looks like. When wall-clock and a code lead disagree with a flat
  deterministic gate, the deterministic gate wins.
