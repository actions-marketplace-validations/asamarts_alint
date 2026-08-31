# Case-study v0.11 re-analysis log

Master tracking for a deep re-analysis of all 30 real-world OSS case
studies, checking how much MORE of each repo's bespoke
manual/script/CI validation alint can replace now that the v0.11
rule-kind + bundled-ruleset work has shipped. (The earlier
`case-study-revalidation-log.md` pass fixed drift; this pass re-derives
coverage against current upstreams + v0.11 capabilities.)

## Why now

The case-study configs were authored in the **v0.9.17 era**, so the
"unlock set" they predate is **everything from v0.10 onward**. (Note on
attribution: the rule *kinds* below shipped in **v0.10**; **v0.11** added
the commit-validation family + `scope_filter.changed_since:` +
`{{env.X}}` interpolation + the LSP/editor work. The subagents flagged
this; the original framing mislabeled the kinds as v0.11.)

v0.10 rule kinds that close gaps the original configs flagged as "alint
can't express this yet":

- `import_gate` — regex import firewall (allow-globs + go/python/rust/js
  presets; `generic` needs an explicit `import_pattern` — there is no
  scala/java preset yet, a confirmed gap): bespoke layering / "don't
  import X from Y" checks.
- `ordered_block` — keep-sorted regions.
- `generated_file_fresh` — runs a declared generator and diffs its
  stdout (non-mutating): "did you re-run codegen?" checks.
- `command_idempotent` — runs a user checker in `--check` mode
  (`files_from`/`files_pattern` -> per-file violations): replaces
  per-file `command:` fan-out (the big perf win) + check-mode gates.
- `pair_hash` — a source's digest must appear in a target.
- `xml_path_equals` / `xml_path_matches` — structured XML: replaces
  bespoke `pom.xml` (Maven) / `.csproj` (.NET) assertions.
- `registry_paths_resolve` / `cross_file_value_equals` — registry
  entry-resolution + cross-file value sync.

v0.11 additions: commit-validation rules (`git_commit_signed_off`,
`_no_fixup`, `_author_allowlist`, `_gpg_signed`), `scope_filter.changed_since:`
(grandfather pre-existing tree, fire only on PR-touched files),
`{{env.X}}` interpolation. Bundled rulesets `dotnet@v1` +
`apache/governance@v1` (the ASF projects: airflow, arrow, spark).

## Methodology (per repo)

1. Shallow-clone the current upstream (`git clone --depth 1`).
2. Exhaustively catalogue existing validations (CI workflows, Makefiles,
   `scripts/`/`dev/`/`tools/`, pre-commit, lint configs, license-header,
   codegen-freshness, import/layering, keep-sorted, lockfile/hash,
   commit hooks).
3. Map each to an alint rule kind, emphasizing the v0.11 additions; note
   genuine non-replaceables (type-checking, test execution) and why.
4. Rewrite `examples/<repo>/.alint.yml` to replace as much as
   reasonable.
5. Verify by running `alint check` against the clone (config loads,
   expected violations fire).
6. Compare coverage / speed / maintainability / readability vs the
   originals and vs the prior config.

Subagents do steps 1-3 + draft the config + the comparison; the parent
integrates (4), verifies (5), and records (6) here, committing per
batch.

## Coverage summary (filled per batch)

