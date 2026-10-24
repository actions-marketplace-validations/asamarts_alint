# v0.21: Internal relation IR (go / no-go)

Status: Accepted (2026-10-24). The eight decisions in "Decisions" are recorded in
[ADR-0023](../../adr/0023-relation-ir-key-parity-and-plugin-contract-0x.md).
Decisions it builds on: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md)
(DEC-2, DEC-5, DEC-7, DEC-8, DEC-10, DEC-13, DEC-14). Questions it decides: Q4, Q10, Q13, and
the first cut of Q8.
Program: [`../general-linter.md`](../general-linter.md), Phase 2 (section 11.5);
[`../plugin-architecture.md`](../plugin-architecture.md), slice P1.

v0.21 is the go/no-go proof for the general-linter program. It builds the internal relation layer
that Phase 3's selectors and constraints will later expose. It proves that layer on one new kind
and one shadowed port of an existing kind, and designs the plugin contract on the same types.
Nothing public changes shape except one new rule kind: there is no `version: 2`, no new config
syntax, and no plugin host.

It delivers four things together:

1. an **internal relation IR** in `alint-core`: resources, selectors, adapters, typed rows with
   provenance, relations, and limits;
2. **`key_parity`**, a new v1 kind and the first user of the IR (DEC-7): one reference file and
   a glob of peers must declare the same key set, including nested JSON and YAML locale files;
3. a **differential shadow harness** and the first port, `cross_file`'s `equals` relation (the
   path the `cross_file_value_equals` alias uses), with the existing implementation still
   authoritative;
4. the **plugin contract `0.x`** design: internal `Adapter` and `Analyzer` traits that the
   built-in row formats and `key_parity` implement, an injectable registry, and a WIT draft.

`key_parity` also brings three new adapters: TOML rows, and the Android string-resource and
.NET `.resx` XML dialects (V21-3).

It also does the measurement work DEC-13 requires before any IR change can be judged.

## Why these belong in one release

An IR without a user is untested design. `key_parity` without the IR would be the standalone
evaluator DEC-8 rules out, and it would be replaced in Phase 3. The shadowed port is the only
evidence that the IR can carry existing semantics byte for byte, which is what "go" means. The
plugin contract has to be designed against the same row and provenance types, or it will leak
internal layouts across the plugin boundary (the failure mode `plugin-architecture.md` section
4.1 cites).

The stop rule from `general-linter.md` section 11.1 applies to this release. If the ported path
does not become simpler and more consistent, the program stops here; the IR, `key_parity`, and
the measurement work keep their standalone value.

## Starting point

Checked against the code at `6a75eb4d1` (2026-10-24).

- **Extraction is string-only and per-consumer.** `ExtractSpec`
  (`crates/alint-core/src/extract.rs:92`) resolves to `Extract` (`:20`): `Structured` (RFC 9535
  JSONPath over the parsed `serde_json::Value`), `Lines`, `Regex` (capture group 1), or
  `WholeFile`. Results are strings plus a type tag for non-string matches (`ExtractedValues`,
  `:267`), and non-literal entries such as `${VAR}` are dropped silently (`is_non_literal`,
  `:237`). Each consumer enforces its own cardinality: `cross_file`'s `equals` requires exactly
  one source value (`crates/alint-rules/src/cross_file/eval.rs:310`). The users are `cross_file`,
  `registry_paths_resolve`, `file_graph` content edges, manifest-derived scope, and the
  `file_content_matches` fact.
- **Spanned rows exist but nothing consumes them.** v0.19's duplicate-preserving
  `StructuredEntryRow` (`extract.rs:312`, built by `extract_entry_rows` at `:333`) covers JSON,
  YAML, dotenv, INI, and properties; TOML, XML, and HCL return `UnsupportedFormat`. Value spans
  resolve lazily through `structured_fix::resolve_value_span`
  (`crates/alint-core/src/structured_fix/mod.rs:50`) for every format but TOML. Two pointer
  types duplicate each other: `PathSeg` (`structured_fix/mod.rs:38`) and `EntryPathSegment`
  (`extract.rs:275`).
