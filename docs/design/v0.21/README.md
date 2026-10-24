# v0.21: Internal relation IR (go / no-go)

Status: Accepted (2026-10-24). The eight decisions in "Decisions" are recorded in
[ADR-0023](../../adr/0023-relation-ir-key-parity-and-plugin-contract-0x.md).
Decisions it builds on: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md)
(DEC-2, DEC-5, DEC-7, DEC-8, DEC-10, DEC-13, DEC-14). Questions it decides: Q4, Q10, and Q13;
the port's results feed Q8, which is decided per kind later.
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
   built-in row adapters and `key_parity` implement, an injectable registry, and a WIT draft.

`key_parity` also brings three new adapters: TOML rows, and the Android string-resource and
.NET `.resx` XML dialects (V21-3).

It also does the measurement work DEC-13 requires before any IR change can be judged.

## Why these belong in one release

An IR without a user is untested design. `key_parity` without the IR would be the standalone
evaluator DEC-8 rules out, and it would be replaced in Phase 3. The shadowed port is the only
evidence that the IR can carry existing semantics byte for byte, which is what "go" means. The
plugin contract has to be designed against the same row and provenance types, or it will leak
internal layouts across the plugin boundary (the failure mode `plugin-architecture.md` section
2 describes).

The stop rule from `general-linter.md` section 11.1 applies to this release. If the ported path
does not become simpler and more consistent, the program stops here; the IR, `key_parity`, and
the measurement work keep their standalone value.

## Starting point

Checked against the code at `6a75eb4d1` and rechecked at `fb9820f5e` (2026-10-24).

- **Extraction is string-only and per-consumer.** `ExtractSpec`
  (`crates/alint-core/src/extract.rs:92`) resolves to `Extract` (`:20`): `Structured` (RFC 9535
  JSONPath over the parsed `serde_json::Value`), `Lines`, `Regex` (capture group 1), or
  `WholeFile`. Results are strings plus a type tag for non-string matches (`ExtractedValues`,
  `:267`). Non-literal entries such as `${VAR}` are skipped with a note (`is_non_literal`, `:237`;
  `cross_file/eval.rs:298`). Each consumer enforces its own cardinality: `cross_file`'s `equals` requires exactly
  one source value (`crates/alint-rules/src/cross_file/eval.rs:310`). The users are `cross_file`
  and its `sync_from` fixer, `registry_paths_resolve`, `file_graph` content edges, manifest-derived
  scope, and the `file_content_matches` fact.
- **Spanned rows exist but nothing consumes them.** v0.19's duplicate-preserving
  `StructuredEntryRow` (`extract.rs:312`, built by `extract_entry_rows` at `:333`) covers JSON,
  YAML, dotenv, INI, and properties; TOML, XML, and HCL return `UnsupportedFormat`. Value spans
  resolve lazily through `structured_fix::resolve_value_span`
  (`crates/alint-core/src/structured_fix/mod.rs:50`) for every format but TOML. Two pointer
  types duplicate each other, and both are public: `PathSeg` (`structured_fix/mod.rs:38`) and
  `EntryPathSegment` (`extract.rs:275`, re-exported from the crate root).
- **`cross_file`** is `FullIndex { path_scope: false }`, with seven relations (`Equals`,
  `Subset`, `Superset`, `SetEquals`, `Identical`, `Resolves`, `Registered`;
  `cross_file/spec.rs:86`) and a 10,000-value cap (`cross_file/mod.rs:67`).
  `cross_file_value_equals` is one of the 11 aliases (`crates/alint-rules/src/lib.rs:810`).
- **Dispatch.** `KindMeta` declares four classes (`crates/alint-core/src/kind_meta.rs:17`); the
  102 canonical kinds split 56 per-file, 22 full-index, 14 rule-major scoped, and 10 single-shot.
  At run time there are two partitions: a rule-major parallel loop for every rule without a
  per-file form, and the file-major `run_per_file` loop
  (`crates/alint-core/src/engine/run.rs:714`) that shares one read per file (ADR-0003). Only
  per-file rules share that read: `cross_file` reads each file it extracts from on its own
  (`read_capped`, `eval.rs:503`).
