# 100+ repo real-world case study — protocol

Status: **Protocol — hardened 2026-06-01. Ready for review; no repos are
touched until it's approved.** The vehicle for the v0.12 cut; runs first
because its findings re-rank the gap backlog and gate the study-gated
kinds (notably the generic [`file_dependency_graph.md`](./file_dependency_graph.md)).
Builds on the proven 30-repo method
([`case-study-v011-reanalysis-log.md`](../../development/case-study-v011-reanalysis-log.md))
and hardens it for scale, comparability, and comprehensiveness.

## Goal — and what "comprehensive" means here

Map, classify, and coverage-measure the *entire bespoke-validation
surface* of 100+ real repos — **comparably across wildly different repo
shapes** — to produce: (1) a reproducible per-repo record, (2) a
demand-ranked new-kind / tuning backlog with corpus evidence, and (3) a
trustworthy coverage trend vs the 30-repo baseline.

"Comprehensive" here means two things that pull in different directions,
and the protocol must satisfy both: **no validation modality is
*structurally* missed**, and **each repo's idiosyncrasies are actively
hunted**, not flattened into a common template.

## Design principle: comprehensive lens, adaptive method

Repos differ enormously — a Bazel hyperscale monorepo, a mid-size
single-language Python library, a JVM/Gradle project, and a Nix flake
expose validation in completely different places and forms. A rigid
rubric would give comparability at the cost of missing repo-specific
shapes; a loose method would miss things. The resolution has three parts:

- A **fixed where-to-look superset** (below) so no *category* is
  structurally overlooked. A given repo exercises only a subset; the
  *absence* of a category is recorded as a finding about the corpus, not
  a hole in the method.
- An **open "what is unusual about THIS repo?" scan** on every repo — a
  deliberate pass for bespoke modalities the superset doesn't name (a
  home-grown validator, an exotic CI gate, a domain-specific invariant).
- An **adversarial completeness verify** (Stage B) framed *for this
  repo's specific shape*: "given this is a {build system} project of
  size {N} in domain {D} under {governance}, what enforced check did
  Stage A miss, mis-classify, or under-count?"

Comparability across the heterogeneous corpus comes from the **metric**
and the **classification taxonomy** being uniform — NOT from forcing
every repo through one shape.

## Repo selection — a stratified, pinned frame (not a wish-list)

Step 0 of the study produces and commits the final 100+ list. This
defines how it is chosen.

**Stratification axes** (aim for spread across each; do not require a
repo per cell):
- **Ecosystem:** Rust, Go, JS/TS, Python, JVM (Java/Kotlin/Scala), Ruby,
  PHP, C/C++, .NET, Swift, Nix, Dart, Elixir, Haskell, shell-heavy.
- **Size tier:** hyperscale monorepo / large / **mid-size
  single-language library (deliberately over-weighted** — the motivation
  below: highest alint fit, lowest current representation) / small.
- **Build system:** make/just/task, nox/tox, Gradle/Maven, cargo+xtask,
  Bazel, npm/pnpm/yarn scripts, CMake, Nix.
- **Governance:** ASF TLP, CNCF, single-vendor (Google/MS/Meta),
  foundation, solo/community maintainer.
- **Domain:** language/compiler, web framework, infra/ops, data/ML,
  systems/DB, library/SDK, CLI tool, docs/site.

**Composition:** re-pin the existing 30 as a baseline tier; add ~70+ to
fill the thin strata — heavy on Python / JVM / Ruby / PHP / systems and
on mid-size single-language libs. Candidate pool (illustrative, not
final): django, pandas, scikit-learn, poetry, black, mypy; spring-boot,
gradle, kotlin, guava, netty; rails, jekyll, laravel, symfony, composer;
llvm, postgres, redis, sqlite, curl, ffmpeg, openssl; terraform,
ansible, grafana, prometheus, dbt, vault; transformers, jax, ray; vue,
svelte, vite, astro; plus a long tail of mid-size libraries across all
of the above.

**Motivation for the weighting.** The 30-repo corpus skewed to big-tech
monorepos heavy on Rust/Go/JS and light on Python/JVM/Ruby/PHP/systems,
with almost no mid-size single-language libs — exactly where the
bespoke-validation / alint fit is often *highest* (those projects
hand-roll shell/CI checks that alint expresses declaratively).

**Provenance.** Every repo is pinned to a **commit SHA + clone date** in
its record. Re-runs target the pin, so configs and coverage numbers are
reproducible and don't silently rot against a moving upstream. (The
30-repo pass shallow-cloned "current upstream" with no pin — not
reproducible; fixed here.)

## Per-repo protocol (two stages)

**Stage A — deep-read (one agent):**
1. Clone at the pinned SHA (full clone when blame/history matters; record
   the depth used).
2. Catalogue every enforced validation against the **where-to-look
   superset**, PLUS the open "what is unusual here?" scan.
3. Classify each finding (taxonomy below). For any file→file reference
   graph the repo enforces (cycles / dangling / orphans / generated-file
   freshness / layered or forbidden edges), record the **edge source**
   (content regex, naming convention, or manifest declaration) — the
   evidence gate for `file_dependency_graph.md`.
4. Draft `examples/<repo>/.alint.yml` expressing as much as is
   reasonable; note genuine non-replaceables and why.
5. Record provenance + the **structured classified catalogue**. (The old
   ≤350-word report cap is lifted — it traded depth for throughput; the
   *synthesis* stays concise, the catalogue is exhaustive.)

