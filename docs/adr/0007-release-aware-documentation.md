---
status: accepted
date: 2026-07-03
decision-makers: asamarts
---

# 0007. Release-aware, single-source documentation contracts

## Status

Accepted. (One of: Proposed | Accepted | Rejected | Deprecated | Superseded by ADR-NNNN.)

Mechanism refined 2026-07-03 by an adversarial review of the plan against the pipeline
code (see [documentation-drift.md](../design/v0.14/documentation-drift.md) §9); the
decision is unchanged, the implementation details below are the corrected ones.

## Context

alint's documentation is generated from versioned contracts (`facts.json`, `roadmap.json`,
the type-derived `schemas/v1/config.json`) that alint.org consumes. The docs bundle pins
most contracts to the release tag, but several artifacts are overlaid from `main` so doc
fixes ship without a release. A 2026-07-03 two-repo audit found this overlay leaks
unreleased state through THREE vectors, all confirmed: (1) the per-rule reference pages
(the post-v0.13 `root_only` extension renders live in the `file_absent`/`dir_exists`/
`dir_absent` Options tables the released v0.13.0 binary ignores); (2) the hand-authored
LikeC4 model (`config-model.c4` gained an unreleased `baseline` field element); (3)
`docs/site/reference/**` (raw main overlay, guarded today only by an author-discipline
comment). The bundle's "refresh-existing" safety blocks new pages but not new content on
existing pages.

The same audit found marketing-count drift the pinned contracts do not cover ("83-rule
catalogue" should be 89; a benchmark page stating both the catalogue count and a real
79-rule scenario size for one measurement). The current gate is a proximity matcher over a
hardcoded noun allowlist and an 11-file scope, so novel phrasings slip through; but a naive
"forbid every count" gate is not viable either, because "83-rule catalogue" (must be
canonical) and "79-rule pass" (a correct scenario size) are lexically identical and
semantically opposite, and some counts (competitor tools, dated snapshots) have no
derivable source.

Decision drivers: preserve the "doc fixes ship without a release" benefit where content
changes often (rule pages, reference); make released-vs-unreleased a property of the
artifact, not author discipline; give every documented number a single source of truth or
an explicit, reviewed exemption. Design doc + finding evidence:
[documentation-drift.md](../design/v0.14/documentation-drift.md). Related: ADR-0001
(spec-driven development), the `facts.json`/`roadmap.json` contracts.

## Decision

We will make documentation release-aware and single-sourced by three mechanisms.

1. **Availability is a schema keyword.** Rule options carry an introducing-version keyword
   **`x-since`** (named with the `x-` extension prefix because `since` is already a
   rule-option name). `docs_export`, when rendering the per-rule Options tables and the
   rules.md prose for a bundle, omits any option whose `x-since` exceeds the RELEASED
   version, and strips prose blocks marked `<!-- alint:since=X -->` by the same comparison.
   The released version is passed explicitly into the export (a `--released-version` arg
   supplied by the docs-bundle workflow from the tag it already resolves), NOT read from
   the worktree's `CARGO_PKG_VERSION` (which reflects `main` and flips at the release-bump
   commit before publication). The keyword lives in `schemas/v1/config.json`: for
   schemars-migrated kinds it is emitted from a field attribute (`#[schemars(extend(...))]`);
   for kinds still defined in the hand-authored base schema it is set there until they are
   migrated (migration preferred, as it also fills their Options descriptions). The prose
   stripper is version-conditional paired-sentinel logic (adapted from the roadmap
   generator's `elide_internal_blocks`); it is new code, not a reuse of the test-only
   `alint:ignore-example` marker. A revert-sensitive test asserts an option with a future
   `x-since` is dropped at the current release and kept at its own.

2. **Surfaces that cannot carry availability metadata are tag-pinned, not main-overlaid.**
   The hand-authored LikeC4 model and the generated crate graph cannot express `x-since`
   and cannot be element-stripped without dangling view references, so their overlay is
   removed and the bundle uses the release tag's copy. This trades doc-fix latency for
   leak-safety on artifacts that change rarely.

3. **Every documented count is sourced or explicitly allowlisted (no silent counts).**
   Counts that can be sourced MUST interpolate from a contract (`facts.json` catalogue
   counts; a new per-ruleset-size field; case-study `rules:` frontmatter). Every count that
   cannot (competitor tools, dated/historical snapshots, composed+deduped bench-scenario
   sizes) must appear in an explicit, enumerated, justified allowlist. A gate fails on any
   bare count-noun integer in the marketing surface that is neither an interpolation nor an
   allowlist entry, carving out the sync-generated docs subtree and the dated blog. This is
   "no SILENT counts," not the infeasible "no exemptions."

## Consequences

Easier: unreleased options and prose cannot leak onto the live site regardless of author
care; the released/unreleased boundary is enforced by the build for the rule pages and by
tag-pinning for the arch model; every count has one source or one reviewed exemption and
cannot silently drift; the `x-since` data is reusable later by `--explain`, the LSP, and
release notes.

Harder: new rule options introduced between releases must carry `x-since` (and unreleased
prose a sentinel) or the regression test blocks the bundle - the intended forcing function,
but one more step per option. Populating `x-since` type-derived requires migrating the
four existence kinds to schemars (a real migration with fidelity-test risk); the base-schema
hand-edit fallback is quicker but not type-derived. Tag-pinning the arch model means arch
doc fixes wait for a release. The count allowlist must be curated and reviewed (it is the
honest cost of the counts that have no contract source); and `benchmarks-trajectory.json`
would need new per-scenario count data to move bench sizes off the allowlist into a
contract (deferred as optional).

## Considered Options

- **Availability mechanism.** Chosen: an `x-since` schema keyword filtered at bundle render
  time against an explicitly-passed released version. Rejected: auto-strip options absent
  from the release-tag schema (handles tables but not prose, and fails CI on every normal
  mid-cycle option add); tag-pin the rule pages wholesale (reverses the fast-doc-fix
  benefit for a frequently-edited surface); rely on the worktree `CARGO_PKG_VERSION` as the
  oracle (wrong during the release-cut window); accept docs-ahead as policy (does not
  prevent recurrence). Naming `since` was rejected for colliding with an existing option
  name.
- **Arch model.** Chosen: tag-pin (the model changes rarely, so latency cost is low and
  the mechanism is trivial). Rejected: element-level sentinel stripping (dangles LikeC4
  view references).
- **Counts.** Chosen: sourced-or-allowlisted with carve-outs. Rejected: "no exemptions"
  (infeasible - competitor and dated counts have no source, and lexically-identical
  counts are semantically opposite); a marker-exemption gate without an enumerated
  allowlist (leaves scoped counts as un-reviewed inline prose); targeted anchors only
  (the current whack-a-mole gate that missed both audit findings).

## More Information

Plan, phased remediation, per-finding evidence, and the review corrections:
[documentation-drift.md](../design/v0.14/documentation-drift.md) (§9 lists what the
adversarial review corrected). Audit context:
[post_v0.13_audit.md](../design/v0.14/post_v0.13_audit.md) (Phase 6, Phase 7, Themes/5).
Implementation PRs will be linked here as they land.
