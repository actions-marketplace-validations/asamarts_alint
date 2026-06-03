# The unified `cross_file` relation kind

Status: **Increment 1 (value relations) shipped — 2026-06-03** (`equals` default
+ `subset` / `superset` / `set_equals`; `cross_file_value_equals` → byte-compatible
alias; rule count 84 → 85, CHANGELOG `[Unreleased]`). Realises **primitive A**
of the [architecture synthesis](./architecture_synthesis.md): one kind,
parameterised by `relation:`, over the shared `extract:` (`crate::extract`) +
`normalize:`. Supersedes [`value_set_membership.md`](./value_set_membership.md)
(decision locked: **one unified kind**, not three) and is the home for the
[`cross_file_normalize.md`](./cross_file_normalize.md) promotion. The graph half
of the cross-file surface is the separate, now-complete
[`file_graph`](./file_dependency_graph.md).

## The reframe: one relation knob, not N kinds

The study's value-relation findings are not 3-4 new kinds — they are the same
"extract a value (or set) from a source, extract from each target, assert a
relation" shape with the **relation** varying. `cross_file_value_equals` (v0.10,
released) already is the 1:1 case of this. The unified kind generalises it:

```yaml
- id: <id>
  kind: cross_file
  source:  { file: <path>, extract: <ExtractSpec> }
  targets: { files: <glob>, extract: <ExtractSpec> }   # or a [{file, extract}] list
  relation: equals        # equals (default) | subset | superset | set_equals | identical | resolves
  normalize: none         # shared transform applied to every extracted value
  allow_missing_target: false
```

## The `relation:` enum (closed, ergonomic)

| `relation` | source ⇒ | each target ⇒ | asserts (per target) | corpus example | replaces |
|---|---|---|---|---|---|
| `equals` | 1 value `v` | each value | every target value `== v` (after normalize) | tokio MSRV; airflow version-coherence | `cross_file_value_equals` |
| `subset` | set `S` | set `T` | `S ⊆ T` (singleton `S` = membership) | pnpm catalog refs ⊆ keys | *gap* |
| `superset` | set `S` | set `T` | `S ⊇ T` (registry covers all uses) | TS diagnostics ⊇ codes-used | *gap* |
| `set_equals` | set `S` | set `T` | `S == T` | rust features ↔ unstable-book; TF v1↔v2 goldens | *gap* |
| `identical` | whole content | whole content | byte-identity (opt. skip-header) | tokio README mirror; symfony | *gap* (`files_equal`) — **deferred** |
| `resolves` | set of paths | *filesystem* | each path exists (1-level forward) | registry member paths exist | `registry_paths_resolve` (forward) — **deferred** |

**Cardinality** is the key per-relation difference: `equals` requires the source
to extract *exactly one* literal value (today's constraint); the set relations
relax that — the extract may yield many values, which form the set `S`. Each
target is compared independently (the per-target model `cross_file_value_equals`
already uses), so "N targets" fans out naturally.

**Relation-specific messages** (the engine knows the relation, so it reports
precisely): `equals` → `{tv} != {sv}`; `subset` → `missing required value(s): {S∖T}`;
`superset` → `value(s) not in source: {T∖S}`; `set_equals` → `missing {S∖T}, extra {T∖S}`.

## Backward-compat: `cross_file_value_equals` becomes an alias

`cross_file` is the engine; the released `cross_file_value_equals` name is kept
as a registered **alias** to the same builder, with `relation` defaulting to
`equals`. Every existing config is therefore byte-compatible — the v0.10 kind's
e2e scenarios (`cross_file_value_equals_pass` / `_fail`) and unit tests are the
regression guard, run unchanged through the new engine. No breaking change; the
kind count moves +1 (the new `cross_file` behaviour), not -1+1.

## Build order (increment-per-relation-class, design-doc-first)

1. **Value relations — SHIPPED 2026-06-03:** `equals` (default; the migrated
   `cross_file_value_equals` logic, byte-identical) + `subset` / `superset` /
   `set_equals`. Reuses `crate::extract` and the existing `Normalize`
   (`none|trim|lower|semver-major`). `source` is a single `{file, extract}`;
   `targets` is a `{files, extract}` glob or a `[{file, extract}]` list — the
   `cross_file_value_equals` shape, unchanged. Delivers the whole
   `value_set_membership` demand and the unification in one cohesive landing.
2. **`identical`** (whole-file `files_equal`) — a different source/target shape
   (whole content, no `extract`; optional skip-header). The tokio-README-mirror
   / symfony case.
3. **`resolves`** (1-level path existence) — the forward half of
   `registry_paths_resolve`; `registry_paths_resolve` keeps its name + rich
   `base`/`must_contain`/`orphans` ergonomics, documented as composing with this.
4. **`normalize:` promotion** ([`cross_file_normalize.md`](./cross_file_normalize.md))
   — `semver_floor` / `strip_prefix|suffix` / `casefold`, scalar-or-list, pushed
   into the shared post-extract transform so every cross-file kind benefits.
5. **Convenience source extensions** — `whole_file` / `file_set` (a glob → the
   set of matching paths) `extract` sources; a multi-file source glob.

Each increment: design-doc-first, atomic rule+wiring commit on CHANGELOG
`[Unreleased]` toward the v0.12 minor. The 110 corpus configs + the study log are
the evidence base.

## Constraints / risks

- **No breaking change to the released kind** — `relation: equals` must be
  byte-identical to `cross_file_value_equals`; the existing tests/scenarios gate
  it. The migration *moves* the logic into `cross_file.rs`; it does not rewrite
  the equals path.
- **Config surface of one kind with N relations** — mitigated by the `equals`
  default (most users, and every `cross_file_value_equals` config, never write
  `relation:`), per-relation schema validation, and a worked example per relation.
- **Performance** — set relations read the source once and each target once; the
  per-target set is built from that target's extract. `requires_full_index`
  cross-file dispatch (already the class for `cross_file_value_equals`), no
  per-file hot path. A 1M-file macro bench scenario lands with the v0.12 release
  sweep (shared with `file_graph`).
- **`resolves` vs `file_graph`** — keep the boundary firm: 1-level existence in
  `cross_file`; anything graph-structural (cycles, orphans, reachability) stays
  in `file_graph`.
