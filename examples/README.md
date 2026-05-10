# Examples

Real-world `.alint.yml` configurations from the launch-prep validation pass
(see [`docs/development/launch-evidence.md`](../docs/development/launch-evidence.md)).
Each subdirectory is one case study — a popular OSS repo's existing
structural-validation tooling inventoried, rebuilt as an alint config,
and compared.

> Marketing/positioning writeups for each case study live at
> https://alint.org/examples/. This index is the engineering reference:
> directory layout, factual one-liner per repo, contribution workflow.

## v0.9.20 reconciliation pass (2026-05-10)

The 30 per-example READMEs were last walked against alint v0.9.17
(2026-05-06). Since then:

- **v0.9.17 engine fixes** — pitfall #18 (per-rule
  `respect_gitignore: false` knob; demoed in `bazel`, `flutter`),
  pitfall #19 (`literal_is_nested` runtime guard).
- **v0.9.18 fix wave** (~10k false-positive eliminations across all
  30 trees):
  - **A1** `hygiene-no-js-build-outputs` requires sibling
    `package.json` — closes FP classes in 9 case studies (kubernetes,
    deno, flutter, golang-go, vscode, nixpkgs, node, next.js, turbo).
  - **A2** `apache-2-source-has-license-header` bundles the long-form
    ASF preamble — closes FPs in 4 (airflow, arrow, spark, tensorflow).
  - **A3** `python@v1` default-excludes test-fixture paths — closes
    FPs in 2 (ruff, cpython).
  - **A4** `monorepo/cargo-workspace` selector parses `[workspace]`
    members — applies to 6 (uv, clap, deno, next.js, turbo, dotnet).
  - **A5** `oss-license-exists` recognises `LICENSE.TXT` / `LICENSE.md`
    — closes FPs in 4 (arrow, deno, dotnet, tensorflow).
  - **A6** `rust@v1 rust-sources-snake-case` gains
    `allow_compiler_naming` knob — closes FPs in 2 (clap, rust-lang/rust).
  - **B1 / B2 / B3** pitfall #22 (YAML `|` → `|-`) fixes in
    TypeScript / deno / tensorflow configs.
  - **kubernetes deep-analysis pilot** (commit `c5b6df32`) — 3
    `.alint.yml` regex bugs fixed (pitfalls #13 / #14 / #22),
    eliminating ~34,000 false positives that the v0.9.17 walk had
    flagged as "P0 for parent-agent triage."
  - `dir_absent` engine extension — now supports `scope_filter`.
- **v0.9.19 / v0.9.20** — width-aware human output across every
  command; bundled rule message audit; em-dash scrub; install-snippet
  reorder (curl + bash now leads everywhere).

Each per-example README now tags v0.9.17 capture counts as historical
and names which fix resolved which previously-flagged FP. Counts have
not been re-walked under v0.9.20 (per-tree clones are expensive and
absolute counts drift with upstream tip); category-level findings are
stable.

## Layout

```
examples/
├── README.md                          # this file
├── <owner>-<repo>/
│   ├── README.md                      # case study writeup
│   ├── .alint.yml                     # the alint config that matches their existing tooling
│   ├── existing-tooling.md            # inventory of what they enforce today (where present)
│   └── comparison.md                  # alint output vs existing tool output + perf delta (where present)
```

## Case studies

30 of planned 40 (P2a complete — 20 of 20; P2b Wave 1+2 — 10 of planned 20
polyglot monorepos; remaining 10 polyglot repos queued for post-launch).
Listed alphabetically. Each entry: a factual one-liner with the rule count
from `alint validate-config` against the case study's `.alint.yml`.

- [`angular-angular/`](angular-angular/) — TypeScript framework with 16 packages; `goldens/public-api/<pkg>/index.api.md` discipline locks the TS API surface of 13 of 16 packages.
- [`apache-airflow/`](apache-airflow/) — 109 pre-commit hooks; ~40% map to alint declaratively.
- [`apache-arrow/`](apache-arrow/) — 6 languages in one tree (C++/Java/Python/Rust/Go/JS); 21 lint hooks across 14 tool repos. Live findings: 16 source files missing the Apache header (all listed in `dev/release/rat_exclude_files.txt`).
- [`apache-spark/`](apache-spark/) — 49 `pom.xml` files; surfaces the v0.10 ship-target `xml_path_matches` / `xml_path_equals` rule kinds.
- [`astral-sh-ruff/`](astral-sh-ruff/) — 900+ Python lint rules but zero rules for ruff's own internal-crate `publish = false` discipline.
- [`astral-sh-uv/`](astral-sh-uv/) — 67-crate workspace conventions enforced nowhere in CI today.
- [`bazelbuild-bazel/`](bazelbuild-bazel/) — surfaces pitfall #18 (`.bazelversion` tracked-AND-gitignored), fixed in v0.9.17 via the per-rule `respect_gitignore: false` knob; the case-study config demonstrates the fix.
- [`clap-rs-clap/`](clap-rs-clap/) — Rust workspace; per-member inheritance via `for_each_dir` over family crates.
- [`denoland-deno/`](denoland-deno/) — Rust + JS + TS multi-language; custom validation scripts in `tools/lint.js`.
- [`dotnet-runtime/`](dotnet-runtime/) — 1,091 `.csproj` files (sparse checkout) + 234 solution files + 257 `Directory.Build.{props,targets}` + 520 `.props/.targets` ≈ 2,300 distinct XML manifests; demand-validates `xml_path_*` at one OOM bigger scale than spark; `dotnet@v1` bundled-ruleset gap.
- [`facebook-react/`](facebook-react/) — `codes.json` registry shape + `ReactVersion.js` propagated to 3 per-package fields.
- [`flutter-flutter/`](flutter-flutter/) — Dart framework + native-OS embedders (Android/iOS/macOS/Linux/Windows/Fuchsia/GLFW + ABI) as peer subdirs under `engine/src/flutter/shell/platform/`. Live findings: 5 Trojan-Source / [CVE-2021-42574](https://nvd.nist.gov/vuln/detail/CVE-2021-42574) errors in `docs/releases/archive/` via `oss-baseline`'s `no_bidi_controls`.
- [`golang-go/`](golang-go/) — zero `.github/workflows/`, zero `Makefile`, zero `.golangci.yml`; the alint config encodes the project's structural contract for the first time.
- [`helm-helm/`](helm-helm/) — Trojan-Source defence + GHA hardening on top of golangci-lint.
- [`istio-istio/`](istio-istio/) — Single-module Go monorepo with 9 Helm charts + Prow CI + CODEOWNERS (not k8s-OWNERS). Per-chart image-hub at *different* JSONPath positions per file — surfaces pitfall #20 + the `value_extractor:` v0.10 design candidate. Multi-doc YAML release-notes file surfaces pitfall #21.
- [`kubernetes-kubernetes/`](kubernetes-kubernetes/) — 50 verify scripts inventoried; alint replaces 17 declaratively.
- [`microsoft-typescript/`](microsoft-typescript/) — eslint + dprint + knip already tight; alint adds the structural floor.
- [`microsoft-vscode/`](microsoft-vscode/) — apples-to-apples vs `build/hygiene.ts`. Covers ~75% of the 8 distinct hygiene checks (6 of 8) declaratively in one config; verified against the live tree (222 violations, zero false positives).
- [`nixos-nixpkgs/`](nixos-nixpkgs/) — 39,101 files + 20,678 `pkgs/by-name/*/*/` package directories. Full 79-rule pass — including `for_each_dir` over the by-name tree — completes in 273 ms wall-clock.
- [`nodejs-node/`](nodejs-node/) — 15-year-old conventions enforced via human review only.
- [`pnpm-pnpm/`](pnpm-pnpm/) — replaces the in-tree `meta-updater` plugin's 13 cross-package field invariants without a per-repo plugin install.
- [`prettier-prettier/`](prettier-prettier/) — 5 net-new gates on top of eslint + prettier + cspell + knip + tsc.
- [`protocolbuffers-protobuf/`](protocolbuffers-protobuf/) — 10 in-tree language bindings (cpp, java, python, csharp, ruby, php, objc, hpb, upb, rust) + 1 spun-out (dart); per-binding `failure_list_<lang>.txt` files; per-binding GHA test workflow. ~45 cross-language assertions one rule would express.
- [`python-cpython/`](python-cpython/) — 56 surfaces inventoried; one alint config consolidates the 38% that's declarative orchestration.
- [`pytorch-pytorch/`](pytorch-pytorch/) — ≈86% of pytorch's 57 `lintrunner.toml` adapters are structural; alint sits beneath, lintrunner keeps the AST-aware tail.
- [`rust-lang-rust/`](rust-lang-rust/) — `src/tools/tidy/` is a custom Rust binary doing alint's job; ~13 of ~32 tidy checks become declarative.
- [`tensorflow-tensorflow/`](tensorflow-tensorflow/) — 1,185 textproto API goldens under `tensorflow/python/tools/api/golden/{v1,v2}/`; demand-validates `cross_language_implementation_complete` at TWO topologies (per-source ↔ per-test within one language; core ↔ N bindings across languages).
- [`tokio-rs-tokio/`](tokio-rs-tokio/) — zero hand-rolled scripts; alint catches 15 conventions tokio's pipeline assumes.
- [`vercel-next.js/`](vercel-next.js/) — first hybrid pnpm + Cargo dual-workspace case in the corpus; drift no per-language linter catches because each linter only sees half the tree.
- [`vercel-turbo/`](vercel-turbo/) — Rust monorepo orchestrator; alint adds 22 gates that don't exist.

## Primitive demand tracker

Aggregated from the per-case-study gap analysis (the "primitives still
needed" call-outs in §9 of each per-example README). Status: ⏳ pending
v0.10 ship · 💭 v0.10 design candidate · 🛣️ v0.11+ ship.

### Core rule kinds — v0.10 ship targets

| Primitive | Status | Demand sources |
|---|---|---|
| `cross_file_value_equals` (incl. `value_extractor:`) | ⏳ | angular, airflow, helm, istio, vscode, node, pnpm, pytorch, tensorflow, tokio, next.js, turbo |
| `xml_path_matches` + `xml_path_equals` | ⏳ | spark, dotnet-runtime |
| `registry_paths_resolve` | ⏳ | arrow, spark, dotnet, flutter, kubernetes, nixpkgs, node, protobuf, cpython, pytorch, rust-lang/rust, tensorflow, next.js |
| `ordered_block` | ⏳ | airflow, spark, flutter, golang-go, protobuf, cpython, rust-lang/rust, tokio |
| `generated_file_fresh` | ⏳ | airflow, spark, kubernetes, nixpkgs, protobuf, cpython, pytorch, tensorflow |
| `import_gate` | ⏳ | airflow, golang-go, helm, kubernetes, pytorch |
| `pair_hash` | ⏳ | golang-go, kubernetes, tokio |
| `command_idempotent` | ⏳ | ruff, helm, prettier |

### Bundled rulesets — v0.10 ship targets

| Ruleset | Status | Demand sources |
|---|---|---|
| `apache/governance@v1` | ⏳ | airflow, arrow, spark |
| `dotnet@v1` | ⏳ | dotnet-runtime |

### v0.10 design candidates

| Primitive | Status | Demand sources |
|---|---|---|
| `*_path_contains` shorthand for "value X in array at JSONPath Y" | 💭 | bazel, clap, helm |
| `pair_inverse` (every partner traces back to a primary) | 💭 | angular, ruff |
| `command_per_repo` mode | 💭 | ruff |
| `json_schema_passes` config-shape mode | 💭 | kubernetes, turbo |
| `*_path_array_iter` (toml/json/yaml) for workspace iteration | 💭 | uv |

### v0.11+ ship targets

| Primitive | Status | Demand sources |
|---|---|---|
| `cross_language_implementation_complete` | 🛣️ | flutter, protobuf, tensorflow |
| Bazel-licensing-declaration-aware rule kind | 🛣️ | tensorflow |
| `walk_error_policy:` knob | 🛣️ | pnpm |

### Emerging gaps (surfaced this audit; not yet on roadmap)

Engine refinements pulled from per-case-study §9s, all single-source or
two-source so far. Listed for completeness; promotion to v0.10 / v0.11
ship targets pending second demand source:

- **Engine knobs:** `select_from:` for monorepo/cargo-workspace (uv,
  deno), `multi_doc_mode:` for `yaml_path_*` (istio), `{stem_all}`
  template token (typescript), `Format::Jsonc` for structured-query
  rules (typescript).
- **New rule kinds:** `dir_name_matches_field` (turbo, next.js),
  `file_pair_block_match` (cpython, rust-lang/rust), `balanced_delimiters`
  (cpython, rust-lang/rust), `archive_contents_matches` (uv),
  `referenced_files_match_filesystem` (deno), `violation_baseline`
  (deno), `dir_contents_match_allowlist` (deno),
  `disallowed_methods_in_file` (deno), `regex_resolves_in_file` (clap),
  `file_content_matches_or_marker` (vscode), `file_header_consistency`
  (node), `column_alignment` (cpython), `line_spacing` /
  `not_executable` / `directory_hash` (pytorch), `markdown_template_match`
  / `case_collision_safe` (tensorflow), `for_each_leaf_dir` (prettier),
  `json_key_value_forbidden` (prettier), `json_key_sort_order` (pnpm).
- **New bundled rulesets:** `azure-pipelines@v1` (dotnet),
  `python/pep-621-shape@v1` (uv), `rust/cargo-release-conventions@v1`
  (clap).

## Using these as starting points

Each `<owner>-<repo>/.alint.yml` is a working config. To use one as a starting
point for your own repo:

```sh
curl -fsSL https://raw.githubusercontent.com/asamarts/alint/main/examples/<owner>-<repo>/.alint.yml \
  > .alint.yml
alint check
```

Trim what doesn't apply to your repo, add what's specific. The configs are
deliberately written to be readable + adaptable, not minimal.

## Contributing a case study

If you've adopted alint for a public repo, consider contributing the case
study back — it helps other users with similar repo shapes.

The per-repo workflow ([`docs/development/launch-evidence.md`](../docs/development/launch-evidence.md#per-repo-case-study-contribution-workflow)) describes the steps.
