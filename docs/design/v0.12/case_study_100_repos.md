# 100+ repo real-world case study

Status: **Planned (v0.12).** Drafted 2026-05-25. The vehicle for the
whole v0.12 cut; runs first because it may reshuffle the gap ranking
and surface new candidates.

## Motivation

The 30-repo re-analysis
([`case-study-v011-reanalysis-log.md`](../../development/case-study-v011-reanalysis-log.md))
validated alint's coverage but its corpus skewed: big-tech monorepos,
heavy on Rust/Go/JS, light on Python/JVM/Ruby/PHP/systems, and almost
no mid-size single-language libraries (where the bespoke-validation /
alint fit is often *highest* because those projects hand-roll
shell/CI checks that alint expresses declaratively).

## Scope

- **100+ repos**, deliberately broadening ecosystem coverage. Candidate
  additions beyond the existing 30:
  - Python: django, pandas, scikit-learn, fastapi, poetry, pip, black, mypy
  - JVM: spring-boot, gradle, kotlin, guava, netty
  - Ruby: rails, jekyll; PHP: laravel, symfony, composer
  - Systems: llvm, postgres, redis, sqlite, curl, ffmpeg, openssl
  - Infra/data: terraform, ansible, grafana, prometheus, dbt, vault
  - ML: huggingface/transformers, jax, ray
  - Web: vue, svelte, solid, vite, astro
  - Long tail: mid-size single-language libs across all the above
- Keep the existing 30 as a baseline tier; the expansion is additive.

## Method (proven in the 30-repo pass)

Subagent batch-orchestration, ~5 repos/batch:

1. Subagent clones to `/tmp/casestudy/<slug>`, catalogues every bespoke
   validation, drafts `proposed.alint.yml` against the full capability
   set, returns a ≤350-word report (counts + new-kind candidates).
2. Parent validates each draft with `alint validate-config` (the
   `examples-validate` CI gate), fixes any schema slips, copies into
   `examples/`, updates the findings log + per-repo README, preflights,
   commits per batch (push verified by ref).
3. Hand subagents a hardened syntax cheatsheet up front — the 30-repo
   pass showed this cuts integration churn to ~1 fix/batch.

## Outputs

- Per-repo example configs (a new `examples/` tier or an extension of
  the current one — decide whether 100+ configs live in-repo or in a
  separate corpus to keep `cargo package` lean).
- An expanded findings log that **supersedes** the 30-repo synthesis.
- A refreshed, demand-ranked gap list that re-prioritises the other
  v0.12 workstreams (and may add new ones).

## Open questions

- In-repo vs. separate corpus repo for 100+ configs (binary/package
  size, `examples-validate` runtime).
- How much of the study can run as a release-gating CI job vs. a
  one-time research artifact.
- Whether to dedupe ecosystems already well-covered (e.g. cap Rust
  workspaces) to maximise *new* validation patterns per repo.
