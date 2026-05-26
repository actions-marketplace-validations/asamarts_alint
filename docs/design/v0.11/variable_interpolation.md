# Variable interpolation across the DSL

Status: **Implemented 2026-05-22.** Shipped as the v0.11 first PR
(keystone: `scope_filter_changed_since.md` and
`commit_validation_rules.md` both depend on `{{env.X}}` in their
examples). Phases 1–4 below landed across `alint-dsl::interp`
(load-time `{{env.X}}` walk), the `when:` `env` namespace
(`alint-core`), and the `git_commit_message` `${VAR}` deprecation
(`alint-rules`), with the concept doc at
`docs/site/concepts/variable-interpolation.md`. Open questions are
resolved below under "Resolved decisions"; the `extends:`-URL
security stance (§Security considerations) is ratified pending
objection. Original draft written 2026-05-14 after v0.9.21 shipped
#26.

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
| `extends:` entry (URL + `#sha256-…`) | ✅ | Per-environment / per-team registry. **Implementation note:** an `extends:` entry is a single string at the YAML-value layer (`ExtendsEntry::Url(String)`), so interpolation covers the hash too — the draft's "SRI hash is never interpolated" carve-out was not built. In practice a hash never contains `{{}}`; the trust argument (see §Security) does not rely on the hash being un-interpolated. |
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

The `when:` parser gains one namespace (`env` → `Namespace::Env`)
and the evaluator gains one dispatch arm. Symmetric with how
`vars` / `facts` are surfaced. No lexer change is needed —
namespace words are generic identifiers the parser dispatches on,
not lexer keywords.

**Resolution timing (refined during implementation):** unlike the
`{{env.X}}` *string* interpolation above (load-time, once), the
`when: env.X` namespace resolves at **evaluation time** — there is
no "load time" for a `when:` clause, which is evaluated when the
engine gates a rule during the run. Env is constant during a run,
so an eval-time read is functionally equivalent to a load-time
snapshot. An unset variable evaluates to `null` (falsy), matching
the "missing fact is falsy" rule. `WhenEnv` carries an optional
injected env map (`with_env`) so tests resolve `env.X`
hermetically; in production the field is `None` and the evaluator
reads the live process environment via `std::env::var` (a safe
read — only `set_var` is `unsafe` in Rust 2024).

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

**Refined during implementation.** The original plan said "remove
`expand_env`; the broader interp pass handles it now" — but that
only holds for the *new* `{{env.X}}` form. The legacy POSIX
`${VAR}` syntax must stay scoped to `git_commit_message.since:`:
generalising it through the interp pass would mean expanding
`${...}` in *every* string field, which would break a literal
`${` in a `pattern:` regex or a `message:`. `${VAR}` was only ever
interpolated in `since:`.

So the actual change:

- **`expand_env` stays** in `alint-rules::git_commit_message`, still
  expanding `${VAR}` / `${VAR:-default}` at evaluate time — legacy
  configs keep working this one minor.
- **`build()` emits a deprecation warning** (the existing
  `eprintln!("alint: warning: …")` channel) when `since:` contains
  `${`. The warning is *actionable*: a `posix_to_env_template`
  helper rewrites the value into the canonical form
  (`${BASE:-origin/main}` → `{{env.BASE | default('origin/main')}}`)
  and shows it inline.
- A `since:` written in the **canonical `{{env.X}}` form is resolved
  upstream** by the `alint-dsl` interp pass at load, so it arrives
  at the rule already substituted (no `${`, no warning).

v1.0 removes `expand_env` and the `${VAR}` path entirely.

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
  unknown namespace `en` (typo for `env`?).
  Supported namespaces: env, vars.