- **Baseline identity** is frozen by the golden gate
  (`crates/alint-e2e/tests/fingerprint_golden.rs`): 526 fingerprints over the 485 scenario files
  plus the v1 bundled-ruleset tests (DEC-5).
- **The v0.12 corpus was not replayable.** Its 110 configs (111 repositories were studied; tokio's
  config lives in `examples/`) are in `docs/design/v0.12/corpus/`, pinned in
  `docs/design/v0.12/case_study_repos.md` by 10-character SHA prefixes, which GitHub will not
  fetch by. 54 configs use the `equals` path (89 `cross_file_value_equals` and 8 `cross_file`
  rules in all), and 22 contain rules that run commands. Since R5, `corpus/pins.tsv` holds the
  full SHAs.
- **Performance gating does not meet DEC-13.** The deterministic gate
  (`crates/alint-bench/benches/det_check.rs`, scenarios S1-S3) is advisory in CI and compares
  each pull request with its merge base. `docs/benchmarks/ANCHOR` names v0.20.0 with a +10%
  budget, not v0.18.2 with +3%, and no instruction counts are stored for v0.18.2. Macro results
  for v0.18.2 exist (`docs/benchmarks/macro/results/linux-x86_64/v0.18.2/`). S4 cannot run on
  v0.18.2: it extends the `versioning@v1` and `toolchain-pins@v1` bundles, which arrived in
  v0.19, and it is a revisioned workload that `bench-gate` resets across revisions. S3 reads
  `crates/*/Cargo.toml` from at least three rules, but they are native rules, not relations, so
  no existing scenario exercises section 9.1's relation coalescing.
- **Plugin groundwork.** `witforge` (`asamarts/witforge`, private) has its subset checker and
  wire lockfile (`6ce6076`, 2026-10-24). Its Rust and protobuf emitters, slice P0, are not
  written. The contract subset forbids recursive types (`plugin-architecture.md` section 3.2).
  Every runtime path calls `builtin_registry()` directly (check, fix and validate-config, the
  LSP, `alint test`'s runner, `rules`, `init`, `suggest`, and `export-agents-md`), `RuleBuilder` is a function
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
  `extract_entry_rows` and the existing parsers. A second built-in adapter wraps today's
  `Extract` (every format, plus regex, lines, and whole file) and emits its string values, type
  tags, and notes unchanged; the `equals` port uses it.
- **`Row` and `Value`**: `null`, boolean, a number that keeps its source lexeme, string, bytes,
  normalized path, lists, and records (V21-1). Domain scalars such as `semver` are added later,
  once several kinds consume them: additive inside alint, but a new contract minor version for
  the WIT, where a new variant case is a breaking change (`plugin-architecture.md` section 8.1).
  In memory a `Value` nests; across the contract it is a flat node list with indices, because WIT
  has no recursive types.
- **`Relation`**: a finite, ordered multiset of rows with the canonical order of section 6.2
  (normalized path bytes, byte offset, structural pointer, label). Conversion to a set is an
  explicit operation.
- **Provenance**: eager resource id and structural pointer. Adapters that already know spans
  (entry rows, TOML through `toml_edit`, the XML dialects through node ranges) carry them
  eagerly; rows from the `Extract` wrapper resolve byte and line ranges lazily, only for failing
  rows, through `resolve_value_span`. One internal pointer type replaces both `PathSeg` and
  `EntryPathSegment` inside the layer; both public types stay, converting into it, so the layer
  changes no public API.
- **Absence and cardinality**: missing, explicit `null`, and an extraction error are distinct row
  states. Every extractor declares its cardinality. A parse failure or limit hit is an analysis
  failure under DEC-2, never an empty relation.
- **Limits** (section 9.5): rows per extractor, total rows per run, distinct values, and
  equality-join inputs and outputs, each with a default and a hard cap.

**Reads.** IR evaluation extends ADR-0003's guarantee to relations. A file selected by several
relations, or by relations and per-file rules, is read at most once per run, and decoded at most
once per (adapter, resource). A new relation scenario (key_parity and per-file rules over the
same files) measures this by `read` syscall counts, and a counting test asserts it; S1-S4 contain
no relations, so they cannot.

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
  - `strip_root` (default `false`): when every compared file has exactly one top-level key, as
    Rails locale files do (`en:`, `fr:`), compare below it;
  - `ignore_keys` (key-path globs: `*` within a segment, `**` across segments).
- **Formats (V21-3):**
  - The five row formats: JSON, YAML, dotenv, INI, and Java properties.
  - TOML, through a new lossless row adapter on `toml_edit`. `[nav] home = ...` and
    `nav.home = ...` are the same key. TOML forbids duplicate keys, so one is a parse failure.
  - Android string resources: `<string>`, `<string-array>`, and `<plurals>` elements, keyed by
    (element type, `name`), since a `<string>` and a `<plurals>` may share a name. Entries marked
    `translatable="false"` exist only in the default `values/` by design and are skipped.
  - .NET `.resx`: string `<data>` entries, keyed by `name`. Entries with a `type` or `mimetype`
    (non-string resources) and designer metadata (names starting with `>>` or `$this`) are
    skipped. .NET falls back to the neutral file for missing keys, so `direction: extra` is the
    usual setting for `.resx`.

  These are `key_parity`'s own format names, not new `Format` variants: `Format` is a closed
  public enum whose every variant all format-aware kinds must handle, and neither dialect is a
  general format. `key_parity`'s `KindMeta` lists the general formats it reads (JSON, YAML, TOML,
  dotenv, INI, properties, XML). The extension picks the format, except for `.xml`: generic XML
  has no key model, so an `.xml` reference needs `format: android-strings`. HCL and any other
  format are rejected at config load. Files of different formats may be compared, since keys
  are format-neutral. Both XML dialects parse through the existing guarded XML path (the
  `roxmltree` 0.20 pin and its depth pre-scan).
