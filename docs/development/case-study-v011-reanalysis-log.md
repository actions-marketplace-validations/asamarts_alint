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
_(pending)_

### Batch 4 — kubernetes, typescript, vscode, nixpkgs, node
_(pending)_

### Batch 5 — pnpm, prettier, protobuf, cpython, pytorch
_(pending)_

### Batch 6 — rust, tensorflow, tokio, next.js, turbo
_(pending)_

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