**Stage B — adversarial completeness verify (a different agent):**
- Tasked to *refute* Stage A's completeness for this repo's specific
  shape: name validation it likely missed, mis-classified, or
  under-counted, and re-examine the highest-risk where-to-look
  categories for that build system / domain.
- Discrepancies reconcile into the final record. This is the
  comprehensiveness + consistency backstop across ~20 batches — without
  it, "exhaustively catalogue" is only as good as one agent's diligence.

**Parent integrates:** `alint validate-config` + `alint check` against
the clone (config loads, expected violations fire), copies into the
corpus, records, preflights, commits per batch (push verified by ref).

## Where-to-look superset (scan all; each repo exercises a subset)

- **CI:** `.github/workflows/**` incl. reusable (`workflow_call`) +
  composite actions; GitLab CI, CircleCI, Azure Pipelines, Jenkins,
  Buildkite, Prow/Tide.
- **Pre-commit / hooks:** `.pre-commit-config.yaml`, lefthook, husky,
  committed `.git` hooks under `hooks/` / `githooks/`.
- **Build-tool tasks:** Makefile, justfile, Taskfile, nox/tox,
  Gradle/Maven, cargo `xtask`, npm/pnpm/yarn scripts, CMake, Bazel
  (`BUILD`/`.bzl`, buildifier), Nix flake checks.
- **Bespoke validators:** `scripts/`, `tools/`, `ci/`, `hack/`, `dev/`,
  `contrib/` — home-grown `check_*` / `validate_*` / `lint_*` /
  `verify_*`.
- **Conventions docs:** CONTRIBUTING, `docs/development`, style guides,
  ADRs — the rules humans enforce in review but no tool checks yet.
- **Metadata:** CODEOWNERS, `.editorconfig`, `.gitattributes`,
  renovate/dependabot, `.gitignore` invariants.
- **Codegen freshness:** `make gen && git diff --exit-code` patterns.
- **Import / layering / architecture:** import firewalls,
  module-boundary checks, dependency allowlists.
- **Lockfile / hash / SBOM:** lockfile presence/uniqueness, vendored-dep
  hashes, checksum manifests.
- **License / header / SPDX; structure invariants** (every package has
  X; no Y under Z); **schema / API-compat** (golden files, API
  snapshots); **docs-as-code** (link checks, doc-build gates).
- **Open scan:** anything bespoke the above does not name.

## Classification taxonomy (uniform → comparable)

Each catalogued validation → exactly one bucket:
- **A. Expressible today** — name the alint kind(s) / bundle.
- **B. New-kind candidate** — the missing primitive + gap shape (+ edge
  source if graph-like).
- **C. Tuning candidate** — expressible but awkward today (wants a
  preset, a `normalize:`, a bundle relaxation, etc.).
- **D. Deliberate non-goal** — AST/type checks, test/build execution,
  semantic dependency-graph resolution, runtime/network. Recorded *with
  the reason*, so the denominator is honest.

## Coverage metric (precise, granularity-locked)

The 30-repo numbers were not comparable: the unit drifted (angular
`23 classes (~121 micro)` vs spark `~28 (~84 surfaces)` vs airflow
`~165`), and all were `~` estimates. Lock it:

- **Unit = one enforced validation behavior** — a check that would
  independently pass/fail and that a maintainer would call "a rule."
  NOT micro-instances (every file a rule scans), NOT broad "classes"
  lumping many rules. When unsure, count at "one CI step / one script's
  one assertion."
- Report raw **A / B / C / D counts** per repo (auditable, not just a
  headline percentage), then derive:
  - **`coverage_today` = A / (A+B+C)** — share of the *addressable*
    surface alint expresses cleanly now (non-goals D excluded from the
    denominator, since they are out of scope by design).
  - **`coverage_with_tuning` = (A+C) / (A+B+C)** — adds the cheap
    expressible-but-awkward cases.
  - **`gap = B / (A+B+C)`** — the share that needs a *new primitive*;
    this is the demand signal that feeds the backlog ranking.

Two independent counters (Stage A author + Stage B verifier) plus the
calibration example below keep the unit applied consistently.

## Consistency + drift control (across ~20 batches)

- A **locked rubric + hardened syntax cheatsheet** handed to every agent
  up front (the 30-repo pass showed this cuts integration churn to ~1
  fix/batch).
- A **calibration worked-example**: one repo fully worked under this
  protocol, read by every agent first, so the unit + taxonomy are
  applied the same way batch to batch.
- A **mid-study spot-audit** of ~10% of repos with a fresh Stage-B pass,
  to catch drift before the aggregate synthesis.

## Outputs

- **Per-repo record:** pinned SHA + date, the classified catalogue
  (A/B/C/D counts + items), `coverage_today` + `coverage_with_tuning` +
  `gap`, repo-specific notes, the file-graph edge-shape harvest, and the
  committed `examples/<repo>/.alint.yml`.
- **Aggregate:** a demand-ranked backlog (signal count × ecosystem
  spread × severity), new-kind candidates with corpus evidence, the
  refreshed coverage trend vs the 30-repo baseline, and the file-graph
  go/no-go verdict.

## Open questions (carried)

- Storage: 100+ configs in-repo (`examples/`) vs a separate corpus repo
  (`cargo package` size, `examples-validate` runtime).
- How much runs as a release-gating CI job vs a one-time research
  artifact.
- Per-stratum repo counts (how many mid-size libs vs hyperscale).
- Pin-refresh policy: do the committed configs track upstream over time,
  or freeze at the study's pins?