- **Key model (V21-4):** a key is a leaf path, and arrays are values that are never descended.
  A key's identity is its segment list, so `{"nav.home": 1}` and `{"nav": {"home": 1}}`, or a
  TOML quoted key `"a.b"` and the dotted `a.b`, are different keys. Messages, `ignore_keys`, and
  baseline keys use one rendering: segments joined by `.`, with bracket notation (`nav['a.b']`)
  for a segment containing `.`, `[`, `]`, or a quote. Properties keys are single segments.
  YAML aliases and merge keys (`<<: *defaults`) are an analysis failure in a compared file:
  rows do not expand them, and comparing unexpanded keys would report false drift. With
  `plurals: cldr`, a CLDR plural group counts as one key:
  - an object whose keys are all CLDR categories (`zero`, `one`, `two`, `few`, `many`, `other`)
    and include `other` (Rails YAML);
  - sibling keys `<base>_<category>` that include `<base>_other` (i18next JSON).

  Android `<plurals>` is one key in either mode. Under `cldr`, only a wholly missing or extra
  plural key is reported. Checking each locale's exact category set needs per-locale CLDR data
  and is not in v0.21.
- **Findings:** one per missing key, at the peer file, with the reference key as a related
  location. One per extra key, at the peer key. Example message: "`locales/fr.json` is missing
  key `nav.home` (present in `locales/en.json`)". Keys appear in messages; values never do. The
  experimental trace preview (ADR-0022) shows `key_parity`'s rows as keys only, so no value
  reaches any output and the redaction rules (DEC-3) have nothing to redact.
- **Failures:**
  - A missing reference file is a finding that always counts as error (DEC-2).
  - A reference or a peer that fails to parse is an analysis failure for that file.
  - A `peers` glob that matches no file is an error-level finding: a misconfiguration, never a
    pass.
  - Duplicate keys within one file are not parity violations; they are the concern of the planned
    B1 `no_duplicate_keys` kind. Rows keep them, and parity compares distinct key paths.
  - `respect_gitignore` defaults to `true`, and `.env` is usually ignored, so `.env` parity needs
    `respect_gitignore: false`; the rules reference says so.