- **`cross_file`** is `FullIndex { path_scope: false }`, with seven relations (`Equals`,
  `Subset`, `Superset`, `SetEquals`, `Identical`, `Resolves`, `Registered`;
  `cross_file/spec.rs:86`) and a 10,000-value cap (`cross_file/mod.rs:67`).
  `cross_file_value_equals` is one of the 11 aliases (`crates/alint-rules/src/lib.rs:810`).
- **Dispatch.** `KindMeta` declares four classes (`crates/alint-core/src/kind_meta.rs:17`); the
  102 canonical kinds split 56 per-file, 22 full-index, 14 rule-major scoped, and 10 single-shot.
  At run time there are two partitions: a rule-major parallel loop for every rule without a
  per-file form, and the file-major `run_per_file` loop
  (`crates/alint-core/src/engine/run.rs:714`) that shares one read per file (ADR-0003).
- **Baseline identity** is frozen by the golden gate
  (`crates/alint-e2e/tests/fingerprint_golden.rs`): 526 fingerprints over the 485 scenario files
  plus the v1 bundled-ruleset tests (DEC-5).
- **The v0.12 corpus is not replayable yet.** Its 110 configs live in
  `docs/design/v0.12/corpus/`, pinned in `docs/design/v0.12/case_study_repos.md` by 10-character
  SHA prefixes, which GitHub will not fetch by. Nothing reads the directory today.
- **Performance gating does not meet DEC-13.** The deterministic gate
  (`crates/alint-bench/benches/det_check.rs`, scenarios S1-S3) is advisory in CI and compares
  each pull request with its merge base. `docs/benchmarks/ANCHOR` names v0.20.0 with a +10%
  budget, not v0.18.2 with +3%, and no instruction counts are stored for v0.18.2. Macro results
  for v0.18.2 exist (`docs/benchmarks/macro/results/linux-x86_64/v0.18.2/`). S3 already reads
  `crates/*/Cargo.toml` from at least three rules, which is the shape section 9.1's coalescing
  scenario asks for.
- **Plugin groundwork.** `witforge` (`asamarts/witforge`, private) has its subset checker and
  wire lockfile (`559d82e`, 2026-10-14). Its Rust and protobuf emitters, slice P0, are not
  written. Every runtime path calls `builtin_registry()` directly, `RuleBuilder` is a function
  pointer, `Format` is a closed enum, and `ExtractSpec` has one field per format
  (`plugin-architecture.md` section 11).
- **`key_parity` has no code.** Flat-format parity is the cookbook's `env-key-parity` recipe, a
  `cross_file` `set_equals` over a `regex:` key extract.

## Scope

### Relation IR

A new internal module, `alint_core::relation`. It is not part of the public API: hidden from
docs and carrying no semver promise until the v1.0 freeze. It defines:

- **`Resource`**: a file in the run's index, identified by its index entry, with its normalized
  path. Paths compare byte for byte after normalization on every operating system; case folding
  is an explicit per-comparison opt-in (V21-2, Q13).
- **`Selector`**: a compiled glob plus the existing scope gates, filtering the index. It is a row
  filter, never a path scope on a full-index rule (constitution invariant 2 stands; Q2 is Phase 3).
- **`Adapter`**: turns a resource's bytes into a row stream. The built-in row adapters wrap
  `extract_entry_rows` and the existing parsers.
- **`Row` and `Value`**: `null`, boolean, a number that keeps its source lexeme, string, bytes,
  normalized path, lists, and records (V21-1). Domain scalars such as `semver` are added later,
  additively, once several kinds consume them.
- **`Relation`**: a finite, ordered multiset of rows with the canonical order of section 6.2
  (normalized path bytes, byte offset, structural pointer, label). Conversion to a set is an
  explicit operation.
