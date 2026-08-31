# Deferred from v0.11

Status: **Planned (v0.12).** Drafted 2026-05-25. These are v0.11-plan
items that were *not* picked up before the v0.11 cut and have no home
in the case-study gap backlog — so they collect here rather than slip
untracked. None gated v0.11; all are additive.

(The fifth v0.11 long-tail item, `cross_language_implementation_complete`,
and the Bazel-licensing-declaration-aware rule kind are case-study-derived
*rule kinds* and live in [`niche_rule_kinds.md`](./niche_rule_kinds.md).)

## 1. `has_sibling` / `has_descendant` scope predicates

- **Origin:** the v0.11 "scope generalisation" plan. `changed_since`
  shipped; these two did not (confirmed absent from `crates/*/src`).
- **What:** additional `ScopeFilter` predicates alongside the shipped
  `has_ancestor` / `changed_since`. `has_sibling` fires a rule on a
  file only if a sibling matching a glob exists; `has_descendant` the
  directory analog.
- **Why cheap:** v0.9.10's `Scope::from_spec` makes predicate additions
  purely additive — no API churn, same evaluation path as the existing
  predicates.
- **Demand:** general DSL completeness rather than a specific corpus
  signal; revisit priority after the 100-repo study, which may surface
  concrete sources.

## 2. `walk_error_policy:` engine knob

- **Origin:** v0.11 opportunistic long-tail (1 source: pnpm's
  `tests/fixtures/has-broken-symlinks/`).
- **What:** a top-level engine setting controlling how the walker
  reacts to unreadable entries — `strict` (error), `skip-broken-symlinks`,
  `permissive` (warn + continue).
- **Why deferred:** single-source, and it touches the walker's error
  path (more blast radius than a rule kind). The 100-repo study —
  spanning far more filesystem shapes (vendored trees, symlink farms,
  generated dirs) — is the right place to confirm the mode set before
  committing to engine-level behavior.

## 3. LSP "Add rule to ignore" code action

- **Origin:** explicitly deferred in
  [`../v0.11/lsp_server.md`](../v0.11/lsp_server.md) — the other LSP
  code actions (Apply fix) shipped; this one edits `.alint.yml`, which
  is a separate design decision.
- **What:** a quick-fix on a diagnostic that appends the rule id (or a
  path-scoped exception) to the config's `ignore:` / a per-rule
  `paths.exclude`, returning a `WorkspaceEdit` against `.alint.yml`.
- **Open questions:** which knob it writes (global `ignore:` vs.
  per-rule exclude vs. an inline suppression comment — alint has no
  inline-suppression syntax today, which may itself be a prerequisite);
  and undo/round-trip behavior when the config is hand-formatted. Pairs
  naturally with the v0.11 fast-follow list in `lsp_server.md`
  (debounce/cancellation, unsaved-file diagnostics, `Hint`-surfacing of
  informational notes) — those are LSP polish that can ride the same
  cut if it grows an LSP workstream.

## Note

Items 1-2 are small enough to land opportunistically alongside the
v0.12 gap kinds; item 3 only matters if v0.12 reopens LSP work. If the
100-repo study produces no new demand for any of these, they can slip
again without harm — they are tracked here, not committed.
