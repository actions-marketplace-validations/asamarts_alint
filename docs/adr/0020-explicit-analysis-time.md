---
status: accepted
date: 2026-10-16
decision-makers: asamarts
---

# 0020. Explicit analysis time

## Status

Accepted (2026-10-16). This supersedes ADR-0003 and constitution invariant 1 only where their
phrase "same input" left ambient time unspecified; their dispatch, ordering, and other determinism
decisions remain in force.

## Context

The determinism guarantee says reports, violations, and fixes are byte-identical across runs on the
same input. `git_blame_age` nevertheless used `SystemTime::now()`, so a repository could pass one
day and fail the next without any repository or command change. The general-linter program chose a
global explicit time input in ADR-0019 decision 11, but the constitution still needs an enforceable
definition of time as input.

Time-dependent checks also need a useful default. Requiring a date on every local run is reliable
but needlessly hostile; defaulting to the workstation clock is convenient but non-reproducible.
Repository history already provides an immutable time tied to the state being analyzed.

## Decision

Analysis time is an explicit deterministic input.

1. A global `--as-of <YYYY-MM-DD>` CLI flag and `ALINT_AS_OF` environment variable provide it.
   The flag wins over the environment. Dates are strict Gregorian UTC dates and denote midnight at
   the start of that date.
2. Without either input, a time-dependent rule uses the analyzed repository's HEAD committer
   timestamp. The engine resolves it once and shares the immutable value with every rule and pass.
3. If no explicit input exists and HEAD time cannot be resolved, a time-dependent rule reports an
   analysis failure. It must not fall back to wall time or silently pass.
4. Every time-dependent kind declares an `analysis_time` capability. Analysis code must not read
   ambient wall time directly.
5. User-visible explanations and findings identify the effective UTC reference date.

Constitution invariant 1 therefore reads: output is byte-identical across runs given identical
repository/configuration inputs and identical explicit environment/CLI inputs. Time-dependent
analysis receives time only through the engine's analysis-time value; absent an explicit value, the
immutable HEAD committer timestamp is part of repository input. Ambient wall time is not analysis
input.

## Consequences

The same commit produces stable time-dependent output on different days and machines. Historical
runs can be reproduced with `--as-of`, and dirty-tree checks remain anchored to a known commit.
CI that wants calendar aging rather than commit-relative aging must provide its policy date
explicitly.

An uncommitted repository without HEAD can still run time-dependent checks by supplying `--as-of`.
Without it, incomplete analysis is visible rather than green. Adding a time-dependent kind requires
both capability metadata and use of the context value, which the conformance and source-audit gates
check.

A date-only input cannot express sub-day policy. This is accepted: current age policy is measured in
whole days, and one strict UTC representation avoids timezone and locale ambiguity. A future need
for finer precision requires a new design rather than broadening this syntax silently.

## Considered Options

- **Ambient wall clock:** rejected because output changes with no input change.
- **Require `--as-of` always:** deterministic but poor default ergonomics for ordinary repository
  checks.
- **File modification time:** rejected because checkout tools, archives, and edits rewrite it and it
  is not stable repository history.
- **HEAD author timestamp:** rejected in favor of committer time, which identifies when the exact
  commit entered its current history and is available directly as `%ct`.
- **Local timezone date:** rejected because the same instant would produce different dates across
  machines.
- **RFC 3339 timestamp input:** more precision than the current whole-day policy needs and a larger
  validation surface; a strict date is sufficient.

## More Information

- [`analysis-time.md`](../design/v0.19/analysis-time.md): implementation-level semantics and tests.
- [ADR-0019](0019-general-linter-program-and-plugin-architecture.md): general-linter program,
  decision 11.
- [ADR-0003](0003-rule-engine-dispatch-and-determinism.md): original determinism guarantee.
- [`constitution.md`](../design/constitution.md): invariant 1.
