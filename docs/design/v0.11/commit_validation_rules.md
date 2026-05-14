# Commit-validation rule family

Status: Design draft, written 2026-05-14 after v0.9.21 shipped #26.

## Problem

v0.9.21's `git_commit_message` `since:` mode made the rule usable
in PR CI for the first time. The reporter (issue #26) hit the
synthetic-merge-at-HEAD gotcha that breaks any single-point commit-
validation rule on `pull_request`-trigger workflows.

The same gotcha applies to every other commit-validation policy
projects commonly enforce — DCO sign-off, no leftover fixup
commits, author-pattern allowlists, GPG signature verification.
None of these have a rule kind in alint today, and if we ship them
as HEAD-only rules they'll have to be retrofitted with `since:`
again. Better to design them as a family from day one with range
support built in.

## Proposed surface — four new rule kinds

Each rule mirrors v0.9.21's `git_commit_message` shape: takes
`since:` + `include_merges:`, emits one violation per failing
commit with the abbreviated SHA + subject snippet, silently
no-ops outside a git repo, hard-fails on unresolvable refs with
the fetch-depth: 0 hint.

### 1. `git_commit_signed_off`

DCO-style `Signed-off-by:` trailer in commit footer.

```yaml
- id: dco
  kind: git_commit_signed_off
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  pattern: '^Signed-off-by: .+ <.+@.+>$'   # default if omitted
  level: error
```

Demand surface: every CNCF / Linux Foundation / Apache /
kernel-style project. Real-repo demand sources from the case-study
corpus: kubernetes, istio, helm, opentelemetry, prometheus,
containerd, runc, etcd.

### 2. `git_commit_no_fixup`

Fail on residual `fixup!` / `squash!` / `amend!` commits.

```yaml
- id: no-fixup-commits-in-pr
  kind: git_commit_no_fixup
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

No configuration knobs needed for the default case; matches
exactly the prefixes `git rebase --autosquash` understands.

Demand: anyone who PRs with `git commit --fixup` and forgets to
rebase. The forgetting is the universal case.

### 3. `git_commit_author_allowlist`

Author email or name matches a regex pattern.

```yaml
- id: commits-from-org-only
  kind: git_commit_author_allowlist
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  email_pattern: '^.+@example\.com$'
  level: error
```

Either `email_pattern:` or `name_pattern:` is required;
specifying both is AND. Demand: enterprise repos enforcing
contributor identity against a corporate domain; OSS projects
catching commits from sock-puppet or compromised accounts.

### 4. `git_commit_gpg_signed`

`git verify-commit` succeeds on every commit in range.

```yaml
- id: signed-commits
  kind: git_commit_gpg_signed
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

The rule shells out to `git verify-commit <sha>` per commit; a
non-zero exit fires. Demand: kernel maintainers, security-
sensitive OSS, anyone using GitHub's "Require signed commits"
branch protection. Notably the rule **doesn't** validate which
keys are trusted — that's git's GPG config or
`.git/allowed_signers`. The rule just checks "did verify-commit
say yes."

## Shared infrastructure

All four rule kinds share:

- `since:` + `include_merges:` options (semantics from v0.9.21).
- The `commit_messages_in_range` helper from `alint-core::git`
  (already shipped; `git_commit_gpg_signed` will need a sibling
  `commit_shas_in_range` that returns just SHAs since it doesn't
  parse messages).
- The `expand_env` helper for env-var interpolation (subsumed by
  v0.11's broader variable-interpolation work — see
  [`variable_interpolation.md`](./variable_interpolation.md)).
- Per-commit violation formatting: abbreviated SHA + truncated
  subject snippet + the rule's specific failure-message body.
- The shallow-clone hint on bad-ref errors.

First rule (probably `git_commit_signed_off`, the highest-demand
kind) carries the cost of any infrastructure extension; rules
#2-#4 are each estimated at ~80 LOC + 5 unit tests + 3 e2e
scenarios.

## Schema additions

Each rule kind gets its own `rule_*` definition in
`schemas/v1/config.json`, mirroring the v0.9.21
`rule_git_commit_message` shape. The four `since:` /
`include_merges:` properties extract into a shared
`$defs/commit_range_options` that v0.9.21's rule kind also
references (small back-compat refactor).

## Bundled-ruleset opportunity (follow-up)

With range support across the family, the `git/conventional-
commits@v1` bundled ruleset becomes viable. v0.9.21 hinted at it
in the issue-26 PR description. Could ship as a 5-rule bundle:
`conventional-prefix`, `subject-length`, `signed-off-by`,
`no-fixup`, `no-merge-noise`. Out of scope for v0.11 but a
natural v0.11.x or v0.12 follow-up.

## Open questions

- **Default for `git_commit_signed_off.pattern:`** — should we
  ship a default that matches the DCO sign-off shape, or require
  the user to provide one? Default keeps trivial configs short;
  requiring breaks the "I just want DCO" use case. Lean toward
  shipping a default that matches the kernel DCO regex.
- **`git_commit_no_fixup` — should it also flag commits whose
  body is empty + subject is a duplicate of an earlier commit
  in the range?** That's the "git commit --fixup with --no-edit"
  shape. Could be a separate option `include_implicit_fixups:
  true`. Skip for v0.11.
- **`git_commit_gpg_signed` — should we surface the GPG-trust
  chain in violation output?** Probably not — that's git's job.
  Just report "unsigned" vs "signed with untrusted key."
