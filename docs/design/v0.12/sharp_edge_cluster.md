# v0.12 #7 "sharp-edge" cluster — triage + outcome

Status: **Done — 2026-06-03.** Triage of the #7 residual bucket from the
[post-build coverage re-analysis](./post_build_coverage_analysis.md) (recorded as
"≈22 repos" of small sharp-edges). Investigating each — as with `file_set` — shrank
it sharply: **2 were real and fixed, 2 were phantoms (study mis-diagnoses,
disproven end-to-end), 2 are already expressible, 1 is a genuine but larger
walk-architecture change (deferred).** The corpus configs predate the build and
were authored by study agents, so a few "limitations" were never real.

## Fixed

- **`pair_hash` `sums-line` path-first order** (golang FIPS `fips140.sum`). The
  parser was hex-first only (`<hex>  <path>`); the Go FIPS snapshot manifest writes
  `<path> <hex>`. Now the digest token is identified by **shape** (the algorithm
  fixes its hex length), so either order parses; ambiguous lines assume hex-first.
  No new option. (`crate::pair_hash`, CHANGELOG `[Unreleased]`.)
- **`unique_by` `case_insensitive:`** (tensorflow `full.bats` Windows-dup check;
  git). Folds the rendered `key` to lowercase before grouping, so `README.md` /
  `readme.md` collide — the case-insensitive-filesystem hazard. Default `false`.
  (`crate::unique_by`, CHANGELOG `[Unreleased]`.)

## Phantoms — disproven, do NOT re-flag

Both were claimed "silently no-ops" / "matches zero files" in corpus configs;
both are contradicted by the Scope glob engine **and** by an end-to-end
`alint check` run:

- **mastodon root-file scoping** (`paths: ['LICENSE']` for `file_content_matches`
  / structured-query kinds "silently no-ops at root"). **False.** A bare root
  filename, `**/LICENSE`, and `*publiccode.yml` all match the root file; an
  end-to-end `file_content_matches` with `paths: ['LICENSE']` fires on a root
  `LICENSE` as expected. `literal_separator(true)` globs match root files fine.
- **kafka mid-path glob** (`**/resources/**/*.json` "matches ZERO files"; the
  faithful `**/src/{main,test}/resources/**/*.json` allegedly broken). **False.**
  Both globs — including the `{main,test}` brace expansion — match nested resource
  JSON, confirmed by the Scope engine and end-to-end. The kafka config's
  exclude-based workaround was unnecessary.

## Already expressible (no work needed)

- **`file_header` preamble-skip** (deno `// Copyright …` after a shebang /
  lint-ignore preamble). `file_header` matches its `pattern` (unanchored) within
  the first `lines:` lines, so `pattern: '(?m)^// Copyright …'` with `lines: 4`
  accepts the copyright on **any** of the first 4 lines — i.e. tolerates a
  preamble. The over-fire the config reported came from a `^`-anchored (whole-blob
  start) pattern; `(?m)^` (per-line) is the fix, no code change.
- **`file_header` per-tree alternation** (elasticsearch triple-AGPL vs Apache-2.0).
  `pattern:` is a regex, so `(AGPL-3\.0|Apache License)` accepts either header;
  per-tree scoping is one `file_header` rule per `paths:` tree. Fully composable
  today.

## Deferred — genuine, but a walk-architecture change

- **tracked-but-gitignored files** (bazel `.bazelversion`, committed yet listed in
  Bazel's own `.gitignore`). alint's walk respects `.gitignore`, so a
  tracked-but-ignored file is not walked and rules on it no-op. `Context` already
  carries the `git ls-files` set (`git_tracked`, used by `git_tracked_only`), so a
  fix is feasible — union git-tracked paths into the walk, or an
  `include_gitignored:`/`force:` opt-in — but it changes walk semantics broadly
  for a narrow (1–2 repo) need. Tracked separately; not part of this cluster.