- **Provenance**: eager resource id and structural pointer; byte and line ranges resolved lazily,
  only for failing rows, through `resolve_value_span`. `PathSeg` and `EntryPathSegment` merge
  into one pointer type.
- **Absence and cardinality**: missing, explicit `null`, and an extraction error are distinct row
  states. Every extractor declares its cardinality. A parse failure or limit hit is an analysis
  failure under DEC-2, never an empty relation.
- **Limits** (section 9.5): rows per extractor, total rows per run, distinct values, and
  equality-join inputs and outputs, each with a default and a hard cap.

**Reads.** IR evaluation keeps ADR-0003's guarantee. A file selected by several relations, or by
relations and per-file rules, is read at most once per run, and decoded at most once per
(adapter, resource). This is measured by `read` syscall counts, not instruction counts.

### `key_parity`

```yaml
- id: locales-match-english
  kind: key_parity
  reference: locales/en.json
  peers: "locales/*.json"
  level: error
```

- **Options:**
  - `reference` (one path) and `peers` (a glob; the reference is excluded automatically);
  - `format` (inferred from the extension when omitted, see below);
  - `direction` (`both`, `missing` or `extra`; default `both`);
  - `plurals` (`exact` or `cldr`; default `exact`);
  - `ignore_keys` (key-path globs).
- **Formats (V21-3):**
  - The five row formats: JSON, YAML, dotenv, INI, and Java properties.
  - TOML, through a new lossless row adapter on `toml_edit`. `[nav] home = ...` and
    `nav.home = ...` are the same key. TOML forbids duplicate keys, so one is a parse failure.
  - Android string resources: `<string>`, `<string-array>`, and `<plurals>` elements, keyed by
    `name`.
  - .NET `.resx`: `<data>` elements, keyed by `name`.

  The extension picks the format, except for `.xml`: generic XML has no key model, so an `.xml`
  reference needs `format: android-strings`. HCL and any other format are rejected at config
  load. Both XML dialects parse through the existing guarded XML path (the `roxmltree` 0.20 pin
  and its depth pre-scan).
- **Key model (V21-4):** a key is a leaf path, and arrays are values that are never descended.
  With `plurals: cldr`, a CLDR plural group counts as one key:
  - an object whose keys are all CLDR categories (`zero`, `one`, `two`, `few`, `many`, `other`)
    and include `other` (Rails YAML);
  - sibling keys `<base>_<category>` that include `<base>_other` (i18next JSON).

  Android `<plurals>` is one key in either mode. Under `cldr`, only a wholly missing or extra
  plural key is reported. Checking each locale's exact category set needs per-locale CLDR data
  and is not in v0.21.
- **Findings:** one per missing key, at the peer file, with the reference key as a related
  location. One per extra key, at the peer key. Example message: "`locales/fr.json` is missing
  key `nav.home` (present in `locales/en.json`)". Keys appear in messages; values never do, so the
  redaction rules (DEC-3) are not engaged.
- **Failures:**
  - A missing reference file is a finding that always counts as error (DEC-2).
  - A peer that fails to parse is an analysis failure for that peer.
  - Duplicate keys within one file are not parity violations; they are B1 `no_duplicate_keys`'s
    concern. Rows keep them, and parity compares distinct key paths.
- **Dispatch and `--changed`:** `FullIndex { path_scope: false }`. It runs when the reference or
  any peer changed.
- **Fixes:** none in v0.21. ADR-0017 classifies key insertion as a Suggestion, and stub insertion
  waits for the generic edit plan.
- **Baseline key:** sorted identity fields (peer path, key path, direction), per DEC-5.
- **The usual kind obligations:** `KindMeta` and `TraceMeta`, rules reference and site page,
  firing and silent scenarios, schema, facts, and the README count claims (102 kinds become 103).

### Shadow harness and the first port

- **The port:** `cross_file` with `relation: equals` (what `cross_file_value_equals` resolves to)
  gets a second implementation on the IR.
