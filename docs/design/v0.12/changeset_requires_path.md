# Diff "must-add" family: `changeset_requires_path` + `pair_changed_together`

Status: **BOTH SHIPPED — 2026-06-03** (`changeset_requires_path` rule count
86 → 87; `pair_changed_together` 87 → 88; CHANGELOG `[Unreleased]`). Build on
v0.11's `scope_filter.changed_since` machinery (`changeset_requires_path` adds
the `collect_changed_paths_filtered` `--diff-filter=A` helper;
`pair_changed_together` reuses `collect_changed_paths_checked` for the full
changed set). Decisions locked: `since:` is **required** on both (a diff rule
without a base ref is a config error, not a silent pass); `changeset_requires_path`
is added-only (status `A`, no `change_type:` knob); `pair_changed_together` is
**directional** (the trigger is `if_changed`, the obligation `then_changed`; a
`then_changed`-only change never fires it — swap the globs in a second rule for
a bidirectional pact); no-base / no-repo / empty-diff / gate-not-met all no-op
gracefully; a bad `since:` hard-fails with the family's shallow-clone hint. The
firing paths need a real two-commit repo (the testkit's `git: { commits }` makes
empty commits), so both are covered by native tests in `shell_out_rules.rs` +
the `NATIVE_FIRES_ALLOWLIST` (a silent no-repo e2e satisfies `coverage_audit`).
**The diff "must-add" family is now complete.**

## Motivation / demand

`scope_filter.changed_since` already computes the set of files a PR
touched, but no rule asserts a *requirement* on that set. Several
corpus repos require a contribution to *add* or *co-modify* specific
files:

- **changeset / changelog-per-PR** — prettier `changelog_unreleased/`,
  cpython `Misc/NEWS.d/next/`, pnpm `.changeset/*.md`. Three explicit
  signals; more latent (many projects gate "did you add a changelog
  entry?" in CI).
- **pair-changed-together** — rust `rustdoc_json` FORMAT_VERSION must
  bump when the format struct changes; turbo/rust release guards
  ("version.txt and the lockfile change together").

## Sketch

Two related kinds, both diff-scoped (only meaningful with a base ref):

```yaml
# "the diff must ADD at least one file matching glob X"
- id: changelog-entry-required
  kind: changeset_requires_path
  add_glob: ".changeset/*.md"
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  # when_changed: "src/**"   # optional: only require it if src/ changed

# "if file A changed, file B must change in the same range"
- id: format-version-bumped
  kind: pair_changed_together
  if_changed: "src/rustdoc-json-types/lib.rs"
  then_changed: "src/rustdoc-json-types/FORMAT_VERSION"
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
```

- `changeset_requires_path`: the `<since>..HEAD` diff must include an
  *added* (status `A`) path matching `add_glob:`. Optional
  `when_changed:` gates the requirement on some other glob having
  changed (avoid demanding a changelog entry for a docs-only PR).
- `pair_changed_together`: if any `if_changed:` path is in the diff,
  at least one `then_changed:` path must be too.
- Both fire only in diff mode (a base ref present); no-op on a
  full-tree run, like `changed_since`.

## Open questions

- Do these belong as rule kinds or as a generalised `scope_filter`
  requirement clause? (Lean: kinds — the assertion is about the *set*,
  not per-file filtering.)
- Added (`A`) vs. modified (`M`) semantics for `changeset_requires_path`
  — almost always "added"; expose a `change_type:` knob or hard-code A?
- How to surface "no base ref ⇒ rule inert" without it looking like a
  silent pass (ties to the v0.11 informational-notes channel).