- **Dispatch and `--changed`:** `FullIndex { path_scope: true }` with the reference and the peer
  glob as its scope: under `--changed` it runs when the reference or any peer changed, and is
  skipped otherwise. The scope is the rule's whole context, so this does not narrow what it
  compares (section 9.1).
- **Fixes:** none in v0.21. ADR-0017 classifies key insertion as a Suggestion, and stub insertion
  waits for the generic edit plan.
- **Baseline key:** sorted identity fields (peer path, key path, direction), per DEC-5.
- **The usual kind obligations:** `KindMeta` and `TraceMeta`, family assignment, rules reference
  and site page, firing and silent scenarios, schema, facts, and every count claim (102 kinds
  become 103): the README, the site concepts page and its diagram, and the alint.org about page
  that its head-parity check reads.

### Shadow harness and the first port

- **The port:** `cross_file` with `relation: equals` (what `cross_file_value_equals` resolves to)
  gets a second implementation on the IR. It extracts through the `Extract` wrapper adapter, so
  it covers every format `cross_file` accepts and depends on no new adapter (R9). What it proves
  is the relation, comparison, finding, and baseline path on the IR; typed extraction for every
  format is not in v0.21.
- **Where the shadow runs:** a test-only `shadow-ir` feature runs both implementations and asserts
  byte-identical findings, messages, locations, notes, analysis failures, baseline fingerprints,
  fixability, and `sync_from` proposed edits. It runs in three places:
  - every scenario that uses `cross_file` `equals`;
  - a property-test generator over extract specs and `normalize:` lists;
  - the 110-config corpus at its pinned commits.
- **Corpus replay:** `xtask corpus-replay` reads the full-SHA pins, fetches each repository at
  depth 1 into a cache outside the tree, and runs alint over it. Every rule whose kind spawns a
  process is removed from the config first, so no third-party command ever runs; a config that
  still does not load is reported, not silently dropped. With `--compare` it runs a second binary
  and reports every repository whose report differs. Once R4 lands, the shadow comparison runs
  inside the `shadow-ir` build and its differences fail the replay. It never runs per pull
  request.
- **Authority:** the existing implementation stays authoritative. Production never runs the
  shadow, so users pay nothing for it (V21-8): the shadow is compiled only under the `shadow-ir`
  feature, and there is no runtime flag. The feature is defined on `alint-rules` and enabled only
  from `publish = false` crates (`alint-e2e`, `xtask`), forwarded through `alint-testkit` the way
  its `strategies` feature is. A feature on a published crate is visible on crates.io, so
  "test-only" is enforced by a gate, not by Cargo: CI checks `cargo tree -e features -p alint`
  for each release target, as `release-binary.sh` builds it, and fails if `shadow-ir` appears. The old code is deleted in v0.22, after one
  release with zero differences (DEC-7).
- **Where replay runs (V21-7):** a `corpus-replay` workflow on the canonical bench host's runner
  (`[self-hosted, linux, bench]`), dispatched before each release that contains IR changes and
  weekly on a schedule, with its cache on that host. The runner executes one job at a time, so a
  replay never overlaps a benchmark. `RELEASING.md` gains the pre-release step.
- **Not in v0.21:** the other six `cross_file` relations, the remaining `ExtractSpec` consumers,
  and manifest-derived scope. The port's results feed Q8.

### Plugin contract `0.x` (slice P1)

- **Module (V21-5):** a doc-hidden `alint_core::plugin_api` module defines `Adapter` and
  `Analyzer` traits over contract types (rows, values, provenance, findings, related locations,
  capabilities, limits), never over engine internals. A test fails if the module imports
  anything but `std`, `core`, `alloc`, and its own items. It is not a separate crate in v0.21:
  the published `alint-core` would depend on it, so it would have to be published too, with the
  new-crate bootstrap in `RELEASING.md`. It moves into a published `alint-plugin-api` crate with
  slice P2.
