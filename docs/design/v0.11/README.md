# v0.11 — Design pass

Status: Working drafts, written 2026-05-14 after v0.9.21 shipped the
`git_commit_message.since:` fix (issue #26). Each file in this directory
is a per-feature design that should be reviewed and revised before
implementation starts.

## What v0.11 ships

The original v0.11 framing was "LSP + developer experience" — inline
diagnostics, hover-on-rule, code actions. The post-v0.9.21 framing is
broader: "LSP + DSL polish." The motivation is that #26's narrow fix
(one rule got `since:`, one field got `${VAR}` interpolation)
surfaced two natural generalisations:

1. **Scope-narrowing wants to extend to other rule kinds.** The
   per-rule "fire only on commits / files in range" pattern is
   useful everywhere it can apply. Today only `git_commit_message`
   has it.
2. **Env-var interpolation wants to extend to every string-typed
   config field.** Today only one field (`since:`) supports it, and
   even there it uses POSIX `${VAR}` rather than alint's existing
   `{{...}}` template convention.

LSP becomes the demonstration vehicle for the new interpolation
system: hover-on-rule renders the resolved value of every
`{{env.X}}` site so adopters see at a glance what their rules
actually do in their current environment.

| File | Sub-theme |
|---|---|
| [`lsp_server.md`](./lsp_server.md) | LSP server design. Originally landed in v0.9.7 under `docs/design/v0.10/`; relocated here in v0.9.22 when v0.10's scope flipped to case-study coverage. |
| [`vscode_extension.md`](./vscode_extension.md) | VS Code extension design. Same origin as `lsp_server.md`. |
| [`single_file_reevaluation.md`](./single_file_reevaluation.md) | Engine contract for LSP per-edit re-evaluation; reuses the v0.9.3 per-file dispatch path so a keystroke doesn't trigger a full repo walk. |
| [`scope_filter_changed_since.md`](./scope_filter_changed_since.md) | New `scope_filter.changed_since:` predicate. Per-rule diff-scope; composable with `has_ancestor`. Also covers the `git_no_denied_paths` `since:` option. |
| [`commit_validation_rules.md`](./commit_validation_rules.md) | Family of four new commit-validation rule kinds (`git_commit_signed_off`, `git_commit_no_fixup`, `git_commit_author_allowlist`, `git_commit_gpg_signed`). All ship with `since:` from day one. |
| [`variable_interpolation.md`](./variable_interpolation.md) | `{{env.X}}` interpolation across every string-typed config field. `\| default(...)` filter for fallbacks. `env.X` namespace in the `when:` expression language. Deprecation path for the v0.9.21 `${VAR}` syntax. |

## Cross-cutting decisions

- **Interpolation syntax is `{{env.X}}`**, matching alint's existing
  `{{vars.X}}` template convention. POSIX `${VAR}` was a v0.9.21
  expedient; it's deprecated in v0.11 and removed in v1.0.
- **Type-like and identifier-like fields stay un-interpolated.**
  `id:`, `kind:`, `level:` are not substituted, by design — env-
  driven rule IDs break audit trails and run reproducibility.
- **Interpolation happens once at config load**, not at evaluate
  time. Env vars don't change during a run.
- **The commit-validation rules share v0.9.21's shape.** Each rule
  takes `since:` + `include_merges:`, emits one violation per
  failing commit with the abbreviated SHA, and silently no-ops
  outside a git repo. Shared infrastructure means rule #2-#4 are
  cheap to add once rule #1 lands.
