---
status: accepted
date: 2026-10-24
decision-makers: asamarts
---

# 0023. Relation IR, `key_parity`, and plugin contract 0.x

## Status

Accepted (2026-10-24). This records the Phase 2 outcomes for ADR-0019 decisions 7, 8, 10, 13,
and 14, and questions Q4, Q10, and Q13. Q8 is decided later, per kind, from the shadow-port
results.

## Context

ADR-0019 schedules the internal relation IR for v0.21 as the program's go/no-go proof: a pilot
kind on the IR (DEC-7), a differential shadow port of one existing kind, the plugin contract
designed with the IR (DEC-14e), and cumulative performance budgets against v0.18.2 (DEC-13). The
v0.21 design ([`v0.21/README.md`](../design/v0.21/README.md)) checked its starting point against
the code and left eight decisions open. They fix the IR's value and path semantics, the pilot's
scope and key model, how the contract proceeds before witforge can generate code, and where the
measurement and shadow evidence come from. Each one is hard to change once rows, findings, or
the WIT draft are built on it.

## Decision

1. **Value types (V21-1, Q4).** Rows carry `null`, boolean, a number that keeps its source
   lexeme, string, bytes, a normalized repository path, lists, and records. Semantic scalars
   (`semver`, SPDX) are added, as tagged domain scalars, only once several kinds consume them;
   adding one is additive. C4 `semver_range` lands first as a standalone library.
2. **Path comparison (V21-2, Q13).** Paths in relations are forward-slash, root-relative, with
   `./`, `..`, and trailing slashes resolved, and are compared byte for byte on every operating
   system. Case folding is an explicit per-comparison opt-in and never follows the host file
   system. This is what `cross_file` `resolves` already does (an exact index lookup after
   `normalize_confined`), so the shadow port stays byte-identical.
3. **`key_parity` formats (V21-3).** v0.21 covers the five formats that already have spanned rows
   (JSON, YAML, dotenv, INI, Java properties), plus TOML through a new lossless row adapter on
   `toml_edit`, plus two XML resource-bundle dialects: Android string resources (`<string>`,
   `<string-array>`, and `<plurals>` keyed by `name`) and .NET `.resx` (`<data>` keyed by
   `name`). Generic XML has no key model and is rejected at config load, as are HCL and any other
   format. The XML dialects parse through the existing guarded XML path.
4. **Key model (V21-4).** A key is a leaf path; arrays are values and are not descended. An
   opt-in `plurals: cldr` option (default `exact`) treats a CLDR plural group as one key: an
   object whose keys are all CLDR categories (`zero`, `one`, `two`, `few`, `many`, `other`) and
   include `other`, or sibling keys `<base>_<category>` that include `<base>_other` (the i18next
   convention). Android `<plurals>` is one key in either mode. Under `cldr`, only a wholly
   missing or extra plural key is reported; checking each locale's exact category set needs
   per-locale CLDR data and is not part of v0.21.
5. **Plugin contract path (V21-5, Q10).** The `Adapter` and `Analyzer` traits and their contract
   types are written by hand in a doc-hidden `alint_core::plugin_api` module with no semver
   promise. A test fails if that module imports anything but `std`, `core`, `alloc`, and its own
   items, so engine internals cannot cross the boundary. The built-in row adapters and
   `key_parity` implement the traits. The `alint:plugin@0.1.0` WIT draft is committed with one
   world per role, host-pushed inputs, and no host callbacks, and witforge's subset checker must
   accept it. Bindings are generated once witforge's emitters (slice P0) exist; the module moves
   into a published `alint-plugin-api` crate with slice P2. A separate crate now would have to be
   published, because the published `alint-core` would depend on it, and would need the
   new-crate bootstrap in `RELEASING.md`.
6. **Performance references (V21-6, DEC-13).** v0.18.2 is the blocking reference for IR pull
   requests: S1-S3 instruction counts at most +3%, and no increase in `read` or `stat` syscalls;
   v0.21's release check adds the 100k and 1M wall-clock (+3%) and peak RSS (+10%) budgets on
   the canonical bench host. `docs/benchmarks/ANCHOR` (v0.20.0, +10%) keeps serving every
   release's bench gates unchanged. Work item R0 records v0.18.2's counts first; if `main`
   already exceeds +3% against v0.18.2 before any IR change, the numbers go back to the
   decision-maker before the gate is set.
