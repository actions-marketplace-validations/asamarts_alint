# `for_each_match` — the in-file line quantifier (④)

**Status: SHIPPED (v0.12).** The one genuinely new rule kind in the v0.12
cycle (rule-kind count +1); every other v0.12 item is a shared-axis enrichment.
Nominated by both the architecture synthesis (axis ④, "in-file block/line
for-all") and the deep case study (`deep_case_study_v2.md`).

## What it is

A per-file rule. For **each line matching `select:`** (a regex), the line must
satisfy the nested `require:` predicates. It is the missing dual of
`ordered_block`'s `select:` — where `ordered_block` *orders* selected lines,
`for_each_match` asserts a *conjunction of predicates* over each one.

```yaml
# Every changelog bullet must end with a linked PR reference, must not use the
# "[Fix #N]" form, and its display number must equal its URL number.
- id: changelog-entries-well-formed
  kind: for_each_match
  paths: ["CHANGELOG.md"]
  select: '^[*-] .*\[#(?P<disp>\d+)\]\([^)]*pull/(?P<url>\d+)\)'
  require:
    matches: ['\)\.$']            # the line (a selected element) must match ALL of these
    forbid:  ['\[Fix #\d+\]']     # ...and match NONE of these
    equal:   [disp, url]          # ...and these named `select` captures must be equal
  level: warning
```

A line the `select` regex does not match is not an element and is ignored. One
violation per offending line, naming the failing predicate.

## Why a new kind (reproduce-first)

Two shapes recur in the corpus and are genuinely inexpressible today; both were
reproduced end-to-end against the shipped binary before this build:

1. **Per-line conjunction** (`lines_all_match`) — "every line matching `P` must
   *also* match `Q1..Qn`". `file_content_matches` is `pattern.is_match(text)` —
   pure **existence** (one match anywhere passes), so a file with one well-formed
   and one malformed `* ` entry passes when it should fail. High corpus value:
   Keep-a-Changelog / changelog-d / towncrier shops all hand-roll a ~15-clause
   changelog-entry grammar (rubocop's `changelog_entry_format` is the canonical
   one).
2. **Intra-line capture equality** (`line_captures_equal`) — "two captured
   groups on the same line must be equal" (mypy's `bad-pr-link`: the CHANGELOG
   display number must equal the `/pull/` URL number). alint's regex engine is
   the Rust `regex` crate (RE2): **no backreferences, no look-around**, so
   `\[#(\d+)\]\(.*pull/(?!\1)\d+\)` is rejected at build time. No `file_content_*`
   kind can compare two captures within one line.

`select` + `require: { matches, forbid, equal }` closes both with one kind.

## What it deliberately does NOT do (reproduce-first dissolved / deferred)

- **`max_consecutive_spaces`** — already expressible: `file_content_forbidden`
  with `pattern: '[^ ] {3,}[^ ]'` fires correctly. Not a `for_each_match` case.
- **count-header / `self_referential_count`** ("line 1 is an integer equal to the
  count of the remaining lines") — a *cardinality* shape, not a per-match
  predicate. A distinct future primitive; deferred (Tier-3, ~2 repos).
- **intra-file reference graph** (`implicit_link_resolves`: every `[name][]` use
  has a `[name]:` definition in the same file) — a line→line orphan/dangling
  check needing cross-match state (a "defined set" vs "used set"), not a
  per-match predicate. Deferred; would generalise `file_graph` to intra-file
  nodes.
- **Nested *rule kinds*** in `require:` (the analysis sketch showed
  `require: - { kind: file_content_matches, ... }`). Rejected as over-general:
  recursive rule dispatch, ambiguous level/message semantics, and most per-file
  kinds operate on whole files, not single lines. The flat
  `matches` / `forbid` / `equal` vocabulary closes every reproduce-confirmed
  case with a readable, bounded surface. `not_equal` / multi-line block elements
  are trivial future adds, deferred until a corpus case needs them.

## Semantics

- `select: <regex>` — a line is an element iff `select` matches it (anywhere in
  the line). Named captures from `select` are available to `require.equal`.
- `require:` — at least one of:
  - `matches: [<regex>...]` — the element line must match **all** of these.
  - `forbid: [<regex>...]` — the element line must match **none** of these.
  - `equal: [<name>...]` — the listed named `select` captures must all be equal
    (string equality; ≥2 names; each name must exist in `select`).
- One violation per (line, failing predicate). Per-file fast path
  (`PerFileRule`), line-oriented; non-UTF-8 files are skipped.

## Wiring checklist (new kind)

`for_each_match.rs` + `pub mod` and `registry.register("for_each_match", …)` and
the kind-list test in `lib.rs`; `rule_for_each_match` schema def + the rule
`oneOf` (both schema copies, byte-identical); an `all_kinds.yaml` entry; a
firing **and** a silent e2e scenario (`coverage_audit_pass_fail`); a `docs/rules.md`
entry (`coverage_audit_rules_md_drift`); the default-options snapshot; the
rule-count bump (README ×2 + about + `all_kinds`, `coverage_audit_readme_claims`);
CHANGELOG. New `+1` dispatch class for the 1M-file macro bench (extends S12,
the per-file class) — tracked with the bench step, not this build.
