# v0.19: Fail-loud findings and the capability inventory

Status: In progress (2026-10-15); see [Status and next steps](#status-and-next-steps-as-of-2026-10-15). This is the first release of the general-linter program: Phase 0
and Phase 1a of [`../general-linter.md`](../general-linter.md) section 11, decided in
[ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md). Each work item
below gets its own design document in this directory (in the [`../TEMPLATE.md`](../TEMPLATE.md)
shape, including the false-positive section) before its code lands, and the item's open
questions are decided there, one at a time.

v0.19 is a minor release with deliberate behaviour changes (W1 turns silently ignored fields into
load errors, W2 and W3 turn some false passes into failures, and W4 changes report text). Every
behaviour change is listed in the changelog with its migration note.

## Status and next steps (as of 2026-10-15)

This section is the hand-off point for whoever picks the program up next. Update it when an item
moves.

### Where things stand

- **Planning is done.** The general-linter program is adopted (ADR-0019); every DEC-1 to DEC-14
  outcome is recorded in [`../general-linter.md`](../general-linter.md) section 13.1, the open
  questions in section 13.2, and the plugin design in
  [`../plugin-architecture.md`](../plugin-architecture.md). W1's design
  ([`kind-meta.md`](kind-meta.md)) is accepted with K1 to K5 decided (its section 7).
- **v0.18.3 shipped** (tag `v0.18.3`, commit `b02109b8`): the four K2 bugs plus the ten review
  findings described below (#327).
- **v0.18.4 shipped** (tag `v0.18.4`, commit `18e1a66d`): the `--changed` performance fix (#329;
  the shared tree file set is built only when a rule looks ancestors up on the filtered index)
  and adotenv 0.1.1 with verified `.env` fix edits (#330). All release jobs, the post-publish
  smoke channels, the Action pin (`e3c9a85b`), and alint.org (#92) are done.
- **W1 is in progress; slices 1 and 2 are complete.** The serializable `KindMeta` / `FixOpMeta`
  types and metadata-taking registry APIs are in `alint-core`; metadata is mandatory for every
  canonical registration; all 95 corrected per-kind records are declared beside their builders;
  and all 11 aliases resolve the exact canonical record. Completeness, audited-distribution,
  typed-selector transition parity, spawn-source parity, and rule-instance conformance gates are
  in place.

### Open release item: the v0.18.4 benchmark record

The v0.18.3 bench record (#328) was closed without merging: it failed the release regression gate
on S2 1m `changed` (+16% `min_ms` against the +15% limit), which is the regression #329 fixed. Its
branch `bench-record/v0.18.3` is kept as the measured record, and
[`../../benchmarks/investigations/2026-10-v0.18.3-changed-mode/`](../../benchmarks/investigations/2026-10-v0.18.3-changed-mode/README.md)
holds the analysis. v0.18.3 therefore has no published results directory.

The v0.18.4 bench-record workflow run (`38777159856`) opens a `bench-record/v0.18.4` PR. To
finish it, follow RELEASING.md "Bench-record review", with one difference: the baseline is
**v0.18.2**, since v0.18.3 was never published.

```sh
git fetch origin bench-record/v0.18.4
git show origin/bench-record/v0.18.4:docs/benchmarks/macro/results/linux-x86_64/v0.18.4/results.json > new.json
cargo run -q -p xtask -- bench-gate --results new.json \
  --baseline docs/benchmarks/macro/results/linux-x86_64/v0.18.2/results.json
```

- Quality: CV at or under 10% on the 100k and 1m cells; regression: `min_ms` at most +15% on every
  cell of 10k files or more; fingerprint: `alint_version` 0.18.4, the canonical host's CPU, Linux.
- Expected: S2 `changed` back near v0.18.2; S1 and S4 `changed` up by about 5 to 10%, which is the
  cost of correct `has_ancestor` evaluation under `--changed` and inside the budget. A failure
  outside those shapes needs a same-host A/B before anything else (build every binary from source
  with one toolchain, check the binaries' hashes, and do not compile during timing runs).
- On a pass: paste the gate output into the PR, update `docs/benchmarks/HISTORY.md`, merge.
- Then alint.org (its `OPERATIONS.md` step 4, following alint.org #88): add
  `src/data/bench-v0.18.4.json` from the results, point both lines of `src/data/bench.ts` at it,
  update `scripts/head-parity-baseline.json`, build, run the checks, open a PR, merge, and confirm
  `/benchmarks/` shows v0.18.4.

### Next work, in order

1. **W1, `KindMeta`.** Slices 1 and 2 of [`kind-meta.md`](kind-meta.md) section 5.2 are complete.
   Next, inject the `KindCatalog` into every DSL loader call site and derive the trust and audit
   lists from it (including the v0.18.3 nested-config lists `UNCONFINABLE_KINDS`,
   `REGEX_SELECT_KINDS`, `NESTED_RULE_KINDS` in `crates/alint-dsl/src/nested.rs`, and
   `SPAWNING_RULE_KINDS`), keeping the old hand lists for one release as equality cross-checks.
   Then fix any config that sets an ignored field and enable K1 enforcement; migrate the schema
   and add K4 restrictions plus the `facts.json` section; then add K5's capabilities line.
2. **W4** (notes in every format) and **W7** (`--as-of`): small and independent of W1's tail.
3. **W2** then **W3** (fail-loud analysis; W3's reject-at-load half needs W1).
4. **W5** (redaction and the leak gate), **W6** (related locations, the JSON schema relaxation,
   the frozen-fingerprint gate), **W8** (duplicate-aware spanned extraction).
5. **W9** as capacity allows, then **W10** (documentation, recipes, roadmap, the stale
   `register_as` doc comment).

Each item gets its design document in this directory before code, per the rule at the top. The
release gates are listed at the end of this file.

### Parallel tracks

- **witforge** (`asamarts/witforge`, private): the plugin contract compiler. The skeleton
  (WIT loading, the subset checker, the wire lock, a `check` / `lock` CLI) is in place with CI
  green; next are the Rust and protobuf emitters. Nothing is published to crates.io until it has
  real functionality (crates.io does not allow name-only placeholder releases).
- **Phase 1b design**: `alint test` (DEC-6) and `explain --trace` (Q14), for v0.20.
- **adotenv 0.2** is developed in its own repository. alint pins 0.1.1; 0.2's breaking model
  will need a single migration PR here (the verified-edit helpers in
  `crates/alint-core/src/structured_fix/mod.rs` are the integration point).

## Before v0.19: the v0.18.3 patch

The W1 inventory ([`kind-inventory.md`](kind-inventory.md)) reproduced four bugs. Decision K2
([`kind-meta.md`](kind-meta.md) section 7) ships all four as a v0.18.3 correctness patch, ahead of
W1:

1. a nested `git_blame_age` never fires unless a top-level rule also uses blame;
2. structured `*_path_*` kinds with literal `paths:` skip `scope_filter` in the rule-by-rule path,
   so `alint fix` can edit a file the filter excluded;
3. nested configs re-root every `select` as a path glob, corrupting the regex `select` of
   `for_each_match` and `ordered_block` and admitting kinds whose selectors are never re-rooted;
4. under `--changed`, `no_case_conflicts` and `max_files_per_directory` miss violations caused by
   a change (they now evaluate on the full index).

Review of the patch found ten more instances of the same classes, fixed in the same patch (see
[`kind-meta.md`](kind-meta.md) section 1 and the v0.18.3 changelog): nested confinement now runs
after template expansion and inside `require:` blocks and covers kind-specific path fields;
`--changed` judges changed files against the whole repository; `json_schema_passes` re-validates
on a schema-only change; and `file_exists`'s literal stat honours `git_tracked_only`.

## Work items

| ID | Item | Decisions | Size | Depends on |
|---|---|---|---|---|
| W1 | `KindMeta` capability inventory | Phase 0, K1-K5 | L | none |
| W2 | Analysis failures become error findings | DEC-2 | M | W1 (for the reject-at-load half of W3) |
| W3 | Close the remaining fail-open sites | section 4.12 | M | W1, W2 |
| W4 | Notes in every output format | section 8.1 | S | none |
| W5 | Evidence redaction and the leak gate | DEC-3 | M | none |
| W6 | Related locations and ranges on findings | DEC-4, DEC-5 | L | W4 |
| W7 | `--as-of` input | DEC-11 | S | none |
| W8 | Duplicate-aware spanned extraction (substrate) | Q5 | M | W6 (span model) |
| W9 | Standalone detection kinds, as capacity allows | section 11.8 | per kind | W1 |
| W10 | Documentation, recipes, and roadmap updates | n/a | S | all |

Suggested order: W1, W4, W7 (small, independent) first; then W2 and W3; W5 alongside; W6, then
W8; W9 interleaved; W10 last.

### W1. `KindMeta` capability inventory

- Add a static `KindMeta` declared at registration for every kind: dispatch
  class, accepted selectors (`paths`, `scope_filter`, `git_tracked_only`, change selectors),
  capabilities (spawn, Git, environment, time, host keyring), formats, and fix operations.
- Emit it into `facts.json` (a new section; the facts gate covers it).
- Derive the spawn and Git trust lists (`SPAWNING_RULE_KINDS`, the Git-required kind list) from
  it, keeping the existing audits as cross-checks during the transition.
- Add a coverage audit: every scenario-built rule instance behaves within its declared
  `KindMeta` (for example, a kind that declares no spawn capability never spawns).

Design: [`kind-meta.md`](kind-meta.md) (accepted; decisions K1 to K5), with the per-kind
snapshot in [`kind-inventory.md`](kind-inventory.md). W1 also covers: load-time rejection of every
configured field a kind ignores (K1, a hard error), a `KindCatalog` injected into the DSL loader
(K3), per-kind JSON Schema restrictions plus migrating the 20 hand-written kind schemas (K4), and
a capabilities line in `alint rules show` and `explain` (K5).

Acceptance: `KindMeta` exists for all 95 kinds and 11 aliases; trust and audit lists derive from it
(the Git-required list grows from 3 to the correct 10); ignored fields are load errors in `check`
and `validate-config`; the schema matches the loader; the instance-conformance audit and the
facts gate pass.

### W2. Analysis failures become error findings (DEC-2)

- Separate "the rule could not evaluate this resource" from violations in the engine's result
  type, at every site that today wraps an `Err` as `Violation::new("rule error: ...")`.
- A failure caused by repository content is reported as a finding that always counts as error,
  regardless of the rule's `level`, with a stable message naming the resource and the reason.
- `alint baseline` refuses to record such findings, and an existing baseline cannot suppress
  them.
- Configuration failures keep exit 2 and internal errors keep exit 3; no new exit code.
- Every format renders them (an ADR-0012 positive-completeness gate).

Acceptance: a warning-level rule over an unparseable target exits 1; `alint baseline` does not
record the failure; scenarios cover each former "rule error" site; changelog entry under
"Changed".

### W3. Close the remaining fail-open sites

From [`../general-linter.md`](../general-linter.md) section 4.12:

- **`scope_filter` on kinds that ignore it**: done in W1 (K1), which rejects every ignored field at
  load and in `validate-config`, including manifest predicates.
- **Manifest-derived scope parse failure** becomes an analysis failure (W2) instead of an empty
  member set or a silent fallback to full scope.
- **Git unavailable**: Git-aware kinds report an analysis failure when Git or the repository is
  unavailable, instead of no findings.
- **Non-string extraction results**: a typed value at a queried path is either compared by its
  lexeme or reported, never silently dropped (the exact rule is decided in this item's design
  document).

Acceptance: each site has firing scenarios proving the new outcome and a gate asserting the
invariant; changelog entries under "Changed".

### W4. Notes in every output format

- SARIF: notes become `level: note` results; truncated analysis becomes
  `toolExecutionNotifications`.
- GitHub, GitLab, and Markdown outputs: a summary line with the note count and a pointer to the
  full list; JUnit and agent formats get an equivalent.
- Human output keeps `--show-notes`.

Acceptance: an ADR-0012 positive-completeness gate proves a note reaches every format.

### W5. Evidence redaction and the leak gate (DEC-3)

- Values extracted from dotenv and properties files are redacted by default in every message
  and format (rendered as a short marker plus a digest).
- A new `sensitive: true` rule option opts any other rule into redaction.
- Baseline files never store raw values: the advisory message is redacted, and the fingerprint
  remains a hash of the identity. The design document must address that an unsalted hash of a
  low-entropy value can be brute-forced (for example, by hashing with a per-repository salt or
  by excluding redacted values from the fingerprint input).
- A leak gate: a scenario plants a known secret in `.env` and asserts its bytes appear in no
  output format and in no baseline file.

Acceptance: the leak gate passes for all eight formats and the baseline; existing fingerprints
are unchanged for non-redacted rules (W6's golden gate).

### W6. Related locations and ranges on findings (DEC-4, DEC-5)

- Extend the finding model with a primary range and labeled related locations, with deterministic
  ordering and caps (at most a fixed number of related locations, then "and N more").
- Primary-location rule: the rule's declared report anchor, else the non-canonical side, else the
  first in canonical order; existing kinds keep today's anchor.
- Emit related locations in SARIF (`relatedLocations`) and the LSP (`relatedInformation`, only when
  the client advertises support) in v0.19. Relax `schemas/v1/check-report.json`'s
  `additionalProperties: false` in v0.19, announced in the changelog; the JSON report gains the
  new fields in the next release (DEC-4).
- Freeze baseline fingerprints: a golden file of every fingerprint over the whole scenario corpus,
  recomputed in CI; related locations never enter a fingerprint; the baseline schema stays at
  version 1 (DEC-5).
- Populate related locations first for `cross_file`, `registry_paths_resolve`, `unique_by`,
  `pair`, and `file_graph`.

Acceptance: the golden fingerprint gate is green with zero changes; SARIF and LSP gates prove
related locations reach both; the JSON schema relaxation ships with its changelog notice.

### W7. `--as-of` input (DEC-11)

- A global `--as-of <date>` flag and an `ALINT_AS_OF` environment variable.
- Without either, time-dependent kinds use the HEAD commit time; outside a Git repository they
  report an analysis failure (W2) unless `--as-of` is given.
- `git_blame_age` moves onto it; the reference time is shown in `explain` and in its findings.
- A superseding ADR records the time input in constitution invariant 1.

Acceptance: the same commit yields byte-identical `git_blame_age` output on different days; the
ADR is accepted; changelog entry under "Changed".

### W8. Duplicate-aware spanned extraction (substrate)

- Extraction gains ordered entry rows with spans for the formats whose parsers can provide them
  (dotenv through `adotenv`; JSON through the `jsonc-parser` AST; YAML, INI, and properties
  through the fix bridge's locators), instead of flattening to a last-wins object.
- This is the substrate for B1 `no_duplicate_keys` and B3 `structured_key_sort`
  ([`../rule-coverage-gaps.md`](../rule-coverage-gaps.md)); the kinds themselves may land in
  v0.19 or v0.20 depending on capacity.
- Question Q5 (how keys and duplicates are represented across the eight formats) is decided in
  this item's design document.

Acceptance: extraction exposes duplicates with spans for the listed formats; no change to
existing kinds' results.

### W9. Standalone detection kinds, as capacity allows

From the "ship now" bucket of [`../general-linter.md`](../general-linter.md) section 11.8, in
value order:

1. A1 `editorconfig_conforms`, after the policy-source-files ADR (DEC-12);
2. the Markdown structure kinds on the shipped CommonMark parser (E2 alt text, E3 required
   headings, E5 duplicate headings, E7 fragment resolution);
3. the SPDX id table with F2 `spdx_identifier_valid`;
4. H1 `pinned_references`, which also closes the reusable-workflow and composite-action holes in
   `ci/github-actions@v1`;
5. the C1 version-SSOT and C2 toolchain-pin bundled rulesets.

Each kind follows the normal cascade (rules.md section, schema, firing and silent scenarios,
rule-page fixtures, facts) and declares its `KindMeta`.

### W10. Documentation, recipes, and roadmap

- Document the flat key-parity recipe (`cross_file` `set_equals` over a `regex:` key extract for
  `.env` against `.env.example`).
- Update the README's counts only through `facts.json`; update ROADMAP's "Latest release" at
  release time.
- Close the remaining review follow-ups: the stale `register_as` doc comment in
  `crates/alint-rules/src/cross_file/spec.rs` (it lists only `{path}`, `{dir}`, `{stem}`).

## Parallel tracks during v0.19

- **Plugin contract compiler (P0).** The separate project of
  [`../plugin-architecture.md`](../plugin-architecture.md) section 6 starts in parallel as
  `witforge` (`asamarts/witforge`, private until alint's contract `0.x` consumes it; Q16,
  decided 2026-10-14).
- **Phase 1b design.** The design documents for `alint test` (DEC-6) and `explain --trace`
  (Q14) can be written during v0.19, for implementation in v0.20.

## Gates for the release

- every behaviour change listed in the changelog with a migration note;
- the frozen-fingerprint golden gate (W6) and the leak gate (W5) green;
- ADR-0012 completeness gates for notes, analysis failures, related locations, and redaction;
- the deterministic performance gate within the program budget (cumulative against v0.18.2,
  DEC-13), plus wall-clock benchmarks on the canonical bench host for W2, W3, and W6;
- the usual release flow in `RELEASING.md`, including the alint.org pin bump.