- **Implementors:** the built-in row adapters and `key_parity` implement the traits; the
  contract's value type is the flat node list described under "Row and Value". "Done"
  follows `plugin-architecture.md` section 10.3: a built-in adapter and analyzer run through the
  traits with no behaviour change.
- **Registry injection:** every runtime call site listed under "Starting point" takes an
  injected registry instead of calling `builtin_registry()`, including `alint-testkit`'s runner,
  which is published API. `Engine::new` already accepts any
  `RuleRegistry`.
- **WIT draft:** `alint:plugin@0.1.0` is committed as a design artifact, with an `adapter` and
  an `analyzer` world (Q10; the `fixer` world follows with slice P2), host-pushed inputs, and no
  host callbacks. Bindings are generated once witforge's
  emitters exist (decision V21-5).
- **Fixer role:** deferred to slice P2, with plugin fixers.

### Measurement (DEC-13)

- **Two references (V21-6):** v0.18.2 is the blocking reference for relation-layer changes, as
  below. `docs/benchmarks/ANCHOR` (v0.20.0, +10%) keeps serving every release's bench gates
  unchanged.
- **What counts as a relation-layer change:** a change to `crates/alint-core/src/relation/`,
  `crates/alint-core/src/plugin_api/`, `crates/alint-core/src/extract.rs`,
  `crates/alint-core/src/engine/`, `crates/alint-rules/src/cross_file/`, or `key_parity`'s
  source, as `ci/scripts/detect-changes.sh` classifies it.
- **Instruction gate:** S1-S3 at 1k and 10k, at most +3% against v0.18.2. The v0.18.2 binary is
  rebuilt in the same job with the same toolchain and measured by the current harness, so the
  comparison has no committed baseline and no host or toolchain drift; the recorded counts are
  for the record. It runs on relation-layer pull requests and on every push to `main` that
  touches the relation layer, so drift is attributed when it lands. It fails rather than
  advises, and counts in the CI summary. Only the alint binary comes from v0.18.2: the harness,
  the generated trees, and the gungraun runner are the current ones, so a gungraun bump does not
  touch the reference side. If a future toolchain stops compiling v0.18.2, the job pins one that
  does.
- **S4 is not compared with v0.18.2.** It cannot run there (see "Starting point"), so S4 stays
  on the per-PR gate against each change's base.
- **Syscalls:** `read`-family and `stat`-family counts (`read`, `pread64`, `readv`, `openat`,
  `newfstatat`, `statx`, `fstat`, `lstat`, `stat`, `getdents64`, `readlink`, `access`) for S1-S3
  at 10k files, counted for the alint process and its threads only (spawned `git` children vary
  by host), must not grow with the tree: at most 32 more calls per family than v0.18.2. Fixed
  per-run probes differ by a few calls (R0 found `main` probing `.alint.d`, and a CPU-count read
  that some runs make and others do not), while a per-file regression adds thousands at 10k
  files.
- **Relation scenario:** R2 adds a scenario with `key_parity` and per-file rules over the same
  files. It has no v0.18.2 counterpart, so it is budgeted separately against the first release
  that has it (section 9.6, "new capabilities").
- **Release check:** before the release, on kbench with caches dropped, S1-S3 x {full, changed}
  against v0.18.2: wall-clock medians at 100k (30 runs) and at 1M (bench-scale's 1M sampling,
  one warm-up and three runs: at 30 runs S3 alone would take about 16 hours) at most +3%, and
  peak RSS at 1M at most +10%.

## Decisions

Taken one at a time on 2026-10-24 and recorded in
[ADR-0023](../../adr/0023-relation-ir-key-parity-and-plugin-contract-0x.md), which also lists the
rejected options.