- **Where the shadow runs:** a test-only `shadow-ir` feature runs both implementations and asserts
  byte-identical findings, messages, locations, notes, analysis failures, and baseline
  fingerprints. It runs in three places:
  - every scenario that uses `cross_file` `equals`;
  - a property-test generator over extract specs and `normalize:` lists;
  - the 110-config corpus at its pinned commits.
- **Corpus replay:** a new `xtask corpus-replay` resolves the pins to full SHAs (committed once),
  fetches each repository into a cache outside the tree, runs both implementations, and reports
  every difference. It runs on the canonical bench host or a scheduled job, never per pull request.
- **Authority:** the existing implementation stays authoritative. Production never runs the
  shadow, so users pay nothing for it (V21-8): the shadow is compiled only under the test-only
  `shadow-ir` feature, and there is no runtime flag. The old code is deleted in v0.22, after one
  release with zero differences (DEC-7).
- **Where replay runs (V21-7):** a `corpus-replay` workflow on the canonical bench host's runner
  (`[self-hosted, linux, bench]`), dispatched before each release that contains IR changes and
  weekly on a schedule, with its cache on that host. The runner executes one job at a time, so a
  replay never overlaps a benchmark. `RELEASING.md` gains the pre-release step.
- **Not in v0.21:** the other six `cross_file` relations, the remaining `ExtractSpec` consumers,
  and manifest-derived scope. Q8's first cut is recorded from what this port shows.

### Plugin contract `0.x` (slice P1)

- **Module (V21-5):** a doc-hidden `alint_core::plugin_api` module defines `Adapter` and
  `Analyzer` traits over contract types (rows, values, provenance, findings, related locations,
  capabilities, limits), never over engine internals. A test fails if the module imports
  anything but `std`, `core`, `alloc`, and its own items. It is not a separate crate in v0.21:
  the published `alint-core` would depend on it, so it would have to be published too, with the
  new-crate bootstrap in `RELEASING.md`. It moves into a published `alint-plugin-api` crate with
  slice P2.
- **Implementors:** the built-in row adapters and `key_parity` implement the traits. "Done"
  follows `plugin-architecture.md` section 10.3: a built-in adapter and analyzer run through the
  traits with no behaviour change.
- **Registry injection:** the CLI's check, fix and validate-config paths and the LSP take an
  injected registry instead of calling `builtin_registry()`. `Engine::new` already accepts any
  `RuleRegistry`.
- **WIT draft:** `alint:plugin@0.1.0` is committed as a design artifact, with one world per role
  (Q10), host-pushed inputs, and no host callbacks. Bindings are generated once witforge's
  emitters exist (decision V21-5).
- **Fixer role:** deferred to slice P2, with plugin fixers.

### Measurement (DEC-13)

- **Two references (V21-6):** v0.18.2 is the blocking reference for IR pull requests, as below.
  `docs/benchmarks/ANCHOR` (v0.20.0, +10%) keeps serving every release's bench gates
  unchanged.
- **Baseline:** record v0.18.2's deterministic instruction counts for S1-S3 on the canonical
  host, and commit them beside the macro results. If `main` already exceeds +3% against them
  before any IR change, report the numbers for a decision before the gate is set.
- **Gate:** add a cumulative mode to the deterministic gate that fails, rather than advises, when
  an IR pull request exceeds +3% against v0.18.2 on S1-S3.
- **Syscalls:** record `read` and `stat` counts (strace) for S1-S4 at 10k files, and fail an IR
  pull request that increases them.
- **Release check:** before the release, measure wall-clock medians (30 runs, caches dropped) and
  peak RSS at 100k and 1M files on kbench against v0.18.2: at most +3% and +10% respectively.

## Decisions

Taken one at a time on 2026-10-24 and recorded in
[ADR-0023](../../adr/0023-relation-ir-key-parity-and-plugin-contract-0x.md), which also lists the
rejected options.

