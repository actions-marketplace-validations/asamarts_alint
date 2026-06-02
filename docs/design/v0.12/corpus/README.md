# v0.12 case-study corpus — draft configs

Agent-drafted `.alint.yml` configs produced by the
[100-repo case study](../case_study_100_repos.md), one per repo. Each was
written by a Stage-A analyst agent against a depth-1 clone pinned to the SHA in
[`case_study_repos.md`](../case_study_repos.md), and **validated with
`alint check` against that clone** (every `config_validated: true`) — see the
per-batch findings and coverage metrics in
[`case_study_log.md`](../case_study_log.md).

These are **research artifacts, not shipped examples.** They express each repo's
*addressable* (A-bucket) enforced-validation surface to ground the coverage
numbers and the new-kind / tuning backlog. The polished, audited example configs
live under `examples/` (the calibration repo, `examples/tokio-rs-tokio/`, is the
reference). This directory is `ignore:`d by the repo's own `.alint.yml` — these
are other repos' configs, so dogfooding alint over them is meaningless.

The configs are pinned to the study's clone SHAs; re-running them targets those
trees, not current upstream.

## Contents

- **Batch 1:** `pallets-flask`, `prometheus-prometheus`, `rubocop-rubocop`,
  `curl-curl`, `eslint-eslint`.
- **Batch 2:** `django-django`, `pydantic-pydantic`,
  `spring-projects-spring-boot`, `symfony-symfony`, `redis-redis`,
  `hashicorp-terraform`, `vitejs-vite`, `BurntSushi-ripgrep`,
  `dotnet-aspnetcore`, `phoenixframework-phoenix`.

Later batches append here as they pass their checkpoint.