| Repo | Validations catalogued | alint-replaceable | NEW via v0.10+ | Key kinds used | Config rules |
|---|---|---|---|---|---|
| angular-angular | 23 classes (~121 micro) | ~70 shape + 8 orchestrated | +4 | import_gate, command_idempotent, generated_file_fresh, git_commit_no_fixup | 128 |
| apache-airflow | ~165 | ~85 (52%, was 35%) | +27 | import_gate, cross_file_value_equals, command_idempotent, git_commit_signed_off/no_fixup, apache/governance@v1 | 90 |
| apache-arrow | ~80 | ~64 (~80%) | +6 | registry_paths_resolve, command_idempotent, import_gate, git_commit_no_fixup, apache/governance@v1 | 114 |
| apache-spark | ~28 (~84 surfaces) | ~74% | +7 | xml_path_* (pom.xml), registry_paths_resolve, import_gate, command_idempotent, git_commit_no_fixup, apache/governance@v1 | 110 |
| astral-sh-ruff | ~30 | ~22 | +8 | command_idempotent, generated_file_fresh, import_gate, git_commit_no_fixup, changed_since, {{env.X}} | 80 |
| clap-rs-clap | ~38 | ~30 | +9 | toml_path_*, cross_file_value_equals, command_idempotent, generated_file_fresh, git_commit_no_fixup | 76 |
| denoland-deno | ~31 | ~22 | +6 | command_idempotent, import_gate, json_path_*, git_commit_no_fixup | 81 |
| astral-sh-uv | ~30 | ~22 | +8 | command_idempotent, generated_file_fresh, registry_paths_resolve, cross_file_value_equals, import_gate, git_commit_gpg_signed, {{env.X}}, changed_since | 89 |
| bazelbuild-bazel | ~55 | ~38 (~69%) | +5 | import_gate, cross_file_value_equals, git_commit_no_fixup, changed_since | 86 |
| dotnet-runtime | ~55 | ~46 (~84%) | +9 | dotnet@v1, json_path_*, xml_path_*, cross_file_value_equals, git_commit_no_fixup, changed_since | 59 |
| facebook-react | ~31 | ~24 (~77%) | +4 | cross_file_value_equals, registry_paths_resolve, generated_file_fresh, git_commit_no_fixup, changed_since | 82 |
| flutter-flutter | ~58 | ~34 (~59%) | +9 | import_gate, command_idempotent, cross_file_value_equals, file_header, file_content_forbidden, changed_since | 58 |
| golang-go | ~22 | ~16 (~73%) | +5 | import_gate (go preset), command_idempotent, pair_hash, registry_paths_resolve, generated_file_fresh | 62 |
| helm-helm | ~23 | ~18 (~78%) | +5 | git_commit_signed_off, import_gate (go preset), command_idempotent, cross_file_value_equals, ordered_block | 58 |
| istio-istio | ~26 | ~21 (~81%) | +6 | git_commit_signed_off, import_gate (go preset), command_idempotent, cross_file_value_equals | 64 |
| kubernetes-kubernetes | ~51 | ~34 (~67%) | +10 | import_gate (go preset), file_header, command_idempotent, for_each_dir | 58 |
| microsoft-typescript | ~27 | ~18 (~67%) | +3 | cross_file_value_equals, registry_paths_resolve, import_gate, command_idempotent | 71 |
| microsoft-vscode | ~58 | ~33 (~57%) | +6 | import_gate, cross_file_value_equals, file_header, file_content_forbidden | 68 |
| nixos-nixpkgs | ~21 | ~14 (~67%) | +3 | ordered_block, command_idempotent, for_each_dir | 58 |
| nodejs-node | ~34 | ~22 (~65%) | +4 | command_idempotent, ordered_block, git_commit_no_fixup, registry_paths_resolve | 61 |
| pnpm-pnpm | ~31 | ~24 (~77%) | +6 | registry_paths_resolve, toml_path_*, command_idempotent, for_each_dir | 92 |
| prettier-prettier | ~28 | ~21 (~75%) | +6 | command_idempotent, file_content_forbidden, json_path_matches, for_each_dir | 65 |
| protocolbuffers-protobuf | ~58 | ~47 (~81%) | +19 | file_header, command_idempotent, cross_file_value_equals, ordered_block | 45 |
| python-cpython | ~31 | ~25 (~81%) | +8 | command_idempotent, cross_file_value_equals, registry_paths_resolve | 78 |
| pytorch-pytorch | ~63 | ~49 (~78%) | +6 | command_idempotent, generated_file_fresh, registry_paths_resolve, cross_file_value_equals, import_gate | 61 |
| rust-lang-rust | ~34 | ~20 (~59%) | +6 | ordered_block, registry_paths_resolve, generated_file_fresh, command_idempotent, file_content_forbidden | 68 |
| tensorflow-tensorflow | ~41 | ~32 (~78%) | +9 | command_idempotent, registry_paths_resolve, cross_file_value_equals, file_header | 58 |
| tokio-rs-tokio | ~24 | ~20 (~83%) | +5 | cross_file_value_equals, pair_hash, ordered_block, command_idempotent, for_each_dir | 73 |
| vercel-next.js | ~61 | ~50 (~82%) | +4 | registry_paths_resolve, command_idempotent, for_each_dir, toml_path_matches | 115 |
| vercel-turbo | ~38 | ~31 (~82%) | +6 | cross_file_value_equals, registry_paths_resolve, import_gate, command_idempotent | 91 |

## Per-batch findings

### Batch 1 — angular, airflow, arrow, spark, ruff

All 5 configs rewritten + validated (`alint validate-config`); rule
counts in the table above. Live `alint check` against the clones was
deliberately NOT run: the configs contain `command_idempotent` /
`generated_file_fresh` rules that shell out to each repo's own tooling
(cargo dev, scalastyle, buf), which isn't installed here and is the
repos' own CI's job; `validate-config` is also the actual
`examples-validate` CI gate.

- **angular** — `import_gate` lifts the ESM/CJS named-import-ban tslint
  rule + a deep cross-package layering firewall; `command_idempotent`
  upgrades 8 generic `command:` wrappers (format gate gains per-file
  offender parsing via `files_from: stdout`); `git_commit_no_fixup`
  newly catches leftover `fixup!` in a PR range. `generated_file_fresh`
  locale-codegen rule shipped as a commented template (angular's
  generator emits via Bazel write-actions, not stdout).
- **airflow** — 35% -> 52% coverage (+27 surfaces). `import_gate` for
  base-operator/session/shared-import firewalls; `cross_file_value_equals`
  for the literal subset of 11 sync hooks; `command_idempotent` collapses
  ~9k codespell / ~7k ruff per-file spawns; DCO + no-fixup now
  expressible. `apache/governance@v1` over-fires twice (no top-level
  KEYS; RELEASE_NOTES.rst vs CHANGELOG) -> needs per-rule `paths:`
  overrides (see cross-cutting).