| ID | Question | Decision |
|---|---|---|
| V21-1 (Q4) | Which value types are core? | Primitives with lexeme-preserving numbers, `path`, lists, and records. `semver` and SPDX wait for several consumers; `semver_range` lands as a standalone library first |
| V21-2 (Q13) | How are paths compared in relations? | Byte-exact after normalization on every operating system, with case folding as an explicit per-comparison opt-in. This matches today's `cross_file` `resolves` lookup |
| V21-3 | Which formats does `key_parity` cover in v0.21? | The five row formats, plus TOML (a new row adapter) and two XML resource-bundle dialects: Android string resources and .NET `.resx` |
| V21-4 | What is a key in a nested file? | Leaf key paths, arrays treated as leaves, plus an opt-in `plurals: cldr` that counts a CLDR plural group as one key |
| V21-5 (Q10, P0) | How does the contract proceed while witforge's emitters do not exist? | Hand-written traits now in a doc-hidden `alint_core::plugin_api` module (not a crate: one the published `alint-core` depends on must itself be published), the WIT draft committed, bindings generated when P0 lands, and a published crate with P2 |
| V21-6 (DEC-13) | How does the frozen v0.18.2 reference coexist with `docs/benchmarks/ANCHOR`? | Both: v0.18.2 is the blocking reference for IR pull requests, and the anchor stays for release gates. If `main` is already over +3% against v0.18.2, the numbers come back for a decision first |
| V21-7 | Where does corpus replay run? | On the canonical bench host's runner, before each release with IR changes and weekly |
| V21-8 | Does the shadow run anywhere outside tests? | No: tests and corpus replay only, under the `shadow-ir` feature |

## Non-goals

v0.21 does not add:

- `version: 2`, named selectors or relations, or the generic `constraint` kind (Phase 3);
- a plugin host, a published plugin API crate, or plugin fixers (slices P2 and later);
- `key_parity` fixes, or `placeholder_parity` (D2);
- an HCL row adapter, generic XML rows (only the two resource-bundle dialects land), or
  per-locale checks of CLDR plural categories;
- plan-level `explain --trace` (it needs the Phase 3 plan);
- deletion of any existing implementation (that happens in v0.22, after a zero-difference release).

## Work items and dependencies

| ID | Deliverable | Depends on | Completion evidence |
|---|---|---|---|
| R0 | v0.18.2 instruction and syscall baselines; cumulative blocking gate mode for IR pull requests | none | baselines committed; the gate fails a synthetic +4% change and passes v0.21's base |
| R1 | `alint_core::relation` types, provenance, absence states, limits, merged pointer type | R0 | unit and property tests for canonical order, the three absence states, and every limit |
| R2 | Built-in row adapters on the IR with read coalescing | R1 | `read` counts on S1-S4 unchanged; one decode per (adapter, resource) asserted by a counting test |
| R3 | `key_parity` | R1, R2 (R9 for its formats) | firing and silent scenarios across all eight formats, nested locales, `plurals: cldr` (nested YAML, i18next suffixes, Android `<plurals>`), `.env`/`.env.example`, a reference-only `--changed` edit, complete trace, golden fingerprints |
| R4 | Shadow harness and the `cross_file` `equals` port | R1, R2 | zero differences on every scenario and on the property generator's cases |
| R5 | `xtask corpus-replay`, full-SHA pins, cache, and difference report | none | replays the 110 configs at their pins on kbench; R4 shows zero differences there |
| R6 | `alint_core::plugin_api` traits, its import-boundary test, and registry injection | R1 | a built-in adapter and `key_parity` run through the traits; golden fingerprints unchanged; the boundary test fails on an engine import |
| R7 | WIT `alint:plugin@0.1.0` draft (bindings if P0 has landed) | R6 | witforge's subset checker accepts the draft |
| R9 | TOML row adapter; Android string-resource and `.resx` dialect adapters | R1, R2 | per-adapter unit tests, duplicate and parse-failure cases, the XML depth guard exercised, positions resolved for findings |
| R8 | Docs, changelog, generated artifacts, budget report, go/no-go assessment | R0-R7, R9 | preflight, docs export, generated checks, and the section 9.6 table filled in |