```

The error format mirrors the existing `Error::rule_config` shape
used for schema validation errors — same display path, same
expected-by-tooling formatter.

### `{{...}}` is shared with foreign template languages

**Refined during implementation** (the `.2` engine surfaced this
via `examples/nixos-nixpkgs`, which has
`command: ["actionlint", "-format", "{{json .}}"]` — a Go template,
not alint interpolation). alint does **not** own the `{{...}}`
namespace: Go templates (`{{json .}}`, `{{end}}`, `{{.Foo}}`),
cookiecutter (`{{cookiecutter.slug}}`), Jinja, etc. all appear
legitimately in `command:` args, `pattern:` regexes, and
`message:` strings.

So the engine only **claims spans it is confident are its own**:

- `{{env.X}}` → resolved (undefined-without-default is an error;
  this namespace is unambiguously alint's).
- `{{vars.X}}` / `{{ctx.X}}` → deferred to later passes.
- An unknown namespace within **Levenshtein distance 1** of
  `env`/`vars` (e.g. `en`, `vasr`) → treated as an alint typo and
  errored, with the hint above.
- **Everything else** (`{{json .}}`, `{{end}}`, `{{cookiecutter.x}}`,
  dotless `{{...}}`) → **passed through verbatim**, untouched.

Trade-off accepted: a genuine alint typo whose namespace is *far*
from `env`/`vars` (e.g. `{{secrets.X}}`) silently passes through as
a literal rather than erroring. That is the price of being a polite
citizen of the shared `{{...}}` space — breaking real configs that
embed foreign templates is the worse failure. The distance-1 typo
net still catches the common single-character slips.

## Security considerations

- **Env vars never reach rule bodies.** WASM plugins (v0.13)
  receive their config dict *post-interpolation* — the host
  resolves `{{env.X}}` references before passing config to the
  guest. Guests can't read env vars indirectly via config
  values they didn't author.
- **No shell evaluation.** `{{env.X}}` reads via `std::env::var`,
  no `system()` or backtick eval. Filter expressions parse to
  a tiny AST (env-ref + zero-or-more filters); no recursion,
  no eval.
- **The `extends:` URL interpolation case** is where an env var
  referenced in an `extends:` entry could change where a config
  load fetches from. **RATIFIED (pending objection):** the user's
  local `.alint.yml` is trusted source — influencing the URL
  requires both writing `{{env.X}}` into the config AND controlling
  the environment, and the former is already a full compromise (an
  attacker who can edit the config can write the malicious URL
  directly). So env interpolation adds no new privilege; ship it.
  **Correction vs the original draft:** an `extends:` entry is a
  single string at the value layer (`ExtendsEntry::Url(String)`),
  so interpolation covers the `#sha256-…` hash too — the "SRI hash
  stays un-interpolated" carve-out was *not* built and is not the
  backstop. The actual backstop is the trust boundary above; for
  configs whose `extends:` URLs are not env-driven, the
  author-fixed hash pins content exactly as before. This is the one
  security-relevant decision in the PR; revisit it if the threat
  model ever assumes an untrusted local config.

## Resolved decisions

All four original open questions are resolved as below; the
draft's leanings are ratified.

- **`vars:` values may reference `{{env.X}}`.** ✅ Lets users
  centralise env-var references in one `vars:` block rather than
  repeating `{{env.X | default(...)}}` at every callsite.
- **Resolution order: `env` first, then `vars`.** The
  interpolation pass resolves `{{env.X}}` first; the existing
  `vars:` substitution pass then resolves `{{vars.X}}`. So a
  `vars:` value can itself be `{{env.X | default('fallback')}}`
  and downstream `{{vars.X}}` uses see the resolved value. (No
  cycle risk: `env` cannot reference `vars`.)
- **`{{ctx.match}}` etc. stay at the evaluate-time layer.** Not
  exposed in this load-time pass — they're per-violation
  substitutions done in the renderer. Keep the layers distinct.
- **No caching of env-var reads.** Even a config with 50
  interpolation sites costs ~50 `std::env::var` calls (~100ns
  each) — sub-microsecond load-time impact. Not worth the
  complexity.

## Implementation order (the v0.11 first PR)

Single PR, phased commits, per the project's design-doc-first +
phased-rollout convention:

1. **`.1 design`** — this doc, Status flipped to finalized
   (done in this commit).
2. **`.2 interp engine`** — new `alint-dsl::interp` module
   (Phase 1 above). ~120 LOC + 15 unit.
3. **`.3 when: env namespace`** — `env` arm in
   `alint-core::when::eval` + one lexer keyword (Phase 2).
   ~30 LOC + 5 unit.
4. **`.4 ${VAR} migration + deprecation`** — remove
   `git_commit_message::expand_env`; `since:` arrives
   pre-interpolated; add the load-time `${`-detection
   deprecation warning (Phase 3). ~10 LOC + e2e.
5. **`.5 schema + docs`** — confirm validation runs
   post-interpolation; `docs/site/concepts/variable-interpolation.md`;
   CHANGELOG `[Unreleased]` entry (canonical vs deprecated form)
   (Phase 4). docs + 6 e2e.

The existing `[Unreleased]` already carries the workspace-dep-pin
chore line; this PR's CHANGELOG entry joins it under the same
section.
