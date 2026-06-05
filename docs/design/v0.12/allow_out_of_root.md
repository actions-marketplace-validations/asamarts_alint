# `allow_out_of_root` — a trust-gated opt-in to read files outside the repo root

**Status: SHIPPED (v0.12, pre-release).** Adds a deliberate escape hatch to the
hard path-confinement shipped in `path-confinement.md`. Default behavior is
unchanged (confined); this only opens a door the user explicitly, and only from
their own top-level config. **v1 scope = the three non-spawn-gated inline read
kinds** (`registry_paths_resolve` `source:`, `json_schema_passes` `schema_path:`,
`pair_hash` `target:`); see Scope for what's deferred and why.

## Motivation

Path confinement (`path-confinement.md`) makes every config-derived path read
fail if it escapes the repo root — unconditionally, no opt-out. That's the right
secure default, but there are legitimate cases for a *trusted* config to point a
read at a file outside the linted tree (a shared schema, a manifest in a sibling
checkout). The constraint is that this must never be reachable from an untrusted
`extends:`'d ruleset — exactly the trust model the spawn gate
(`SPAWNING_RULE_KINDS` / `reject_command_rules_in`) already enforces.

## Config — top-level `.alint.yml` only

```yaml
# default (absent) → hard confinement (current behavior)

allow_out_of_root: true                       # blanket: all rules may read out-of-root

allow_out_of_root:                            # scoped
  kinds: [json_schema_passes, pair_hash]      # any rule of these kinds
  rules: [external-shared-schema]             # specific rule ids
```

A rule is **permitted** iff `allow_out_of_root == true`, OR its `kind` ∈ `kinds`,
OR its `id` ∈ `rules`. Absent / `false` / empty → permitted nothing (confined).

The parsed form:

```rust
enum AllowOutOfRoot {
    Confined,                                       // default
    All,                                            // `true`
    Selective { kinds: HashSet<String>, rules: HashSet<String> },
}
impl AllowOutOfRoot { fn allows(&self, id: &str, kind: &str) -> bool { … } }
```

## Trust model — rejected from `extends:`

`allow_out_of_root` is honored **only** from the user's own top-level config. If
any `extends:`'d ruleset (local, remote, or bundled) sets it, the load **fails**
with a clear error — modeled exactly on `reject_command_rules_in` /
`reject_custom_facts_in`. Enforced in the `extends:` resolution loop
(`loader.rs`), before merge: a parent `RawConfig` with a non-default
`allow_out_of_root` is refused. The top-level value is the only one that survives
the merge. (An adopted ruleset granting itself out-of-tree reads is the precise
threat confinement exists to stop, so the door can only be opened locally.)

## Scope — read sites only

`allow_out_of_root` liberates the **read** sites (read a file outside root),
where the semantics are crisp. **Resolve / index** sites stay unconditionally
confined: an out-of-root path is by definition not an in-tree `FileIndex` entry,
so "does this declared path exist in the tree" has no out-of-root meaning.

**v1 honors the three non-spawn-gated kinds whose read is a single inline site:**

| Honors `allow_out_of_root` (v1) | Confined / deferred |
|---|---|
| `registry_paths_resolve` `source:` · `json_schema_passes` `schema_path:` · `pair_hash` `target:` | **Resolve/index** (always confined): `registry` entries · `cross_file` `resolves` · `file_graph` edges. **Spawn-gated** (always confined — the hatch is moot, since declaring `generated_file_fresh` already requires top-level trust = RCE): `generated_file_fresh` `file:`. **Deferred to a fast-follow** (read sites buried in helpers — `read_rel` / `check_identical` / `check_fresh` — and niche for out-of-root; same policy mechanism, only the helper threading + per-helper note remain): `cross_file` `identical`/value reads · `file_graph` `fresh` reads. |

So a registry may read an **external** `source:` manifest, but the paths that
manifest declares must still resolve in-tree. A rule kind listed in
`allow_out_of_root` that isn't wired (e.g. `cross_file` today) is a silent no-op
— the setter is a default no-op for kinds that don't honor the flag.

## Behavior when permitted

At a read confinement site, when the path escapes **and** the rule is permitted,
the rule reads the path joined to root (absolute → itself; `../../x` → resolved
up) — the pre-confinement behavior — and emits an informational **note**
(`reading out-of-root path "…" — permitted by allow_out_of_root`). The escape is
never silent. When not permitted, behavior is unchanged: an "escapes the repo
root" violation, no read.

## Plumbing — `AllowOutOfRoot` on `Config` + a `Rule` setter (no `Context` change)