R0 and R5 can start immediately and in parallel. R1 waits for R0's baselines. R3, R4, and R9 can
run in parallel once R2 lands; `key_parity` is built and tested on the five row formats first,
and gains the R9 formats as each adapter lands. If the release runs long, R6 and R7 move to
v0.22 rather than delaying R3 and R4.

## Cross-cutting invariants

1. **No behaviour change for existing configurations.** Findings, messages, exit codes, and
   baseline fingerprints stay byte-identical; the golden gate and the shadow harness assert it.
2. **Fail loud.** A parse failure, type mismatch, or limit hit is an analysis failure, never an
   empty or partial relation that passes (section 6.3).
3. **Read once.** No file is read twice in one run because the IR selected it (ADR-0003); the
   syscall gate asserts it.
4. **Determinism.** Rows, related locations, and truncations follow the canonical order;
   repeated and shuffled parallel runs produce identical output.
5. **Budgets.** The section 9.6 budgets hold against v0.18.2, enforced for IR pull requests.
6. **Internal stays internal.** No IR type appears in `alint-core`'s public API or in JSON
   output. Rows stay visible only through the experimental trace preview (Q7, ADR-0022).
7. **Contract types only.** Nothing crosses the plugin traits except contract types; a test
   fails if `alint_core::plugin_api` imports anything but `std`, `core`, `alloc`, and itself.
8. **The shadow never ships.** The release build's feature set excludes `shadow-ir`, and CI
   asserts it.
9. **Every invariant has a gate.** A fix or feature is complete only when a test asserts its
   invariant.

## Release acceptance

v0.21 is complete only when all of the following are observed:

- `key_parity` passes its scenarios in all eight formats (the five row formats, TOML, Android
  string resources, and `.resx`), including nested JSON and YAML locale files, `plurals: cldr`
  on nested YAML, i18next suffixes, and Android `<plurals>`, `.env` against `.env.example`, a missing reference, an unparsable peer, duplicate
  keys, and an edit to the reference alone under `--changed`. Its trace is complete, and its
  fingerprints are stable across repeated and shuffled runs.
- The `cross_file` `equals` port shows zero differences on every scenario that uses it, on the
  property generator, and on the 110-config corpus replay.
- The S1-S3 instruction counts are within +3% of v0.18.2, `read` and `stat` counts have not
  increased, and the 100k and 1M wall-clock and RSS budgets hold on kbench.
- A built-in adapter and `key_parity` run through the plugin traits with the golden fingerprints
  unchanged.
- The go/no-go assessment is written: whether the ported path became simpler and more consistent,
  with the evidence. If it did not, the stop rule applies and Phase 3 does not start.
- Full workspace, cross-platform, shell, advisory, and preflight checks pass, and generated
  artifacts (schema, facts, docs export, roadmap) regenerate cleanly.

## Risks

1. **witforge's emitters slip.** The hand-written traits (V21-5) keep v0.21 independent of them.
2. **Relation-major evaluation adds reads.** R2's syscall gate catches the v0.18.0 regression
   class before any kind moves.
3. **Corpus replay cost.** It is cached and scheduled, not run per pull request. Repositories
   that disappeared upstream are reported and skipped, not counted as differences.
4. **Size.** Phase 2 is an L-sized phase, and V21-3 adds three adapters to it. The order above
   lets R6 and R7 slip without weakening the go/no-go evidence, and R9 does not block R4.
5. **New XML adapters reopen the parser denial-of-service class.** Both dialects parse through
   the existing guarded XML path; R9's evidence includes the depth guard.
6. **The budget is already spent.** If R0 finds `main` over +3% against v0.18.2 before any IR
   change, the gate cannot be set as written; the numbers go back for a decision (V21-6).

## Next step

Start R0 (v0.18.2 baselines and the blocking gate mode) and R5 (corpus replay) in parallel. R1
starts once R0's baselines are recorded.