| ID | Question | Decision |
|---|---|---|
| V21-1 (Q4) | Which value types are core? | Primitives with lexeme-preserving numbers, `path`, lists, and records. `semver` and SPDX wait for several consumers; `semver_range` lands as a standalone library first |
| V21-2 (Q13) | How are paths compared in relations? | Byte-exact after normalization on every operating system, with case folding as an explicit per-comparison opt-in. Today's `resolves` lookup is already exact, but on `Path` components, so Windows separators differ; normalization removes that |
| V21-3 | Which formats does `key_parity` cover in v0.21? | The five row formats, plus TOML (a new row adapter) and two XML resource-bundle dialects, Android string resources and .NET `.resx`, as `key_parity`'s own format names |
| V21-4 | What is a key in a nested file? | Leaf key paths, arrays treated as leaves, plus an opt-in `plurals: cldr` that counts a CLDR plural group as one key |
| V21-5 (Q10, P0) | How does the contract proceed while witforge's emitters do not exist? | Hand-written traits now in a doc-hidden `alint_core::plugin_api` module (not a crate: one the published `alint-core` depends on must itself be published), the WIT draft committed, bindings generated when P0 lands, and a published crate with P2 |
| V21-6 (DEC-13) | How does the frozen v0.18.2 reference coexist with `docs/benchmarks/ANCHOR`? | Both: v0.18.2 is the blocking reference for relation-layer changes (S1-S3; S4 cannot run on v0.18.2), and the anchor stays for release gates. If `main` is already over +3% against v0.18.2, the numbers come back for a decision first |
| V21-7 | Where does corpus replay run? | On the canonical bench host's runner, before each release with relation-layer changes and weekly, with spawning rules removed |
| V21-8 | Does the shadow run anywhere outside tests? | No: tests and corpus replay only, under the `shadow-ir` feature |

## Non-goals

v0.21 does not add:

- `version: 2`, named selectors or relations, or the generic `constraint` kind (Phase 3);
- a plugin host, a published plugin API crate, or plugin fixers (slices P2 and later);
- `key_parity` fixes, or `placeholder_parity` (D2);
- an HCL row adapter, generic XML rows (only the two resource-bundle dialects land), typed
  extraction for the `equals` port, YAML alias or merge-key expansion, or per-locale checks of
  CLDR plural categories;
- plan-level `explain --trace` (it needs the Phase 3 plan);
- deletion of any existing implementation (that happens in v0.22, after a zero-difference release).

## Work items and dependencies

| ID | Deliverable | Depends on | Completion evidence |
|---|---|---|---|
| R0 | v0.18.2 instruction and syscall baselines; cumulative blocking gate mode for IR pull requests | none | baselines committed; the gate fails a synthetic +4% change and passes v0.21's base |
| R1 | `alint_core::relation` types, provenance, absence states, limits, the internal pointer type, the flat contract form of `Value` | R0 | unit and property tests for canonical order, the three absence states, every limit, and the nested-to-flat round trip |
| R2 | Built-in row adapters and the `Extract` wrapper on the IR, with read coalescing; the relation scenario | R1 | S1-S3 syscall counts unchanged; one read and one decode per (adapter, resource) asserted by a counting test across relations and per-file rules; the relation scenario recorded |
| R3 | `key_parity` | R1, R2 (R9 for its formats) | firing and silent scenarios across all eight formats, nested locales, `strip_root` on Rails files, `plurals: cldr` (nested YAML, i18next suffixes, Android `<plurals>`), `.env`/`.env.example`, every failure case above, a reference-only `--changed` edit, keys-only trace previews, golden fingerprints |
| R4 | Shadow harness and the `cross_file` `equals` port | R1, R2 | zero differences on every scenario and on the property generator's cases |
| R5 | `xtask corpus-replay`, full-SHA pins, cache, and difference report | none (R4 for the shadow comparison) | replays the 110 configs at their pins on kbench; once R4 lands, zero shadow differences there |
| R6 | `alint_core::plugin_api` traits, its import-boundary test, and registry injection | R1 | a built-in adapter and `key_parity` run through the traits; golden fingerprints unchanged; the boundary test fails on an engine import |
| R7 | WIT `alint:plugin@0.1.0` draft (bindings if P0 has landed) | R6 | witforge's subset checker accepts the draft |
| R9 | TOML row adapter; Android string-resource and `.resx` dialect adapters | R1, R2 | per-adapter unit tests, duplicate and parse-failure cases, the dialect skip rules, the XML depth guard exercised, eager spans for findings |
| R8 | Docs, changelog, generated artifacts, budget report, go/no-go assessment | R0-R7, R9 | preflight, docs export, generated checks, and the section 9.6 table filled in |

