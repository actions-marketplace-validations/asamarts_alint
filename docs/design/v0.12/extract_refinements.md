# v0.12 #4 "extract refinements" cluster — triage + outcome

Status: **Done — 2026-06-03.** Triage of the #4 residual bucket from the
[post-build coverage re-analysis](./post_build_coverage_analysis.md) (≈11 repos).
Reproducing each claim first — the discipline that dissolved `file_set` and most
of [#7](./sharp_edge_cluster.md) — left **1 real fix (JSONC tolerance, built) and
2 already-expressible** (occurrence-via-regex idiom; version-compose via component
rules). The pattern holds: investigate before building.

## Fixed

- **JSONC tolerance for `json:` structured queries.** `tsconfig.json`,
  `.vscode/*.json`, and friends carry `//` + `/* … */` comments and trailing
  commas (a `.json` extension but JSONC content), which the strict
  `serde_json` parser rejected — so `json_path_*`, the `json:` extract
  (`cross_file` / `registry_paths_resolve`), and `json_schema_passes` all failed
  on them (astro, TypeScript, nix). `Format::parse` (the single shared parse
  point) now **tries strict JSON first** (plain JSON is byte-identical and pays
  nothing) and, only on failure, **retries on a JSONC-stripped copy** (comments +
  trailing commas removed by a string-aware pass, so markers inside `"…"` strings
  are preserved). A genuinely-broken document still fails and reports the
  *original strict* error. (`crate::structured_path`, CHANGELOG `[Unreleased]`.)

## Already expressible (no work needed)

- **`occurrence: first/nth` — the "latest changelog version" need** (black, httpx,
  elixir, pandoc). The claim: extracting from a multi-version `CHANGELOG.md`
  returns *all* versions, so `cross_file` errors `"source must resolve to exactly
  one value (the query matched several)"`. **Reproduced as workable:** a
  start-anchored lazy regex yields exactly *one* capture — the first match:

  ```yaml
  source:
    file: CHANGELOG.md
    # (?s) dot-matches-newline · \A anchors at start · .*? lazily reaches the
    # FIRST `## X.Y.Z` heading → a single value (the latest release).
    extract: { regex: '(?s)\A.*?## (\d+\.\d+\.\d+)' }
  ```

  Confirmed end-to-end: the source resolves to the single latest version (skipping
  a leading `## Unreleased`, which the version regex doesn't match), passes when it
  agrees and fails with a clean single-value mismatch when it doesn't. A dedicated
  `occurrence:` option would be marginally cleaner DSL, but the idiom is real and
  fully covers the corpus need — documented in `docs/rules.md`.

- **Multi-capture version-compose** `{MAJOR}.{MINOR}.{PATCH}` from separate
  constants (rails, protobuf). rails already ships a working expression: one
  `cross_file` rule per component plus a `version_core` var — its own config notes
  the full-string compose would be "nice to have," not required. A convenience
  gap, deferred until a case appears that the component form can't express.

## Net

The only genuinely-unworkable item in #4 was JSONC; the other two compose from
existing primitives. Consistent with `file_set` and #7: each "gap cluster" has
roughly halved on investigation. **Reproduce a claimed limitation before
building.**
