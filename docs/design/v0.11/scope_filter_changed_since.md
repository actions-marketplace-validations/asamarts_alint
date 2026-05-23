# `scope_filter.changed_since:` — per-rule diff-scope predicate

Status: **Implemented (2026-05-22)** — the `scope_filter.changed_since:`
predicate. The diff set is resolved once per `Engine::run`/`fix` and
cached on the `FileIndex` (`OnceLock<HashMap<ref, HashSet<PathBuf>>>`),
so `Scope::matches(path, index)` keeps its signature (44 hot-path call
sites unchanged). `changed_since` AND-composes with `has_ancestor`;
either may be set alone (`has_ancestor` is now optional in the spec).
`alint-core::git::collect_changed_paths_checked` distinguishes no-git
(silent) from bad-ref (hard error with the shallow-clone hint).

**Deferred to a follow-up:** the adjacent `git_no_denied_paths.since:`
option (the design's §"adjacent rule-level option") — it is a separate
rule-level change, not part of the `scope_filter` predicate, and is
tracked for a later v0.11.x item.

Original draft written 2026-05-14 after v0.9.21 shipped #26.

## Problem

`alint check --changed --base <ref>` already restricts the file walk
to the `<ref>..HEAD` diff. But it's run-wide: every per-file rule
flips into diff-mode together. Real configs want to mix scopes:

- "Every Rust source file must start with an SPDX header" — only
  fire on **new** files in a PR (grandfather pre-existing files).
- "Every filename must be `snake_case`" — fire on the **whole tree**
  always (don't grandfather violations).
- "Every workflow must pin actions to commit SHAs" — fire on
  workflows **changed in the PR** (let the rule's `paths:` glob
  already narrow to `.github/workflows/*.yml`).

There's no way to express that today. The CLI flag is all-or-nothing.

## Proposed surface

A new `changed_since:` predicate on the existing `scope_filter:`
axis. v0.9.10 introduced `scope_filter: { has_ancestor: <path> }`
for closest-ancestor manifest scoping; v0.11 was already going to
add `has_sibling` and `has_descendant`. Adding `changed_since:` is
a natural extension along the same axis.

```yaml
- id: spdx-header-on-new-files
  kind: file_header
  paths: "src/**/*.rs"
  pattern: "^// SPDX-License-Identifier: MIT"
  scope_filter:
    changed_since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

The rule walks the whole tree (so the per-file matcher and `paths:`
glob work as designed), then `scope_filter.changed_since` filters
to files in the `<since>..HEAD` diff before evaluating. The diff is
computed once per `Engine::run` and cached on the rule `Context`,
same shape as v0.9.5's path-index — no per-rule shell-out to `git
diff`.

`changed_since` AND-composes with the other predicates: a rule with
both `has_ancestor: Cargo.toml` AND `changed_since: origin/main`
fires only on files that are BOTH in a Cargo crate AND in the PR
diff.

## Rule-kind applicability

`scope_filter.changed_since:` applies to per-file rules that fire
once per matching file. Roughly 40 of 60 rule kinds:

| Family | Per-file rules covered |
|---|---|
| Content | `file_content_matches`, `file_content_forbidden`, `file_header`, `file_starts_with`, `file_ends_with`, `file_hash`, `file_max/min_size`, `file_max/min_lines`, `file_footer`, `file_shebang`, `file_is_text`, `file_is_ascii` |
| Structured query | `json_path_*`, `yaml_path_*`, `toml_path_*`, `json_schema_passes`, `markdown_paths_resolve` |
| Naming | `filename_case`, `filename_regex` |
| Text hygiene | `no_trailing_whitespace`, `final_newline`, `line_endings`, `line_max_width`, `indent_style`, `max_consecutive_blank_lines` |
| Security / Unicode | `no_merge_conflict_markers`, `no_bidi_controls`, `no_zero_width_chars` |
| Encoding | `no_bom` |
| Structure (per-file subset) | `no_empty_files` |
| Portable metadata | `no_illegal_windows_names` |
| Unix metadata | `no_symlinks`, `executable_bit`, `executable_has_shebang`, `shebang_has_executable` |
| Git hygiene (per-file subset) | `commented_out_code` |
| Plugin tier | `command` |

## Rule kinds where `changed_since` does NOT apply

By design — the rule's semantics need the full tree.

- **Cross-file relational rules** (`pair`, `for_each_dir`,
  `unique_by`, `every_matching_has`, `dir_contains`,
  `dir_only_contains`, `for_each_file`): they assert
  relationships. Restricting them to the diff would silently
  break the semantics ("`pair` over only changed files" falsely
  passes when one half of the pair was unchanged).
- **Existence / absence rules** (`file_exists`, `file_absent`,
  `dir_exists`, `dir_absent`): they need to see the whole tree
  to assert presence or absence.
- **Aggregate structure rules** (`max_directory_depth`,
  `max_files_per_directory`, `no_case_conflicts`): one-shot
  tree-property checks, not per-file iterations.

The schema enforces this at config-load time: `scope_filter:` with
`changed_since:` set is rejected on these rule kinds with a clear
error.

## `git_no_denied_paths.since:` — adjacent rule-level option

`git_no_denied_paths` is path-listing rather than file-walking
(it consults git's index directly). It already has the equivalent
of "tree-wide vs diff-only" semantics. Adding a `since:` option
to it — mirroring v0.9.21's `git_commit_message.since:` — gives it
the same range-scoping power:

```yaml
- id: no-secrets-in-pr
  kind: git_no_denied_paths
  deny: ["**/*.pem", "**/*.key", "**/.env"]
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

The rule fires on paths **added in `<since>..HEAD`**, not on every
matching tracked path. Catches PR secrets even when HEAD's tree
doesn't show the secret at the tip (e.g., a force-push that
shuffled commits but kept the bad one in history).

## Failure modes

Same shape as v0.9.21's `git_commit_message.since:`:

- **No git, or `git` not on PATH**: silent no-op.
- **`changed_since:` ref doesn't resolve**: hard error with the
  fetch-depth: 0 hint. Reuses the v0.9.21 error message.
- **Range is empty** (force-push where base == HEAD): silent
  no-op; rule walks zero files.

## Implementation

- New `ChangedSince` predicate variant on `ScopeFilter`.
- Engine resolves the diff once per run (reuse
  `collect_changed_paths(root, Some(since))` from
  `alint-core::git`).
- The resolved diff set lands on rule `Context` alongside the
  existing path-index.
- Per-rule `evaluate()` calls iterate matched files through the
  scope-filter predicates as today; `changed_since` just adds
  one membership test against the cached diff set.

Estimated: ~80 LOC + 10 e2e scenarios.

## Open questions

- **Should `--changed --base <ref>` at the CLI override per-rule
  `changed_since:`?** Probably yes (CLI wins), with a stderr note
  when the override happens, so PR-CI's run-wide `--changed`
  doesn't silently miss rules that wanted a different base.
- **Should we expose the resolved diff set to `when:` clauses
  too?** e.g., `when: facts.changed_files contains 'Cargo.toml'`.
  Possibly v0.12 — keeps v0.11's scope tight.