R0 and R5 can start immediately and in parallel. R1 waits for R0's baselines. R3, R4, and R9 can
run in parallel once R2 lands; `key_parity` is built and tested on the five row formats first,
and gains the R9 formats as each adapter lands. R6 and R7 are part of v0.21 (ADR-0023 rejects
deferring the contract); only R7's generated bindings wait for witforge's emitters.

## Cross-cutting invariants

1. **No behaviour change for existing configurations.** Findings, messages, exit codes, and
   baseline fingerprints stay byte-identical; the golden gate and the shadow harness assert it.
2. **Fail loud.** A parse failure, type mismatch, or limit hit is an analysis failure, never an
   empty or partial relation that passes (section 6.3).
3. **Read once.** No file is read twice in one run because the IR selected it (ADR-0003); the
   counting test and the relation scenario's syscall counts assert it.
4. **Determinism.** Rows, related locations, and truncations follow the canonical order; a test
   runs `key_parity` and the port with shuffled index order and several thread counts and
   requires identical output.
5. **Budgets.** The section 9.6 budgets hold against v0.18.2, enforced for IR pull requests.
6. **Internal stays internal.** No IR type appears in `alint-core`'s public API or in JSON
   output. Rows stay visible only through the experimental trace preview (Q7, ADR-0022). The
   relation module is `pub(crate)`, and the generated, gated report schemas gain no field.
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
  on nested YAML, i18next suffixes, and Android `<plurals>`, Rails files with `strip_root`,
  `.env` against `.env.example`, a missing reference, an unparsable reference and peer, a
  `peers` glob with no match, duplicate keys, and an edit to the reference alone under
  `--changed`. Its trace is complete, and its
  fingerprints are stable across repeated and shuffled runs.
- The `cross_file` `equals` port shows zero differences on every scenario that uses it, on the
  property generator, and on the 110-config corpus replay.
- The S1-S3 instruction counts are within +3% of v0.18.2, their `read` and `stat` counts have
  not increased, the relation scenario is recorded, and the 100k and 1M wall-clock and RSS
  budgets hold on kbench.
- A built-in adapter and `key_parity` run through the plugin traits with the golden fingerprints
  unchanged.
- The go/no-go assessment is written: whether the ported path became simpler and more consistent,
  with the evidence. If it did not, the stop rule applies and Phase 3 does not start.
- Full workspace, cross-platform, shell, advisory, and preflight checks pass, and generated
  artifacts (schema, facts, docs export, roadmap) regenerate cleanly.

## Risks

1. **witforge's emitters slip.** The hand-written traits (V21-5) keep v0.21 independent of them.
2. **Relation-major evaluation adds reads.** The relation scenario and the counting test catch
   the v0.14 S2 read-path class, which instruction counts cannot see, before any kind moves.
3. **Corpus replay cost and safety.** It is cached and scheduled, not run per pull request.
   Repositories that disappeared upstream are reported and skipped, not counted as differences.
   Spawning rules are removed before any config runs.
4. **Size.** Phase 2 is an L-sized phase, and V21-3 adds three adapters to it. R9 does not block
   R4, and `key_parity` is useful on the five row formats before R9 completes.
5. **New XML adapters reopen the parser denial-of-service class.** Both dialects parse through
   the existing guarded XML path; R9's evidence includes the depth guard.
6. **The budget is already spent.** If R0, or the release check's wall-clock and RSS runs, find
   `main` over budget against v0.18.2 before any relation-layer change, the gate cannot be set as
   written; the numbers go back for a decision (V21-6).

## Next step

Start R0 (v0.18.2 baselines and the blocking gate mode) and R5 (corpus replay) in parallel. R1
starts once R0's baselines are recorded.