7. **Corpus replay (V21-7).** `xtask corpus-replay` runs as a workflow on the canonical bench
   host's runner (`[self-hosted, linux, bench]`), dispatched before each release that contains
   IR changes and on a weekly schedule, with its repository cache on that host. That runner
   executes one job at a time, so a replay never overlaps a benchmark. A release with IR changes
   requires zero differences.
8. **Shadow scope (V21-8).** The shadow implementation is compiled only under a test-only
   `shadow-ir` feature, used by the scenario suite, the property generator, and corpus replay.
   Release binaries contain only the authoritative implementation; there is no runtime flag.

The work items, invariants, and acceptance tests are normative in
[`docs/design/v0.21/`](../design/v0.21/README.md).

## Consequences

- The IR's value model is small enough to finish in one release and leaves room for domain
  scalars without a contract break.
- Relation results are identical on Linux, macOS, and Windows. A reference with the wrong case
  is reported as missing even where it opens locally, which is the bug that breaks Linux CI.
- `key_parity` covers the common locale stacks (i18next, Vue, Angular, Rails, Hugo, Java,
  Android, .NET) and `.env` parity at launch. The release grows by three adapters, and each
  needs its own scenarios.
- Pluralized locale sets get a quiet mode without suppressing real misses through
  `ignore_keys`; exact per-locale category checking remains a later option.
- No public crate exists before the plugin contract is stable, and v0.21 needs no crates.io
  bootstrap. The boundary is enforced by a test rather than by the dependency graph until P2.
- IR pull requests cannot accumulate cost unnoticed, while ordinary releases keep the anchor
  that accepted the v0.18-v0.20 costs. Two references are documented side by side.
- A `cross_file` difference that only the corpus exposes is found within a week of merging, and
  always before a release.
- Users pay nothing for the shadow. Differences that appear only in users' own configurations
  stay invisible until v0.22 switches implementations, after a zero-difference release.

## Considered Options

- **`semver` and SPDX scalars in v0.21:** rejected because nothing in v0.21 consumes them, and
  shipping them in the WIT draft early would make removing them a contract change.
- **Keeping string-only values:** rejected because nested records and the distinct missing,
  `null`, and error states do not fit.
- **Following the host file system, or always folding case:** rejected because findings and
  baseline fingerprints would differ by machine, and either would change today's `cross_file`
  results.
- **Only the five existing row formats:** rejected in favour of covering TOML and the two XML
  dialects at launch.
- **Every object path, or descending arrays by index:** rejected because a missing section
  reports once per key under it, and reordered or resized locale arrays would look like drift.
- **Plural handling through `ignore_keys` only:** rejected because ignoring `few` or `many` also
  hides genuinely missing plural forms.
- **Blocking the contract on witforge P0, or deferring it to v0.22:** rejected because the first
  makes v0.21 depend on an unstarted project and the second contradicts DEC-14e.
- **A new `alint-plugin-api` crate now (published as internal, or used only from tests):**
  rejected because the first needs a release bootstrap for an unstable crate and the second
  leaves the built-ins not implementing the traits.
- **Moving the anchor to v0.18.2 at +3% for everything, or re-basing the IR budget on v0.20.1:**
  rejected because the first un-accepts deliberate v0.18-v0.20 costs and the second lets the
  costs of program Phases 0 and 1 escape the program budget.
- **Hosted scheduled replay, or replay on every IR pull request:** rejected because hosted runs
  are unreliable and cache-limited, and per-PR replays would hold the bench runner.
- **An opt-in `ALINT_SHADOW_IR` runtime flag:** rejected because it ships a second
  implementation and a diagnostics surface few users would enable.

## More Information

- [`v0.21/README.md`](../design/v0.21/README.md): scope, work items, invariants, and release
  gates.
- [ADR-0019](0019-general-linter-program-and-plugin-architecture.md): parent program decision.
- [ADR-0003](0003-rule-engine-dispatch-and-determinism.md): read-once and determinism guarantees
  the IR keeps.
- [ADR-0006](0006-baseline-suppression.md): baseline identity the golden gate freezes.
- [`general-linter.md`](../design/general-linter.md) sections 6.2 (relation model), 9.4
  (plugins), and 9.6 (performance budget).
- [`plugin-architecture.md`](../design/plugin-architecture.md): slices P0-P2.