- **arrow** — ~80%. `registry_paths_resolve` closes the #1 prior gap
  (`rat_exclude_files.txt` entry-resolution); `command_idempotent`
  covers the flatc + cpp11 codegen-freshness gates that **no existing
  linter sees** (they're `linguist-generated` + RAT-excluded).
  Honest negatives: no keep-sorted markers exist (ordered_block N/A);
  ICLA not DCO (signed_off N/A).
- **spark** — ~74%. Headline: `xml_path_*` replaces fragile
  `(?s)<parent><groupId>…` regex with structural `pom.xml` assertions
  (generalizes to every Maven repo). `apache/governance@v1` collapses
  ~11 hand-rolled ASF rules. `import_gate` lifts scalastyle
  `IllegalImportsChecker` — **but** there's no scala preset, so it uses
  `language: generic` + an explicit `import_pattern` (gap: a
  scala/java preset). Spark uses no DCO (JIRA + squash), so
  signed_off/author_allowlist omitted.
- **ruff** — +8. `command_idempotent` expresses the central `cargo dev
  generate-all --mode check` codegen-freshness gate declaratively (prior
  config punted to CI) and collapses ~5,844 per-file formatter spawns;
  `generated_file_fresh` for schema freshness; `import_gate` for
  `ruff_* ↛ ty_*`; `scope_filter.changed_since` + `{{env.X}}` grandfather
  the 1,597 malformed fixtures and fire hygiene only on PR-touched files.

**Integration fixes (subagents didn't run alint, by design):**
`registry_paths_resolve` needs `extract: { lines: {} }` +
`entries_are_globs: true` for newline glob-list registries (arrow +
spark used a non-existent `glob:` field / omitted `extract`); `import_gate`
`generic` requires an explicit `import_pattern` (spark scala rules).

### Batch 2 — uv, bazel, clap, deno, dotnet-runtime

All 5 configs rewritten + validated (`alint validate-config`); like batch
1, live `alint check` was deliberately not run (the configs shell out to
each repo's own tooling via `command_idempotent`/`generated_file_fresh`).

- **dotnet-runtime** — ~84% (+9). The flagship `dotnet@v1` payoff:
  `extends: [alint://bundled/dotnet@v1]` (ecosystem-gated on
  `facts.has_dotnet`) subsumes ~6 hand-written rules (global.json exists,
  `$.sdk.version` json_path pin, csproj Sdk-style, Nullable, bin/obj
  hygiene, .editorconfig). `xml_path_*` replaces every fragile
  `<Project Sdk=...>` substring regex with a real root-attribute
  assertion that ignores commented-out/nested duplicates; `json_path_*`
  pins the Arcade/Helix SDK families in global.json. **Confirmed by
  inspection:** runtime has NO root `Directory.Packages.props` (it pins
  via `eng/Versions.props`, not NuGet CPM) — `dotnet@v1`'s CPM rule is
  `if_present` so it correctly no-ops, no false positive (validates the
  v0.10 `if_present`-everywhere design choice). Residual: `.slnx`
  project-list ↔ on-disk csproj resolution needs an XML-attribute-list
  extractor for `registry_paths_resolve` (new-kind candidate). Genuine
  non-replaceables kept on their tools: MSBuild eval, Roslyn `dotnet
  format` (format.sh is *mutating*, not `--check` — deliberately NOT a
  command_idempotent target), ApiCompat binary diff, Helix.
- **bazel** — ~69% (~38/55, +5). `import_gate` reclaims two textual
  layering firewalls (C++ ↛ JNI headers; `sun.misc.Unsafe` routing);
  `cross_file_value_equals` syncs the `.bazelversion` pin against the
  presubmit config; v0.11 `changed_since` PR-scopes the now-5,729-file
  Java license-header sweep. **Bonus: the re-analysis caught a
  274-false-positive bug in the *original* example config** — a
  BUILD-file-naming rule with `prefix: "#"` that matched almost
  everything. New-kind candidates: `.bazelrc` path resolution;
  `MODULE.bazel.lock` freshness (a `generated_file_fresh` shape, but
  it's a spawning kind, left as a commented template).
- **clap** — ~30/38 (+9). `toml_path_*` (9 equals + 5 matches) asserts
  the workspace `Cargo.toml` shape structurally; `cross_file_value_equals`
  syncs the 4 member-crate version pins to the workspace; `for_each_dir`
  enforces per-crate README/CHANGELOG presence; `command_idempotent`
  upgrades the rustfmt/clippy/typos gates to per-file offender parsing;
  `git_commit_no_fixup` + `git_commit_message` catch PR-range hygiene.
- **deno** — ~22/31 (+6). `command_idempotent` collapses the
  `dprint`/`deno fmt --check` + clippy per-file spawns; `import_gate`
  expresses the cli↛runtime layering rule; `json_path_equals` pins
  shape in the deno.json/config; `git_commit_no_fixup` for PR hygiene.
- **uv** — ~22/30 (+8). `command_idempotent` (ruff check + ruff format
  --check, with per-file offender parsing) replaces the plain
  `command:` shellouts and matches the real mutating pre-commit hooks;
  `generated_file_fresh` for the 4 codegen-freshness gates;
  `registry_paths_resolve` + `cross_file_value_equals` for the
  workspace-member pins; `{{env.UV_RUFF_VERSION}}` interpolation pins
  the shellout tool version to CI's; `git_commit_gpg_signed` (uv signs
  releases). Note: `{{env.X}}` needs the `| default('...')` filter to
  stay valid when the var is unset at parse time.

**Integration fixes:** uv's `{{env.UV_RUFF_VERSION}}` interpolation had
no default, so `validate-config` (no env set) rejected it — added
`| default('latest')` (same lesson as batch 1's ruff, which already had
defaults). The other four validated as-drafted.

### Batch 3 — react, flutter, go, helm, istio

All 5 configs rewritten + validated (`alint validate-config`); live
`alint check` deliberately not run (they shell out to each repo's own
tooling). Integration required many field-name fixes (see below) — the
subagents drafted blind to the real schema.

- **react** — 24/31 (~77%, +4). Headline: `cross_file_value_equals`
  natively replaces `version-check.js` (the exact primitive the v0.9.17
  example called out as a non-replaceable shell-out) — the version
  exported by `ReactVersion.js` must equal `$.version` in the three
  published manifests; a second cross-file rule syncs `ReactVersions.js`
  (release) ↔ `ReactVersion.js` (runtime). `registry_paths_resolve`
  resolves `react/package.json` `files` tarball allow-list on disk;
  `generated_file_fresh` covers `extract-errors` codes.json freshness
  (advisory — needs a built tree). Honest non-replaceables (7): the 5
  in-tree AST eslint rules, flow, jest, dangerfile (PR-diff + bundle-byte
  regression), lint-build (built bundles).
- **flutter** — 34/58 (~59%, +9). The `analyze.dart` prize:
  verifyNoMissingLicense → two `file_header` rules (literal
  `Copyright 201[34]`, NOT a year range); verifyNoTrailingSpaces +
  verifySpacesAfterFlowControlStatements → `file_content_forbidden`;
  verifyNoBadImportsInFlutterTools + verifyNoTestImports → `import_gate`
  (generic + dart `import_pattern`). `cross_file_value_equals` for the
  engine.version ↔ engine.stamp pin (`allow_missing_target: true`).
  `changed_since` grandfathers the ~8k-file legacy tree on the four
  text-sweep rules. Non-replaceables: dart/flutter analyze + clang-tidy,
  the 6 custom_rules (Dart AST), golden pixel-diff. The
  @Deprecated↔notice pairing is genuinely inexpressible (no per-file
  conditional-content rule) and was dropped — stays in analyze.dart.
- **go** — 16/22 (~73%, +5). Headline: the `deps_test.go` package
  firewall → `import_gate` (preset `go`), encoding the high-stakes
  negative edges (runtime ↛ fmt/os/reflect/net; stdlib ↛ cmd/internal).
  `pair_hash` (contains mode) for `fips140.sum` ↔ the CMVP zip digests;
  gofmt -l fan-out → `command_idempotent`. Non-replaceables: cmd/api
  symbol freeze (AST over 25 build tuples); the full *transitive* deps
  DAG closure (import_gate is flat per-file regex). Signoff N/A
  (Gerrit + CLA, no DCO).
- **helm** — 18/23 (~78%, +5). DCO → `git_commit_signed_off` (the
  load-bearing CNCF gate); depguard/gomodguard → `import_gate` (preset
  `go`); `.github/env` Go pin ↔ go.mod → `cross_file_value_equals`;
  golangci "keep sorted" linter list → `ordered_block`; gofmt/golangci/
  tidy → `command_idempotent`. **`compliance/apache-2@v1` over-fires
  (NOT extended)** — helm uses an abbreviated branded header, so the
  canonical ASF/RAT bundle would false-positive on most files; kept
  bespoke `file_header` rules.
- **istio** — 21/26 (~81%, +6). `make gen-check` codegen freshness →
  `command_idempotent` (correctly NOT `generated_file_fresh`: `make gen`
  MUTATES, that kind diffs stdout only); DCO → `git_commit_signed_off`;
  depguard 16-pkg ban + operator/istioctl directory boundary →
  `import_gate` (preset `go`); 5 chart-hub `file_content_matches`
  collapsed to one `cross_file_value_equals`. **`compliance/apache-2@v1`
  over-fires (excluded)** — ~1,700 generated `.gen.go`/`.pb.go` carry no
  header, some say "The Kubernetes Authors", no top-level NOTICE.

**Integration fixes (the subagents' recurring schema errors — fold into
batch 4-6 prompts):** `import_gate` uses `language:` not `preset:`, and
`forbid:` is a SINGLE regex string (not a list — collapse to an
anchored alternation); `allow:` is the list. `cross_file_value_equals`
source/targets use `file:` (not `path:`) + `extract:`, and conditional
presence is `allow_missing_target: true` (not a `when:` map — `when:` is
a fact-expression STRING, there is no `file_contains`/`file_exists`
predicate). `registry_paths_resolve` uses `base:` not `base_dir:`.
`pair_hash` uses `source:` + `target:` + `format:` (not `paths:`/`mode:`).
`generated_file_fresh` takes a single `file:` (not `paths:`).
`ordered_block` uses `start:`/`end:` (not `begin:`) and has no
`item_pattern`. "Forbid a pattern" is `file_content_forbidden` with
`pattern:` (not `file_content_matches` + `forbid:`). `scope_filter` is
PER-RULE only (block form), never a top-level config key.

### Batch 4 — kubernetes, typescript, vscode, nixpkgs, node

All 5 configs rewritten + validated. The hardened syntax cheatsheet in
the subagent prompts (folding in batch-3's recurring schema errors) cut
integration to a single fix across all five (node: `registry_paths_resolve`
does not take `allow_missing_target` — that field is cross-file only).

- **kubernetes** — 34/51 (~67%, +10). The 66 per-directory
  `.import-restrictions` files → `import_gate` (preset `go`); per-language
  boilerplate → `file_header` (year-OPTIONAL — post-2025 k8s drops the
  year: `Copyright ([0-9]{4} )?The Kubernetes Authors`); codegen/vendor
  freshness → `command_idempotent` in verify mode. **Dropped two
  bundles:** `compliance/apache-2@v1` over-fires (branded "Kubernetes
  Authors" header, thousands of generated headers, no NOTICE), and
  `ci/github-actions@v1` (k8s has NO `.github/workflows/` — it runs on
  Prow). EasyCLA not DCO → signoff omitted. Non-replaceables:
  govet/typecheck/internal-modules/vendor-cycles (AST), publishing-bot
  content sync.
- **typescript** — 18/27 (~67%, +3). `cross_file_value_equals` pins the
  dprint TS plugin version (`.dprint.jsonc` wasm URL ↔ package.json
  `@dprint/typescript`); `registry_paths_resolve` resolves
  `src/lib/libs.json` `$.libs[*]` → the `.d.ts` sources exist;
  `import_gate` (js) as a coarse stand-in for a no-direct-import AST
  rule. Non-replaceables: all 9 custom AST eslint rules, the baseline
  accept/diff loop, generated-lib freshness (mutating generators).
- **vscode** — 33/58 (~57%, +6). `cross_file_value_equals` closes the
  baseline's flagship deferred gap (copilot `engines.vscode` ↔ root
  version); `import_gate` (js) ports the http/https-import ban, the
  direct-gulp-import ban, and the uniform `common/` ↛ node/electron
  cross-layer slice. **Key limit:** vscode's `code-import-patterns` is a
  *generated default-deny per-file allowlist* (hundreds of entries +
  layer ordering) — `import_gate` is forbid+allow, so only uniform
  cross-layer bans port. 44 semantic AST rules stay on eslint.
- **nixpkgs** — 14/21 (~67%, +3). `ordered_block` finally gets clean
  targets: maintainer-list.nix + team-list.nix wrap lists in literal
  `# keep-sorted start/end` markers; `command_idempotent` collapses the
  treefmt suite (nixfmt/actionlint/zizmor); `for_each_dir` for the
  `pkgs/by-name` shard file-shape (the largest iteration). No nix
  ecosystem bundle exists (gap). Non-replaceables: nix eval, nixfmt
  semantics, meta.maintainers/license cross-refs (need a nix extractor),
  CODEOWNERS-glob resolution.
- **node** — 22/34 (~65%, +4). `command_idempotent` ×7 collapses the
  per-language lint fan-outs (eslint/cpplint/ruff/remark/shellcheck/
  yaml/clang-format); `ordered_block` ×2 for the README TSC + collaborator
  lists (previously only a bespoke mjs script); `git_commit_no_fixup`
  (node lands squashed). **Baseline correction:** node has NO SPDX/license
  `file_header` convention (only 2 src files carry one) — that mapping
  was wrong and was removed. Non-replaceables: 30 custom AST eslint rules
  + cpplint, core-validate-commit metadata/trailer semantics,
  lint-readme-lists' live-GitHub-team check, license-builder (generator).

### Batch 5 — pnpm, prettier, protobuf, cpython, pytorch

All 5 configs rewritten + validated. One integration fix (pytorch:
`allow_missing_target` must be a rule-level field, not nested inside a
`targets[]` item).

- **protobuf** — 47/58 (~81%, **+19** — the largest single-repo gain in
  the corpus). The famous `src/file_lists.cmake` ↔ Bazel-glob staleness
  → one `command_idempotent` rule (replacing a scheduled CI job); BSD
  header sweep across ~10 language bindings → `file_header` (the repo
  had zero header rules before); per-language version coherence
  (version.json × 9 + protobuf_version.bzl) upgraded from shape-only
  regex to true `cross_file_value_equals`; conformance failure-list
  sortedness → `ordered_block`. Non-replaceables: the wire-format
  conformance suite, protoc/C++ compile.
- **cpython** — 25/31 (~81%, +8). `make regen-all` codegen freshness →
  `command_idempotent` (was a static file_exists stub); ruff/black →
  `command_idempotent`; configure.ac ↔ patchlevel.h version coherence →
  `cross_file_value_equals`; `.gitattributes` generated markers →
  `registry_paths_resolve`. Non-replaceables: Argument Clinic
  self-checksums (self-referential intra-file digest, bespoke algo), the
  NEWS.d-blurb-per-PR rule (a diff "must-ADD" predicate), the 4 C-API
  semantic checks (stable_abi/smelly/check-c-globals).
- **pytorch** — 49/63 (~78%, +6). The `.lintrunner.toml` formatter codes
  (clang-format/ruff/codespell/pyfmt/…) → `command_idempotent` ×9 ("the
  formatter is a no-op", the real CI invariant); torchgen freshness →
  `generated_file_fresh`; `build_variables.bzl` → `registry_paths_resolve`.
  **Useful correction:** `cmake/Codegen.cmake` literally `exec()`s the
  `.bzl` as Python, so there is NO bzl↔CMake duplication to sync — the
  v0.9.17 README's "WORKFLOWSYNC/sync gap" was partly a mis-diagnosis.
  Non-replaceables: clang-tidy/mypy, the custom AST adapters, WORKFLOWSYNC
  (N-to-N job equality), STABLE_SHIM (git-diff-hunk-aware).
- **pnpm** — 24/31 (~77%, +6). **The repo went polyglot since the
  v0.9.17 study** — it now has a full Rust half (`pacquet/` + crates).
  `registry_paths_resolve` resolves the pnpm-workspace.yaml `packages:`
  globs to real dirs; `toml_path_matches` covers the new Rust toolchain;
  `command_idempotent` for meta-updater-no-drift + cargo fmt/taplo/typos.
  Honest finding: meta-updater's invariants are COMPUTED-value syncs
  (`pnpm@11.3.0` vs `11.3.0`), not verbatim equality — so they fall back
  to dual regex pins (a `normalize:`/transform on cross_file_value_equals
  would close this; see protobuf).
- **prettier** — 21/28 (~75%, +6). Per-tool `command_idempotent` (was a
  single `yarn lint` wrapper); `check-deps.js` fully retired (5
  package.json pins → declarative); ~7 of 9 changelog sub-checks
  declarative + PR-scoped via `changed_since`. Non-replaceables: tsc,
  eslint AST, knip/cspell, the changelog_unreleased-per-PR "must-ADD"
  predicate.

### Batch 6 — rust, tensorflow, tokio, next.js, turbo

All 5 configs rewritten + validated. Integration fixes: tokio
`ordered_block` `comparator: byte` → `lexical` (valid: lexical /
lexical-ci / numeric); turbo `registry_paths_resolve` `source:` is a flat
string with `extract:` as a sibling (not nested cross-file-style); turbo
`$schema` JSONPath is `$['$schema']` (no leading dot before the bracket);
tensorflow registry rule was missing `level:`.

- **rust** — 20/34 (~59%, ~27 behaviors, +6). `ordered_block` maps
  `tidy::alphabetical` exactly (the tidy-alphabetical-start/end markers);
  the whole `tidy::style` whitespace/length/forbidden-token sweep → ~10
  declarative rules; the Cargo.lock `source =` allowlist (tidy::extdeps,
  exactly 2 allowed sources) → exact; rustfmt → `command_idempotent`;
  triagebot path-filter → `registry_paths_resolve`. **Correction to the
  prompt's hint:** the per-tier PERMITTED_DEPENDENCIES allowlist needs a
  `cargo metadata` graph walk, NOT `import_gate` (which reads source, not
  the resolved dep graph) — kept as a non-replaceable.
- **tensorflow** — 32/41 (~78%, +9). The bats sanity suite (pylint/
  buildifier/clang-format/codespell/api-compat) → `command_idempotent`
  with per-file offender parsing; the `tensorflow.org/code/<path>` link
  integrity → `registry_paths_resolve`; requirements_lock cross-version
  pin parity → `cross_file_value_equals`. **`compliance/apache-2@v1`
  over-fires** (1,185 generated `.pbtxt` goldens + `_pb2.py` + third_party)
  — extended with a same-id `paths.exclude` override.
- **tokio** — 20/24 (~83%, the corpus high). `cross_file_value_equals`
  ×3 — the headline being MSRV coherence across 5 crates' `rust-version`
  + ci.yml's `rust_min` (the classic hand-synced 6-file drift, now
  machine-enforced from one source); `pair_hash` for the README
  byte-mirror; `ordered_block` for spellcheck.dic. No DCO → signoff N/A.
- **next.js** — 50/61 (~82%, +4). `registry_paths_resolve` for the
  errors/manifest.json registry (a documented v0.9.17 gap);
  `command_idempotent` ×11 collapses all lint tools; dual-half coverage
  (JS json_path/for_each_dir + Rust toml_path/rust-toolchain lockstep).
- **turbo** — 31/38 (~82%, +6). n-way npm-version ↔ version.txt
  coherence → `cross_file_value_equals` (the real cross-half contract);
  `registry_paths_resolve` on `[workspace].members`; `import_gate`
  mirroring clippy.toml type/method bans. **Premise corrections:** turbo
  is MIT (not MPL-2.0) and Rust files carry no SPDX header (assert the
  `#![deny]` attr instead); the Cargo `turbo` crate is pinned 0.1.0, so
  coherence is npm ↔ version.txt, not Cargo ↔ npm.

**All 30 case studies re-analyzed.** Every example config now exercises
the v0.10/v0.11 capability set; coverage rose materially on every repo
(biggest jumps: protobuf +19, k8s +10, tensorflow +9, cpython +8,
airflow +27pp). The aggregate cross-cutting synthesis follows below.

## Cross-cutting findings

_(Running list; finalized in the aggregate phase after all batches.)_

**New-rule-kind candidates surfaced (by # of repos so far):**
- **`pair_inverse`** (3: angular inverse-goldens, ruff `insta
  --unreferenced reject`, spark orphan-module detection) — "every X must
  be referenced by some Y" / orphan detection. Strongest candidate; was
  already a design candidate, still unshipped.
- **JS/ESM-export value extractor** (angular) — `cross_file_value_equals`
  can't read a value exported from a `.mjs` module, blocking
  commit-scope-list sync.
- **cross-language registry consistency** (spark `modules.py` ↔ pom
  `<modules>`) — beyond `cross_file_value_equals`' literal subset.
- **XML-attribute-list path extractor for `registry_paths_resolve`**
  (dotnet `.slnx` `<Project Path=...>` list ↔ on-disk csproj) — single
  XML extract exists, but resolving an XML *attribute list* as a path
  registry is the gap. Pairs with the spark/arrow registry work.
- **`.bazelrc`-style include/path resolution** (bazel) — resolve the
  paths an rc file imports. Narrower than the above.
- **value-set membership / `*_path_contains`** (react prod-error-codes
  "every thrown Error literal ∈ codes.json"; helm; batch-1 echoes) —
  `cross_file_value_equals` does 1:1 equality, not N-in-1 membership.
  Now flagged by 3+ repos; pairs conceptually with `pair_inverse`.
- **`registry_append_only`** (react codes.json) — assert a registry
  only ever grows (documented invariant; no kind expresses it).
- **`git_commit_subject_matches`** (go Gerrit `pkg: lowercase verb`) —
  a subject-line convention rule; the commit-validation family has
  signed-off / no-fixup / author / gpg but no subject-shape rule.
- **`cross_language_implementation_complete`** (flutter per-platform
  engine-surface parity) — 3rd demand signal across the corpus.
- **`git_commit_subject_matches`** (now 4 signals: go Gerrit
  `pkg: lowercase verb`, node `subsystem: desc ≤72col`, nixpkgs
  `pkgs/x: old -> new`, + others) — **the single most-requested new
  commit kind.** The v0.11 commit family has signed-off / no-fixup /
  author / gpg but no subject-shape rule. Strongest new-kind candidate
  overall; cheap to build on the existing commit-validation plumbing.
- **`registry_value_used` / value-set membership** (typescript
  diagnosticMessages, react codes.json, helm) — assert each registry
  key/value is referenced ≥1× across a target file set. Reconfirms the
  N-in-1 membership gap from batches 1-3; now ~5 signals.
- **A `nix@v1` ecosystem bundle** (nixpkgs) — no nix ecosystem bundle
  exists, unlike rust/go/python/node/dotnet.
- **`changeset_requires_path` / "the diff must ADD a file under glob X"**
  (prettier changelog_unreleased, cpython Misc/NEWS.d, pnpm `.changeset/`
  — 3 explicit signals, more latent) — `scope_filter.changed_since`
  already computes the changed set, but no kind asserts that the change
  set MUST include a new path matching a glob. The cleanest, most
  broadly-demanded new kind tied to the v0.11 changed-since machinery.
- **`normalize:`/value-transform on `cross_file_value_equals`** (protobuf
  `4.36-dev` vs `4.36.0`; pnpm `pnpm@11.3.0` vs `11.3.0`; nodeVersion
  `22.13.0` vs `>=22.13`) — 2-3 signals. The existing `normalize:`
  supports trim/lower only; a strip-prefix / semver-floor transform
  would close the "same value, two FORMS" drift that currently forces
  dual regex pins. Pairs with the membership candidate.
- **`embedded_checksum` / `self_checksum`** (cpython Argument Clinic
  `output=<hash>` end-markers) — a self-referential intra-file digest
  with a tool-specific algorithm; `pair_hash` assumes distinct files +
  standard sha. Niche but cleanly scoped.
- **`cross_file_keys_cover` / value-set ⊆ key-set** (pnpm catalogMode
  strict: every catalog reference must resolve to a catalog key) —
  another facet of the membership family alongside `registry_value_used`.
- **`pair_changed_together`** (rust rustdoc_json FORMAT_VERSION ↔ the
  format struct; turbo/next release guards) — two files that must change
  in the same commit. Diff-aware, like `changeset_requires_path`.
- **full-file `lines:{}` equality / `structured_block_equals`** (tokio
  README mirror with diff-on-mismatch; rust rustdoc template sync) —
  `pair_hash` reports only a digest mismatch, not the offending lines.
- **`no_case_collisions`** (tensorflow Windows dup-casing; a recurring
  cross-platform hazard) and **`dir_name_equals_field`** (turbo crate/pkg
  dir ↔ name field) — small, cleanly-scoped structural kinds.

**Engine/ruleset tuning candidates (batch 6 additions):**
- **`compliance/apache-2@v1` over-fires on EVERY large Apache/CNCF repo
  in the corpus — 5 confirmations** (airflow, helm, istio, kubernetes,
  tensorflow). The universal failure mode: branded/abbreviated headers,
  thousands of generated files (`.pbtxt`/`.pb.go`/`.gen.go`/`_pb2.py`),
  third_party/ vendored trees, "The X Authors" attribution, and no
  top-level NOTICE. **This is the single highest-confidence ruleset fix
  in the whole re-analysis.** Recommended: ship generated-file +
  third_party excludes and header-tolerance in the bundle, OR document a
  copy-paste `paths.exclude` + relaxed-`file_header` override recipe (the
  pattern every batch independently re-derived).
- **`import_gate` reads source text, not the resolved dependency graph**
  (rust PERMITTED_DEPENDENCIES, also helm/istio depguard partially) —
  it expresses flat per-file "dir X ↛ import Y" edges well, but a
  Cargo.lock/`cargo metadata` allowlist or a transitive-closure firewall
  (go deps_test.go) is out of scope. A `cargo metadata`/lockfile-aware
  dependency-allowlist kind is a distinct, frequently-wanted feature.

**Engine/ruleset tuning candidates (batch 3 additions):**
- **The ASF compliance bundles over-fire on real Apache projects** —
  now 3 confirmations: `apache/governance@v1` (airflow, batch 1) and
  `compliance/apache-2@v1` (helm + istio, batch 3). Real ASF/CNCF repos
  use abbreviated/branded headers, exclude generated `.pb.go`/`.gen.go`,
  attribute some files to other authors ("The Kubernetes Authors"), and
  often have no top-level NOTICE/KEYS. The canonical bundles assume the
  full RAT header on every file. Action for the aggregate phase: relax
  the bundles (header tolerance + generated-file excludes) or ship a
  documented per-rule `paths:`/exclude override recipe.
- **`import_gate` is a flat per-file regex, not a graph** (go
  deps_test.go) — it catches "directory X must never import Y" edges
  (the bulk of the value) but cannot express transitive DAG closure.
  A genuine, acceptable limit worth documenting.
- **mutating-generator working-tree-diff** (helm gen-test-golden, istio
  `make gen`) reconfirmed — `generated_file_fresh` is stdout-only, so
  mutating generators must use `command_idempotent` (--check mode). This
  is now the dominant pattern; `generated_file_fresh` fits only the rare
  stdout-emitting generator.

**Engine/ruleset tuning candidates:**
- **`import_gate` has no scala/java preset** (spark) — `generic` +
  explicit `import_pattern` works but a preset would be cleaner.
- **`apache/governance@v1` over-fires on real ASF repos** (airflow: no
  top-level KEYS; RELEASE_NOTES.rst vs CHANGELOG) — relax the bundle or
  document per-rule `paths:` overrides.
- **`generated_file_fresh`'s stdout-diff model has limited reach** —
  many real codegen tools (angular Bazel write-actions, arrow in-place
  generators) mutate files rather than emit to stdout, so
  `command_idempotent` (check-mode) is the more broadly applicable form.

**Headline coverage gain (batch 1):** every repo's prior "alint-future"
bucket is now largely expressible; airflow alone went 35% -> 52%. The
biggest single wins are `command_idempotent` (codegen-freshness +
per-file-spawn collapse) and `xml_path_*` (Maven `pom.xml`).

## Aggregate synthesis (all 30 repos)

Re-analysis complete across all six batches. Every example config was
rewritten against its current upstream and the full v0.10/v0.11 capability
set, then validated with `alint validate-config` (the actual
`examples-validate` CI gate). Coverage rose on all 30 repos.

### Coverage at a glance

- **Typical replaceable share landed in the ~65-83% band**, up from the
  ~35-55% the v0.9.17-era configs expressed. Highs: tokio ~83%, next.js /
  turbo / protobuf / cpython ~81-82%. The residual is consistently the
  same shape: AST/type-aware linters (eslint/clippy/govet/mypy/tsc),
  compile/test execution, and semantic graph analysis — alint's
  deliberate non-goals.
- **Newly-unlocked surfaces per repo ranged +3 to +19**, dominated by
  four v0.10 kinds: `command_idempotent` (per-file-spawn collapse +
  codegen-freshness in --check mode — the single most-used unlock),
  `cross_file_value_equals` (version/MSRV/pin coherence — repeatedly the
  baseline's explicitly-deferred "needs a new primitive" gap),
  `import_gate` (layering/dep firewalls), and `registry_paths_resolve`
  (manifest→disk resolution). The v0.11 additions (`changed_since`,
  commit-validation, `{{env.X}}`) showed up everywhere as the way to
  PR-scope an otherwise-intractable full-tree sweep.

### New-rule-kind candidates, ranked by demand (signal count)

1. **`git_commit_subject_matches`** (go, node, nixpkgs + latent) — the
   commit family's missing subject-shape rule. Cheapest to build on
   existing plumbing; clearest single win.
2. **`changeset_requires_path`** — "the diff must ADD a file under glob
   X" (prettier changelog_unreleased, cpython NEWS.d, pnpm `.changeset/`;
   related: turbo/rust release guards, `pair_changed_together`). Ties
   directly to the v0.11 `changed_since` machinery.
3. **value-set membership family** — `registry_value_used` (typescript
   diagnostics, react codes.json), `cross_file_keys_cover` (pnpm
   catalog), `cross_file_set_equals` (rust features↔book, tf goldens).
   Recurring N-in-1 / set-equality need that `cross_file_value_equals`
   (1:1) and `pair_hash` (digest) cannot express. (Note: verify how much
   `registry_paths_resolve`'s existing `orphans`/`must_contain` already
   covers before designing new kinds.)
4. **`normalize:`/value-transform on `cross_file_value_equals`**
   (protobuf `4.36-dev`↔`4.36.0`, pnpm `pnpm@x`↔`x`) — strip-prefix /
   semver-floor so "same value, two forms" stops forcing dual regex pins.
5. **richer `import_gate`** — default-deny/table-driven mode (vscode
   code-import-patterns) and glob-discovered per-dir rule files (k8s 66
   `.import-restrictions`). Plus a SEPARATE dep-graph allowlist kind
   (`cargo metadata`/lockfile-aware) for rust PERMITTED_DEPENDENCIES /
   go transitive closure — out of import_gate's flat-regex scope.
6. Smaller/niche: `embedded_checksum` (cpython clinic), `no_case_collisions`
   (tf), `dir_name_equals_field` (turbo), full-file `lines:{}` equality
   with diff-on-mismatch (tokio README mirror), `cross_language_implementation_complete`
   (flutter/protobuf parity), a `nix@v1` ecosystem bundle.

### Engine/ruleset tuning (highest confidence first)

1. **The ASF compliance bundles over-fire on every large Apache/CNCF repo
   — 5+ confirmations** (`compliance/apache-2@v1`: helm, istio, k8s,
   tensorflow; `apache/governance@v1`: airflow). Universal cause:
   branded/abbreviated headers, generated files, third_party trees, "The
   X Authors" attribution, no top-level NOTICE. **The single
   highest-confidence fix.** Ship generated-file/third_party excludes +
   header tolerance in the bundles, or document the `paths.exclude` +
   relaxed-`file_header` override recipe every batch independently
   re-derived.
2. **`generated_file_fresh` is stdout-only; real codegen mutates files**
   — `command_idempotent` (--check) is the broadly-applicable form. Worth
   making explicit in the docs so users don't reach for the wrong kind.
3. **`import_gate` presets** — no scala/java/dart/nix preset (generic +
   `import_pattern` works but a preset is cleaner).

### Release-readiness assessment

The re-analysis surfaced **no v0.10/v0.11 regressions or defects** — only
additive feature opportunities (the candidates above) and one
documentation/ruleset-polish item (the ASF-bundle over-fire, which has an
immediate per-config workaround already shipped in the affected examples).
None of these block the v0.11 release: they are the v0.12+ backlog. The 30
refreshed example configs all validate and materially raise the
demonstrated coverage of the shipped capability set. **v0.11 is clear to
cut on this analysis.**
