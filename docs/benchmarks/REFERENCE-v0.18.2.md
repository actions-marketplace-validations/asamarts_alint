# The v0.18.2 performance reference

ADR-0019 decision DEC-13 holds the general-linter program to cumulative budgets against the frozen
v0.18.2 release, and ADR-0023 decision V21-6 makes that reference blocking for relation-layer
changes. This file records the reference measurement (v0.21 work item R0) and how the gate uses
it. `docs/benchmarks/ANCHOR` (v0.20.0, +10%) is a separate reference that every release's bench
gates use; it is unchanged.

## The gate

`ci/scripts/reference-perf-gate.sh`, run by ci.yml's `perf-gate-reference` job whenever
`ci/scripts/detect-changes.sh` classifies a pull request or a push to `main` as a relation-layer
change:

- **Instructions.** The job builds the v0.18.2 release binary and HEAD's, and runs both through
  HEAD's `det_check` harness: the same generated trees, toolchain, Valgrind and gungraun runner,
  with only the alint binary differing. Any S1-S3 cell (1k and 10k) more than +3% over v0.18.2
  fails. The numbers below are not a baseline the gate reads: it measures both sides in the same
  job, so neither the host nor the toolchain can drift the comparison.
- **Syscalls.** `read`-, `stat`-, `openat`- and `getdents64`-family counts for the alint process
  and its threads (spawned children excluded, see `ci/scripts/syscall-counts.py`) on the 10k
  trees. A family more than 32 calls over v0.18.2 fails.
- **Not S4.** S4 extends the `versioning@v1` and `toolchain-pins@v1` bundles, which arrived in
  v0.19, so the v0.18.2 binary cannot load it. S4 stays on the per-PR deterministic gate.

Both checks are blocking and count in the CI summary. A gate that cannot run fails as a labelled
tooling failure.

## Recorded measurement (R0)

Measured on 2026-10-24 at `fb9820f5e` on the shared development host (x86-64, Arch Linux, rustc
1.99.1, Valgrind 3.25.1, gungraun 0.20.0), with both binaries built by the same toolchain and run
through the same harness. Instruction counts depend on the toolchain and the C library, so these
are a record, not a baseline.

### Instructions (Valgrind `Ir`, `det_check`)

| Cell | v0.18.2 | main (`fb9820f5e`) | Change |
|---|---:|---:|---:|
| S1 1k | 52,691,903 | 48,983,783 | -7.04% |
| S1 10k | 431,761,845 | 396,279,941 | -8.22% |
| S2 1k | 194,618,264 | 189,117,197 | -2.83% |
| S2 10k | 1,872,799,790 | 1,810,427,475 | -3.33% |
| S3 1k | 2,119,721,372 | 2,115,991,900 | -0.18% |
| S3 10k | 22,383,599,179 | 22,374,765,762 | -0.04% |

`main` is at or below v0.18.2 on every cell, so V21-6's stop condition (the budget already spent
before any relation-layer change) does not apply. S3 has the least headroom: the relation layer's
whole +3% budget is still available there, and nothing more.

### Syscalls (10k trees, alint process only)

| Tree | Family | v0.18.2 | main | Extra |
|---|---|---:|---:|---:|
| S1 | read | 41 | 42 | 1 |
| S1 | stat | 21,298 | 21,297 | -1 |
| S1 | open | 431 | 433 | 2 |
| S1 | dents | 804 | 804 | 0 |
| S2 | read | 19,240 | 19,240 | 0 |
| S2 | stat | 10,895 | 10,894 | -1 |
| S2 | open | 10,030 | 10,031 | 1 |
| S2 | dents | 804 | 804 | 0 |
| S3 | read | 40,048 | 40,048 | 0 |
| S3 | stat | 28,206 | 28,205 | -1 |
| S3 | open | 20,434 | 20,435 | 1 |
| S3 | dents | 804 | 804 | 0 |

The differences are fixed per run: `main` also probes for a `.alint.d` fragment directory, and
some runs (of either binary) read `/sys/devices/system/cpu/online` while others do not, which is
why two identical runs can differ by a call or two. A per-file regression adds thousands of calls
at 10k files, hence the 32-call allowance.

## Validation

- `ci/scripts/test-reference-perf-gate.sh` pins the outcome handling with stubbed tools: pass,
  an instruction breach, syscall growth over and within the allowance, tooling failures, the
  restoration of HEAD's binary, and the child exclusion of the syscall counter.
- A synthetic regression against the saved v0.18.2 measurement, using the gate's own filter and
  limit (`--callgrind-limits='ir=3%'`): a loop added where the walker builds each entry.

  | Synthetic cost per walked entry | S1 1k | S1 10k | Gate |
  |---|---:|---:|---|
  | about 1,600 instructions (+3.6% / +4.2% over `main`) | -2.85% | -4.40% | pass |
  | about 5,400 instructions | +5.22% | +5.14% | fail ("Performance has regressed") |

  The first row shows what a cumulative budget means: `main` starts 7-8% below v0.18.2 on S1, so a
  change costing 4% there still passes. S3 has no such headroom.

## Noise

Instruction counts are not perfectly repeatable for the multi-threaded binary: rayon and walker
threads that spin while idle are counted too. Re-running the unchanged `main` binary moved S3 10k
by 0.73% (22,374,765,762 then 22,211,411,264) and S3 1k by 0.17%; S1 and S2 moved by less. On S3
10k the +3% limit therefore leaves a little over 2% of reliable room. If that produces false
failures, the fix is to pin the thread counts in `det_check`, which needs a walker thread knob as
well as `RAYON_NUM_THREADS`.
