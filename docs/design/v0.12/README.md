# v0.12 — Design pass

Status: **Planned.** Drafts written 2026-05-25 after the post-v0.11
30-repo case-study re-analysis
([`docs/development/case-study-v011-reanalysis-log.md`](../../development/case-study-v011-reanalysis-log.md)).
Each file here is a per-workstream design that should be reviewed and
revised before implementation starts. Nothing in v0.12 is started yet;
v0.11 (LSP + DSL polish) cuts first.

## What v0.12 ships

Two interlocking halves:

1. **Real-world coverage expansion (100+ repos).** The v0.10 push and
   the 30-repo re-analysis proved a repeatable method: clone a real
   repo, catalogue its bespoke manual/script/CI validation, express as
   much as alint can, record the residual as a concrete new-kind
   candidate or a deliberate non-goal. v0.12 scales that 3-4× and
   broadens the ecosystem spread, then folds any *new* gaps it finds
   into the same release.
2. **Closing the gap rule kinds the 30-repo pass already ranked.** The
   30-repo synthesis produced a demand-ranked backlog (no regressions,
   all additive). v0.12 implements it, design-doc-first per project
   convention.

## Why now

The 30-repo re-analysis (the immediate predecessor of this cut)
established that alint's *shipped* v0.10/v0.11 capability set already
expresses ~65-83% of a typical real repo's bespoke validation, up from
~35-55% pre-v0.10. The residual splits cleanly into (a) a small ranked
set of additive new kinds — built here — and (b) deliberate non-goals
(AST/type linters, compile/test execution, semantic graph analysis),
which alint runs but does not reimplement. v0.12 is the cut that turns
the (a) backlog into shipped features and validates the result against
a much wider corpus.

## Workstreams (each has a design doc in this directory)

| Doc | Scope | Demand signals (30-repo pass) |
|---|---|---|
| [`case_study_100_repos.md`](./case_study_100_repos.md) | The 100+ repo study + pipeline | — (the vehicle) |
| [`git_commit_subject_matches.md`](./git_commit_subject_matches.md) | Commit subject-shape rule | go, node, nixpkgs |
| [`changeset_requires_path.md`](./changeset_requires_path.md) | Diff "must-add" + `pair_changed_together` | prettier, cpython, pnpm; rust, turbo |
| [`value_set_membership.md`](./value_set_membership.md) | N-in-1 / set membership family | ts, react, pnpm, rust, tf |
| [`cross_file_normalize.md`](./cross_file_normalize.md) | `normalize:` value-transform | protobuf, pnpm |
| [`import_gate_enrichment.md`](./import_gate_enrichment.md) | default-deny + glob-discovered rules | vscode, kubernetes |
| [`file_dependency_graph.md`](./file_dependency_graph.md) | generic file-reference graph — cycles / orphans / freshness / layering; language-agnostic, **study-gated** | (0 file-graph sources yet) |
| [`dependency_graph_allowlist.md`](./dependency_graph_allowlist.md) | package-graph dep firewall (**decoupled + deferred**; non-goal-adjacent) | rust, go |
| [`niche_rule_kinds.md`](./niche_rule_kinds.md) | 6 small kinds, 1-2 sources each | cpython, tokio, tf, turbo, flutter |
| [`asf_bundle_overfire.md`](./asf_bundle_overfire.md) | ASF bundle fix + import_gate presets + docs | airflow, helm, istio, k8s, tf |
| [`deferred_from_v011.md`](./deferred_from_v011.md) | v0.11 carryovers (scope predicates, walk-error policy, LSP ignore-action) | DSL completeness; pnpm |

## Release shape

The study runs first (it may reshuffle the ranking and add new-kind
candidates). Rule kinds land design-doc-first, one atomic
draft-then-implement commit per kind, accumulating on CHANGELOG
`[Unreleased]` toward the v0.12 minor — same cadence as the v0.10
rule-kind cut. The ASF-bundle fix is the highest-confidence,
lowest-risk item and can land early as a standalone.
