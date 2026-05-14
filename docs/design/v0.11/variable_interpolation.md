# Variable interpolation across the DSL

Status: Design draft, written 2026-05-14 after v0.9.21 shipped #26.

## Problem

v0.9.21 added POSIX-style `${VAR}` / `${VAR:-default}`
interpolation to exactly one config field:
`git_commit_message.since:`. That was the minimum scope needed
to close issue #26 — it lets users wire `since:
${ALINT_BASE_SHA}` to a GitHub Actions env var without
hand-editing `.alint.yml` per environment.

The narrow surface has two problems:

1. **The same pattern wants to extend everywhere.** Multi-team
   repos want to template `extends:` URLs by team; per-
   environment configs want to template `paths:` globs by root
   directory; everyone wants the option of env-driven values in
   `pattern:` / `policy_url:` / `content:` etc. Today they have
   to template the `.alint.yml` itself externally — clunky and
   doesn't compose with `extends:`.
2. **The syntax is inconsistent with the rest of the DSL.**
   alint already has `{{vars.X}}` for static-var substitution
   in rule messages and `{{ctx.match}}` for per-violation
   substitution. POSIX `${VAR}` is a different shape for what's
   conceptually the same thing — variable substitution in a
   string.

## Proposed surface

### Canonical syntax: `{{env.X}}` + `| default(...)`

```yaml
# Direct env var, hard error at load if unset
since: "{{env.ALINT_BASE_SHA}}"

# Env var with default fallback
since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"

# Composed with other text in the value
extends:
  - "https://policy.{{env.TEAM}}.example.com/v1/rules.yml#sha256-..."
  - "https://shared.{{env.ENV | default('prod')}}.example.com/v1/baseline.yml#sha256-..."

paths: "{{env.PACKAGES_DIR | default('./packages')}}/**/*.rs"
```

Filter syntax (`| default(...)`) follows the Jinja / Liquid /
Nunjucks convention. Small surface for v0.11; future filter
additions (`| upper`, `| lower`, `| trim`) cost nothing.

### Where interpolation applies

Every string-typed VALUE field in the config, at load time:

| Field | Interpolated | Why |
|---|---|---|
| `extends:` URL | ✅ | Per-environment / per-team registry |
| `extends:` SRI hash | ❌ | Hashes are fixed at config-author time |
| `paths:` glob | ✅ | Per-environment path roots |
| `pattern:` regex | ✅ | Environment-driven matcher |
| `since:`, `changed_since:` | ✅ | The motivating case |
| `policy_url:` | ✅ | Per-team policy URLs |
| `message:` | ✅ (adds `{{env.X}}` to existing `{{vars.X}}` / `{{ctx.X}}`) | Per-environment context in violation messages |
| `content:`, `content_from:` | ✅ | Template fix bodies |
| `vars:` value side | ✅ | Lets `vars` themselves be env-driven |
| `id:` | ❌ | Rule IDs must be stable across environments; env-driven IDs break audit trails + run reproducibility |
| `kind:` | ❌ | Type-like, not a value |
| `level:` | ❌ | Enumerated; env-driven severity is a footgun |
| `when:` clause (as a string) | n/a — see below | Expression-language clause, not a string substitution |

### `env.X` in the `when:` expression language

Today `when:` clauses can reference `vars.X` (declared in
`vars:`) and `facts.X` (declared in `facts:`):

```yaml
when: vars.strict_mode and facts.has_rust
```

v0.11 adds `env.X` as a third namespace:

```yaml
when: env.CI == "true" or env.GITHUB_ACTIONS == "true"
```

Same load-time resolution: env vars are read once when the
config loads. The `when:` lexer gains one keyword (`env`) and
the resolver gains one variable-namespace dispatch. Symmetric
with how `vars` / `facts` are surfaced.

### Schema validation order

Schema validation runs **after** interpolation. So
`{{env.SUBJECT_MAX | default('72')}}` in an integer-typed field
parses as `72` and validates clean. Pre-interpolation schema
validation would reject the raw `{{...}}` text as a non-integer.

Note: this means a typo'd env-var name with a wrong-typed default
(e.g. `{{env.MAX | default('seventy')}}` on
`subject_max_length:`) surfaces as a schema-validation error, not
an interpolation error. The error path includes the field name so
the user can trace which interpolation site produced the bad
value.

### Migration of v0.9.21's `${VAR}` syntax

`git_commit_message.since:` keeps accepting `${VAR}` and
`${VAR:-default}` in v0.11 but emits a deprecation warning at
load time:

```
warning: rule "conventional-commit": `since: ${ALINT_BASE_SHA}`
  uses the v0.9.21 `${VAR}` interpolation syntax. The canonical
  v0.11+ syntax is `{{env.ALINT_BASE_SHA}}`; the `${VAR}` form
  will be removed in v1.0. See https://alint.org/docs/configuration/
  #variable-interpolation.
