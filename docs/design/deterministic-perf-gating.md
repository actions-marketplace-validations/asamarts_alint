# Deterministic performance gating

**Status:** IN PROGRESS (design approved 2026-06-07)

## Motivation

`docs/benchmarks/investigations/2026-06-v0.12-perf-validation/` proved that
deterministic profiling (instruction / cache / branch counts via Valgrind) is
**load-immune**: it gave an airtight regression verdict in ~30 min on a *busy*
shared box, where the wall-clock `bench-scale` needed a 5-hour quiet window and
*still* got contaminated by co-tenants (v0.11.1 AND v0.12.0). Wall-clock benches
on the shared kbox are chronically unreliable as a regression gate.

**Goal:** make deterministic counts the **primary, automated, per-PR regression
gate**, decoupled from the contaminated self-hosted runner, with minimal drift.
Demote wall-clock `bench-scale` to absolute-throughput *characterization*.

## Decision: adopt `gungraun` (formerly `iai-callgrind`)

`gungraun` (v0.19.x, the renamed `iai-callgrind`) wraps Callgrind + Cachegrind +
DHAT to produce deterministic metrics, is explicitly designed for noisy/CI
environments ("comparable between different systems, negating environment
noise"), runs each bench once (fast), and has built-in regression gates
(`--callgrind-limits` / `--cachegrind-limits` fail on a per-event breach;
`--save-baseline` / `--baseline` for comparison). Hand-rolling callgrind parsing
+ baseline + gate would reinvent it and add drift surface.

(Decisions locked 2026-06-07: (1) adopt gungraun; (2) widen scenario coverage +
add 100k at release time; (3) branch mispredicts ADVISORY with gating only at
very-high deltas.)

## Architecture — two deterministic layers

**Layer 1 — function-level (micro), gungraun library benches**
(`crates/alint-bench/benches/det_engine.rs`):
- `Engine::run` over a fixed in-memory `FileIndex` (dispatch + aggregation; H2 class)
- the walker over a fixed tree (H1 — the +788k indirect mispredicts found in the
  investigation)
- `Scope::matches` / the per-file `evaluate_file` dispatch

**Layer 2 — end-to-end (macro), gungraun binary benches**
(`crates/alint-bench/benches/det_check.rs`):
- `alint check <tree>` under Callgrind + Cachegrind, setup fn materializes the
  fixed `gen-monorepo` tree (reuse `crates/alint-bench/src/tree.rs`, seed `0xA11E47`)
- **Scenario coverage (widened per decision 2):** a broad subset of S1–S14 —
  walk (S1), per-file content (S2, S5, S6), per-file v0.10/v0.12 kinds (S12, S14),
  cross-file (S7, S11), git (S8), polyglot/scope_filter (S9, S10). Per-PR gate runs
  **1k + 10k**; the **100k tier is added at release time** (still load-immune, just
  slower under valgrind).

## Gating policy (mirrors `gate.rs` gating-vs-advisory split)

| Metric (source) | Role |
|---|---|
| **Instruction count `Ir`** (callgrind) | **GATING**, tight (~+1–2%/bench) — the real-work signal; proved the +300% external (+0.08%). |
| **Branch mispredicts** (cachegrind `Bcm`+`Bim`) | **ADVISORY**, with a **hard gate at very-high deltas** (decision 3): report Δ always; fail only when Δ exceeds a high ceiling (e.g. > +50%), so a genuine misprediction blowup still trips while benign feature drift (the +2–3%) does not. |
| **D1 / LL cache misses** (cachegrind) | ADVISORY |
| **Syscalls** (strace, supplementary) | optional check — new per-file syscall (H1 `lstat`) |

## Automation — load-immune per-PR CI gate

New `ci.yml` job `perf-gate` (gated on `changes.outputs.rust`), on a **GitHub-hosted
runner** (load-immune ⇒ no self-hosted quiescence): install pinned valgrind →
`cargo bench --bench det_engine --bench det_check` → gungraun compares vs the
committed baseline → **fails the PR** on an `Ir` breach. Each bench runs once ⇒ fast.
Regressions caught at PR time, deterministically — not 5 h later in a contaminated
tag bench.

## Drift control

`Ir` is byte-stable for a fixed binary + inputs. Every source pinned; baseline
regeneration is an explicit, documented trigger:

| Source | Pin | Regen trigger |
|---|---|---|
| rustc / LLVM | `rust-toolchain.toml` + existing `codegen-units = 1` (deterministic codegen order) | toolchain bump |
| valgrind (cache/branch model) | `ARG VALGRIND_VERSION` in `bench/Dockerfile` + pinned install in the CI job | valgrind bump |
| dependencies | committed `Cargo.lock` | Ir-changing dep bump |
| gungraun + its runner | pinned dev-dep + `cargo install gungraun-runner --version =X` | gungraun bump |
| input tree | seed `0xA11E47` | frozen |

Baselines live committed under `docs/benchmarks/deterministic/<rustc>-<valgrind>/`
(keyed by the pinning fingerprint, reusing the `Fingerprint` struct) so a
toolchain/valgrind bump yields a NEW baseline dir, never a silent shift. The bench
Docker image gains pinned valgrind for reproducibility anywhere.

## Relationship to wall-clock bench

`bench-scale` / `bench-gate` stay for absolute-throughput + cross-tool numbers, but
are **demoted from the regression gate** (contamination-prone). `RELEASING.md` +
the bench-record review updated: deterministic gate = primary regression signal;
wall-clock = characterization, trusted only on a verified-quiet box. Separate track:
a dedicated / cpuset-pinned bench runner so wall-clock characterization is
trustworthy too.

## Phased rollout

1. **Adopt + prototype** — gungraun dev-dep (pinned) + runner; port S1/S6/S12 binary
   benches + 2–3 library benches; first baseline; pin valgrind in the Docker image.
2. **CI advisory** — `perf-gate` runs + reports on PRs but does not fail (~1 week) to
   calibrate the `Ir` limit against real PR noise.
3. **Flip `Ir` to gating** — once calibrated; branch/cache stay advisory (+ the
   very-high branch gate).
4. **Widen scenarios + 100k-at-release**; document + demote wall-clock; optional
   strace syscall check.
5. **Runner-isolation fix** (separate track) — dedicated/cpuset-pinned bench runner.

## Findings / progress

_(filled as phases land)_
