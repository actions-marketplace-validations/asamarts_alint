# Benchmark host migration: 3900X dev box -> kbench (2026-07-15)

Status: **In progress.** Decision made 2026-07-15: `kbench` (a dedicated,
quiet Intel i7-6700HQ laptop) becomes THE canonical alint benchmark host; the
3900X dev box is retired from benching (it was contended daytime, which
repeatedly contaminated CI bench-record runs). Fingerprint + rationale:
`docs/benchmarks/investigations/2026-07-1m-writeback-contention/` and the
project memory note.

This is a deliberate **re-baseline**, not a continuation: the methodology
(`METHODOLOGY.md`) holds that absolute numbers are not comparable across
machines. kbench is ~2-3x slower per-core than the 3900X, so every published
number changes. We re-anchor cleanly by rebuilding the WHOLE recent trajectory
on kbench.

## What has to change (and status)

1. **`bench-record.yml` retargeted.** `runs-on` -> `[self-hosted, linux, bench]`
   (kbench), not `[self-hosted, linux, alint]` (the 3900X CI runner). The macro
   step sets `ALINT_BENCH_DROP_CACHES=1` (kbench's 16 GB needs the page-cache
   fix or S2/1m fails the gate) and `TMPDIR=/bench` (its `/tmp` is a 7.8 GB
   tmpfs that overflows at 1M; `/bench` is ext4 on NVMe). **DONE.**

2. **Register kbench as a self-hosted runner** with the `bench` label, native
   (not a container: `drop_caches` must hit the host page cache, and the job
   needs kbench's passwordless sudo). Reboot-persistent via a systemd service.
   **STAGED** (`$SCRATCH/laptop-12-register-runner.sh`), deferred until the
   backfill finishes so the install doesn't contaminate live measurements.

3. **Backfill the recent releases on kbench.** v0.10.0, v0.10.1, v0.10.2,
   v0.11.0, v0.12.0, v0.13.0 (v0.11.1 stays skipped — engine unchanged), each
   grafting the drop_caches harness onto the tag and benching the tag's own
   alint binary, all with the fix flag. **RUNNING** (~21 h).

4. **Publish + re-baseline the corpus.** When the backfill is gate-green:
   archive the 3900X series (`macro/results/linux-x86_64/` and the micro
   equivalent) under `archive/`, then publish the kbench results into
   `linux-x86_64/<tag>/` so the live series is fully kbench and internally
   consistent. Each `index.md` fingerprint records the i7-6700HQ + notes the
   drop_caches flag as a measurement condition. **PENDING** (behind the
   backfill).

5. **Regenerate `HISTORY.md` + the trajectory** from the kbench corpus, and
   reconcile the hardcoded v0.5.6 baseline / the `coverage_audit_benchmarks_trajectory`
   expectations (both were 3900X). alint.org's `/benchmarks/` trajectory then
   reflects kbench. **PENDING.**

6. **rustc pinned to 1.97.0.** rustc is part of the bench fingerprint, so the
   whole kbench series must stay on one toolchain. `bench-record.yml` now pins
   `dtolnay/rust-toolchain@1.97.0` (was `@stable`), matching the backfill.
   CI stays on `@stable` (it tests current stable); only bench is pinned. Bump
   deliberately + re-baseline when the fleet moves, never via `stable` drift.
   **DONE.**

## Ordering constraint

Steps 2, 4, 5 must NOT run while a benchmark is measuring on kbench (they'd
contaminate the numbers, defeating the dedicated-host point). The backfill (3)
is the gate: it runs to completion first, then 2/4/5 execute on the idle box,
then v0.14 is tagged (which fires the retargeted bench-record on kbench).