1. `alint-core` gains `AllowOutOfRoot` (`Confined | All | Selective { kinds, rules }`,
   custom `Deserialize` from `true` | `{ kinds, rules }`, default `Confined`) with
   `allows(id, kind) -> bool` + `is_confined()`. `RawConfig` (alint-dsl) parses it
   from the YAML; `Config` (alint-core) carries the resolved value as a
   `#[serde(skip)]` field, so a directly deserialized / bundled `Config` can never
   set it (only the loader's `finalize()` does).
2. The `extends:` loop calls `reject_allow_out_of_root_in(&parent.allow_out_of_root, …)`
   on every inherited `RawConfig` (the trust gate); `merge()` carries the surviving
   top-level value; `finalize()` copies it onto `Config`.
3. `Rule` gains `fn set_allow_out_of_root(&mut self, _allow: bool) {}` (default
   no-op). The two eval-build sites — `main.rs` run-path (shared by `check`/`fix`)
   and the LSP — call
   `rule.set_allow_out_of_root(config.allow_out_of_root.allows(&spec.id, &spec.kind))`
   right after `registry.build(spec)`. A site that forgets the call leaves the rule
   confined (the safe default); only the wired kinds override the setter.
4. The wired rules consult the flag at their read site via a shared helper:

   ```rust
   // crate::pathsafe
   pub(crate) enum Confined { In(PathBuf), AllowedEscape(PathBuf), Denied }
   pub(crate) fn confine(path: &Path, allow_escape: bool) -> Confined;
   pub(crate) fn out_of_root_note(path: &Path) -> String;
   ```

   `In(p)` → `root.join(p)` + read; `AllowedEscape(p)` → read + push the note;
   `Denied` → "escapes the repo root" violation, no read.

Chosen over a `Context` field (43 constructor sites) and a computed `RuleSpec`
field (26 `RuleSpec` literals across ~14 files): the setter touches only the two
eval-build sites + the wired rules, and fails safe.

## Security invariants preserved

- Default is confined (opt-in only).
- An untrusted `extends:` can never enable it (hard load error).
- Resolve / index sites stay confined regardless of the policy.
- Permitted reads are scoped to named rules/kinds and surfaced as notes.

## Out of scope

The walker symlink escape (Phase 2) stays an **unconditional** fix — symlinks
are repo *content*, not config-declared paths, so the top-level-config opt-in
doesn't map onto them. (A future "allow this repo's symlinks out of tree" knob
would be a separate, content-trust decision.)

## Future extension (PLANNED, not built in this increment): resolve / index-site escape

The read-only scope above is intentional. This section is the full plan for a
follow-up that widens `allow_out_of_root` to the **resolve / index** sites, so
the work is documented rather than merely deferred. The config surface, trust
model, and `RuleSpec` plumbing are **unchanged** — only the set of sites that
consult `spec.allow_out_of_root` grows, plus a new filesystem-stat fallback.

### Which sites become eligible — and which deliberately do NOT

The widening applies **only to existence-style checks**, where "this path points
at something that exists" has a clean out-of-root meaning. The graph-structural
`file_graph` modes are explicitly **excluded** because an out-of-root target is
not a scanned node and has no coherent role in them.

| Mode / site | Widen? | Out-of-root semantics |
|---|---|---|
| `registry_paths_resolve` entries (`expect: file/dir/any`) | ✅ eligible | stat `root.join(raw)`; exists → pass+note, missing → the normal violation |
| `cross_file` `relation: resolves` | ✅ eligible | same — stat the extracted out-of-root path |
| `file_graph` `require: no_dangling` (incl. `derive_target`) | ✅ eligible | stat the resolved/derived out-of-root edge target |
| `file_graph` `require: acyclic` | ❌ excluded | out-of-root targets aren't scanned nodes → external leaves, excluded from the cycle graph (what confinement already does) |
| `file_graph` `require: forbidden_edges` | ❌ excluded | the `to:` glob is tree-relative; matching an out-of-root path is ill-defined → external, not subject to the firewall |
| `file_graph` `require: no_orphans` | ❌ excluded | reverse-edge over the node subgraph; out-of-root targets aren't nodes |

### Mechanics

- New resolve helper alongside `pathsafe::confine`:
  `resolve_existence(root, path, allow_escape) -> { InIndex(p) | OutOfRootFsHit(p) | Missing | DeniedEscape }`.
  Permitted + escaping → `std::fs::metadata(root.join(raw))` (a **stat**, never a
  read); otherwise index-only, exactly as today.
- **Cost:** one `metadata` per *permitted + escaping* resolved path. The
  index-only fast path is unchanged for the overwhelmingly common in-tree case,
  so there is no regression for existing configs. It is still filesystem access
  outside the tree, so it stays behind the same top-level-only permission.
- **Observability:** a permitted resolve-escape that exists emits a note
  (`resolved out-of-root path "…" exists — permitted by allow_out_of_root`); a
  missing one emits the rule's normal dangling/unresolved violation (the path is
  genuinely absent — not a permission problem).

### Open questions to settle in the follow-up

- **Symlink following on the stat:** `metadata` (follow, "does the target
  exist") vs `symlink_metadata` (the link itself). Lean `metadata`, consistent
  with "does this resolve to a real file/dir" — but note it composes with the
  Phase 2 walker symlink work and should be decided alongside it.
- **`registry` `must_contain` on an out-of-root dir:** honoring `must_contain`
  would require *reading* the out-of-root directory listing (more fs access than
  a stat). Decide whether `must_contain` is supported for out-of-root entries or
  is a documented limitation.
- **`derive_target` constant `to:`:** a many-sources→one-target derived path that
  escapes — same stat treatment, but confirm the note isn't emitted once per
  source (dedupe).

### Why deferred

No demonstrated use case for out-of-root *existence assertions* (unlike reads,
which have the concrete external-schema/manifest case); it adds filesystem
access to currently index-only fast paths; and three of the `file_graph` modes
have no clean out-of-root semantics. Build when a real use case appears — the
config/trust/plumbing surface is already in place, so the follow-up is purely
the eligible-site stat fallback + tests.

## Test plan

- **Policy** unit tests: `true`/`kinds`/`rules`/absent resolve correctly;
  `allows(id, kind)` matrix.
- **Trust gate**: an `extends:`'d config with `allow_out_of_root` → load error
  (local + bundled).
- **Per-rule**: for each of the 6 read sites — permitted + escaping path → reads
  the out-of-root file + emits the note (point at a real out-of-tree sentinel);
  not-permitted + escaping → still "escapes the repo root" (the existing Phase 1
  regression tests stay green).
- **Schema**: `allow_out_of_root` (bool | `{kinds, rules}`) in both schema copies.
- **Snapshot**: regen `default_options.txt` (the 6 rule structs gain a bool).
