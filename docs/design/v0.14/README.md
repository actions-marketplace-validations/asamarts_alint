# v0.14 — Hardening pass

Status: **In progress** — the hardening remediation is complete; a 2026-07-03
two-repo drift audit then opened a fourth strand (documentation + site drift:
remediation + prevention), tracked below. The v0.14 cut has four strands:

1. **Baseline / grandfathering mode** — already landed on CHANGELOG
   `[Unreleased]` (slices 1–4, audit follow-ups #88–#94). Design:
   [`../baseline.md`](../baseline.md), ADR-0006.
2. **The post-v0.13 security + correctness hardening pass** — the
   subject of this directory. A 12-agent adversarial audit (2026-06-27)
   of both repos (alint + alint.org) surfaced two confirmed RCE bypasses
   of the spawn gate, a path-confinement invariant that is false in the
   presence of in-repo symlinks, and a long tail of correctness, output,
   CLI, doc-drift and site-drift findings. The remediation is tracked,
   finding-by-finding, in [`post_v0.13_audit.md`](./post_v0.13_audit.md).
   **As of 2026-07-03 all CRITICAL + HIGH findings and the full M1–M14
   cluster have landed**; only the deferred-with-rationale tail remains.
3. **Post-v0.13 e2e sweep + adversarial-review remediation** (#111–#116) —
   a feature-by-feature test of everything since v0.13 found five more real
   bugs (baseline self-lint, a flat-`when:` DoS abort, a non-UTF-8 output
   crash, a silent `fix --format` degrade, a non-convergent fixer), and an
   adversarial review of that whole remediation found + fixed two more (the
   baseline walk-exclusion was `check`-only and over-excluding; a vacuous
   M5 regression test). Each fix ships with a revert-sensitive test; all
   tracked in CHANGELOG `[Unreleased]`.
4. **Documentation + site drift (remediation + prevention)** - a 2026-07-03
   two-repo audit (then an adversarial review of its own plan) found unreleased
   v0.14 content leaking onto the live site via THREE main-overlaid vectors
   (the per-rule pages' `root_only`, the LikeC4 model's `baseline` element, and
   `docs/site/reference/**`), stale marketing counts, and a missing prevention
   layer. The plan and the prevention design (a release-aware `x-since` schema
   keyword + arch-model tag-pinning + sourced-or-allowlisted counts, ADR-0007)
   live in [`documentation-drift.md`](./documentation-drift.md).

## Why a hardening pass headlines v0.14

alint's whole value proposition is *trustworthy* governance: the spawn
gate ("extending a ruleset can never run code"), path confinement ("a
rule can never read outside the tree"), and "fail loudly" are load-
bearing security claims that the README, ADRs, the Kani proof and
alint.org all advertise. The audit found that the central one — the
spawn gate — has **two independent bypasses** (templates and nested
configs), and that the gate's design (enumerate spawning kinds at a
single pre-expansion choke point) is the recurring root cause: the same
shape shipped once before with `generated_file_fresh` (`gff`). Closing
these correctly, and re-grounding the security prose in what the code
actually guarantees, is higher priority than any new feature, so it
leads the cut.

## Workstreams

| Doc | Scope |
|---|---|
| [`post_v0.13_audit.md`](./post_v0.13_audit.md) | The full audit findings + the phased remediation plan + per-finding status. The living checklist for the cut. |
| [`ci-fork-pr-isolation.md`](./ci-fork-pr-isolation.md) | Proposal (H6 follow-up): route untrusted fork-PR CI to ephemeral GitHub-hosted runners, keeping it off the self-hosted box. Spec to review before a workflow change lands. |
| [`documentation-drift.md`](./documentation-drift.md) | The 2026-07-03 doc + site-drift audit (adversarially reviewed): consolidated findings across three leak vectors, the resolved decisions (ADR-0007), immediate remediation, and the prevention layer (`x-since` schema keyword + arch-model tag-pin + sourced-or-allowlisted counts + regression tests). §9 records the review's corrections to the first draft. |

## Release shape

The hardening lands phase-by-phase on CHANGELOG `[Unreleased]`, security
first (criticals → highs → mediums → lows/docs), each phase an atomic
commit with a forward `Next: Phase N` pointer (project convention for
large drops). The audit doc's status column is flipped as each finding
lands, mirroring the design-doc "Status: Implemented" cadence. alint.org
fixes land in the site repo in lockstep but are tracked here for a single
source of truth. v0.14 cuts once the CRITICAL + HIGH phases are green and
the MEDIUM/LOW tail is either fixed or explicitly deferred-with-rationale.
