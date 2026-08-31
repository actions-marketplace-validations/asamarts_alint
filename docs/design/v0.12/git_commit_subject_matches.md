# `git_commit_subject_matches`

Status: **Shipped — 2026-06-03** (`crate::git_commit_subject_matches`, rule count
85 → 86, CHANGELOG `[Unreleased]`). The 5th commit-validation family member,
reusing `commit_range`. Built minimal: `matches:` (subject-line regex) + the
family `since:`/`include_merges:`; the proposed `max_length:` sugar was DROPPED to
avoid duplicating `git_commit_message`'s `subject_max_length:` (compose the two
rules, or encode length in the regex). Top-ranked gap from the 30-repo pass; the
cheapest, clearest single win.

## Motivation / demand

The v0.11 commit-validation family (`git_commit_signed_off`,
`_no_fixup`, `_author_allowlist`, `_gpg_signed`) has no subject-line
shape rule, yet three corpus repos enforce a strict commit-subject
grammar in CI:

- **go** (Gerrit): `pkg/path: lowercase summary` (component prefix +
  lowercase verb).
- **node**: `subsystem[,subsystem]: description`, ≤72 cols, lowercase.
- **nixpkgs**: `pkgs/path: oldver -> newver` and conventional-commit
  types via `lint-commits.js`.

## Sketch

A fifth member of the family, reusing `alint-rules::commit_range`
(head-or-range fetch, per-commit violations with abbreviated SHAs,
`since:`, `include_merges:`):

```yaml
- id: subject-grammar
  kind: git_commit_subject_matches
  matches: '^[a-z0-9_/.-]+: [a-z].{0,70}$'   # regex on the subject line
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  # optional: max_length: 72 (sugar over the regex)
```

- `matches:` — regex against the first line of each commit message.
- Inherits `since:` / `include_merges:` / `{{env.X}}` interpolation
  from the family for free.
- Optional `max_length:` ergonomic shortcut (the ≤72 convention is
  near-universal; cheaper than making every user encode it in the
  regex).

## Open questions

- Subject = first line only, or first line + a blank-second-line check?
  (node enforces the blank line; could be a separate `body_format`
  option or out of scope.)
- Do we need a `forbid:` inverse, or is a negative-lookahead regex
  enough? (Lean: regex is enough; keep the kind minimal.)
- Trailer/metadata-line semantics (node `PR-URL:` / `Reviewed-By:`)
  stay a non-goal — that is `core-validate-commit`'s job.