```

One minor-version overlap window is enough for a feature that
shipped four days before its successor. v1.0 removes the legacy
path.

## Implementation

### Phase 1 — interpolation pass

A new module `alint-dsl::interp` runs after YAML parse but
before schema validation:

1. Walk every parsed YAML node.
2. For each string-typed scalar value, find `{{...}}` spans.
3. Parse each span as `<namespace>.<name>` optionally followed
   by `| <filter>(<arg>...)`.
4. For `env.X`: look up `X` via the injected `env_lookup` fn.
   Apply filters. Substitute.
5. For `vars.X` and `ctx.X`: leave un-substituted; later passes
   handle those.

Test seam: `env_lookup: impl Fn(&str) -> Option<String>` mirrors
the v0.9.21 `git_commit_message` pattern. Production callsite
uses `std::env::var(name).ok()`; tests pass a fake map. Keeps
the crate `forbid(unsafe_code)`-compatible (Rust 2024 marks
`set_var` unsafe).

### Phase 2 — `when:` engine update

The existing `vars` / `facts` namespace dispatch in
`alint-core::when::eval` gains an `env` arm. Lexer adds one
keyword. ~30 LOC + 5 unit tests.

### Phase 3 — `git_commit_message` migration

The v0.9.21 `expand_env` function in
`alint-rules::git_commit_message` is removed (the broader
interp pass handles it now). The `since:` field's raw value
arrives at the rule already interpolated. The rule adds a
load-time deprecation-warning check: if `since_raw` contains
`${`, emit the warning.

### Phase 4 — documentation

- New concept doc at `docs/site/concepts/variable-interpolation.md`.
- Cross-references in `docs/rules.md` on every rule whose field
  accepts interpolation.
- `docs/site/integrations/github-actions.md` updated with the
  `{{env.X}}` form alongside the existing recipe.
- CHANGELOG entry distinguishing the canonical form from the
  deprecated form.

Estimated total: ~120 LOC interp engine + ~30 LOC `when:`
extension + ~10 LOC migration + 15 unit + 6 e2e + docs.

## Error messages

Interpolation errors carry the failing site so users can trace
them:

```
error: rule "conventional-commit", field `since:`:
  references undefined env var `ALINT_BASE_SHA` and has no default.
  Set the env var, or use the default-value filter:
    since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
```

```
error: extends entry 1, field `url:`:
  unknown namespace `evn` (typo for `env`?).
  Supported namespaces: env, vars.
```

The error format mirrors the existing `Error::rule_config` shape
used for schema validation errors — same display path, same
expected-by-tooling formatter.

## Security considerations

- **Env vars never reach rule bodies.** WASM plugins (v0.12)
  receive their config dict *post-interpolation* — the host
  resolves `{{env.X}}` references before passing config to the
  guest. Guests can't read env vars indirectly via config
  values they didn't author.
- **No shell evaluation.** `{{env.X}}` reads via `std::env::var`,
  no `system()` or backtick eval. Filter expressions parse to
  a tiny AST (env-ref + zero-or-more filters); no recursion,
  no eval.
- **The `extends:` URL interpolation case** is where attacker-
  controlled env vars could redirect a config-load to a
  malicious host. The user's local config is trusted source —
  if an attacker can edit it, they don't need env-var
  interpolation to redirect URLs. But: document this gotcha in
  the security model.

## Open questions

- **Should `vars:` values be allowed to reference `{{env.X}}`?**
  Yes (per the table above). This lets users centralise env-var
  references in one `vars:` block rather than repeating
  `{{env.X | default(...)}}` at every callsite.
- **Order between `vars:` and `env:`?** Interpolation pass
  resolves `{{env.X}}` first, then the existing `vars:`
  substitution pass resolves `{{vars.X}}`. So a `vars:` value
  can be `{{env.X | default('fallback')}}` and downstream uses
  of `{{vars.X}}` see the resolved value.
- **Should we expose `{{ctx.match}}` etc. at this layer?**
  No — those are evaluate-time per-violation substitutions
  done in the renderer, not load-time. Keep the layers distinct.
- **Caching for repeated env-var reads in `vars:` cascade?**
  Probably not needed at this scale — even a config with 50
  interpolation sites costs ~50 `std::env::var` calls (each
  ~100ns). Sub-microsecond load-time impact.
