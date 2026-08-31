# The unified `cross_file` relation kind

Status: **COMPLETE — 2026-06-03** (value relations `equals`
default + `subset`/`superset`/`set_equals`, `cross_file_value_equals` →
byte-compatible alias, count 84 → 85; then `identical` whole-file + `resolves`
path-existence; then the `normalize:` promotion — `semver-minor` + composable
lists; then the `whole_file` extract source — increment 5a). The optional
`file_set` source (increment 5b) was **dropped** after a dedicated scan of the
111-repo corpus: every parity shape the corpus actually expresses is already
covered (see §5b), and not one repo asked for `file_set`'s only unique
capability (bidirectional glob-vs-glob path-set parity). CHANGELOG
`[Unreleased]`. Realises **primitive A**
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
2. **`identical` — SHIPPED 2026-06-03** (whole-file `files_equal`) — byte
   identity (no `extract`; optional `skip_header_lines`). The tokio-README-mirror
   / symfony case. `source.extract` must be absent; `targets` carry no `extract`
   (validated in `build`'s `validate_shape`).
3. **`resolves` — SHIPPED 2026-06-03** (1-level path existence) — each source
   path resolves relative to the source file's dir and must exist (file or dir);
   no `targets`. The forward half of `registry_paths_resolve`, which keeps its
   name + rich `base`/`must_contain`/`orphans` ergonomics (use it for those).
4. **`normalize:` promotion — SHIPPED 2026-06-03**
   ([`cross_file_normalize.md`](./cross_file_normalize.md)) — `semver-minor`
   (the `MAJOR.MINOR` band, which alone reconciled both corpus signals) +
   the composable scalar-or-list form. `strip_prefix`/`strip_suffix`/`casefold`
   deferred (not corpus-proven). Currently `cross_file`-local; pushing it into
   the shared `crate::extract` post-processing for every cross-file kind is a
   later refactor.
5. **Convenience source extensions** —
   - **5a `whole_file` (SHIPPED 2026-06-03)** — an `extract: { whole_file: {} }`
     source/target yielding the entire file content as one value. Makes
     `relation: equals`/`subset`/etc. operate on whole-file content without an
     `identical` byte-compare (e.g. a `LICENSE` that must equal `LICENSE-MIT`
     even though both carry `${YEAR}` interpolation markers). The non-literal
     skip (for interpolated *paths*) is bypassed for `whole_file` — content is
     compared verbatim. No new kind/count change; an extract source on the
     existing cross-file kinds.
   - **5b `file_set` (DROPPED 2026-06-03 — corpus-unjustified)** — would have
     added a glob → path-set source for bidirectional glob-vs-glob path-set
     parity. A dedicated scan of all 111 corpus repos showed every parity shape
     repos *actually* express is already covered, two in production configs:
     - "every X has sibling Y by stem" → `every_matching_has`/`for_each_file` +
       `{stem}` (eslint `lib/rules/*.js` → `docs/src/rules/{stem}.md` +
       `tests/lib/rules/{stem}.js`, `eslint-eslint.alint.yml:45`);
     - "Y exists AND Y.value == f(X)" → `for_each_dir` + nested `json_path_equals`
       `equals: "{path}"` (docusaurus `packages/*` repository.directory,
       `facebook-docusaurus.alint.yml:84`);
     - sibling pairing (.c↔.h) → the released `pair` kind + `{stem}` (vapor
       `Sources/CVaporBcrypt/*.c` → `{stem}.h`, `vapor-vapor.alint.yml:89`);
     - manifest→disk + orphans → `registry_paths_resolve` (TypeScript `libs.json`);
     - value set-equality → `cross_file set_equals` (istio dependabot lists).

     `file_set`'s *only* unique capability — bidirectional path-set parity in one
     rule with set-diff reporting — was requested by **zero** of the 111 repos (a
     targeted search for the reverse/orphan direction: `stray`, `orphan test`,
     `bidirectional`, `set_equals.*path` found nothing beyond vapor's `pair`
     case). Building it would add surface ahead of *and past* demand, against the
     study-gated v0.12 method. The real new-kind candidates the scan surfaced are
     different kinds (`registry_orphans_from_text_list` B1,
     `cross_file_symbol_set_equals` B2 — both source extensions, not path-set
     parity; see `case_study_log.md`). **`cross_file` is therefore complete** with
     `whole_file` as its final increment.

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
