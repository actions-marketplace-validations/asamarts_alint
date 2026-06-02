# v0.12 architecture synthesis — primitives from the 111-repo study

Status: **Design (drafted 2026-06-02), pre-implementation.** Synthesises the
[100-repo study](./case_study_log.md) (457/748 = 61% coverage; 257 file-graph
edge sources; locked demand-ranked backlog) into the *primitive* design for the
v0.12 build phase. Supersedes the "one kind vs three" open question in
[`value_set_membership.md`](./value_set_membership.md) (decision: **one unified
kind**) and folds in [`cross_file_normalize.md`](./cross_file_normalize.md). The
graph half lives in [`file_dependency_graph.md`](./file_dependency_graph.md).

## The reframe: compose, don't accrete

The ~50 study candidates are not ~20 new kinds. alint's cross-file/relational
surface is a **composition of orthogonal axes that already share one extract
layer** (`crate::extract`); most findings are new *combinations*, not new kinds.

| Axis | Today | Study wants added |
|---|---|---|
| **① Source / Extract** (`crate::extract`) | toml · json · yaml · regex · lines (shared) | `whole_file` · `filename` · `git:{subject,author,trailer}` · `file_set` (glob→path set) · multi-capture template |
| **② Normalize** (per-kind enum) | none · trim · lower · semver-major | promote to a **shared, extensible** transform: + `semver` · `basename` · `casefold` |
| **③ Relation** (the comparison) | `equals` · `resolves` · `digest` · `exists` | **set** relations + **identity** (whole-file) |
| **④ Quantifier / Scope** | `for_each_*` · `every_matching_has` · `pair` · `paths` | for-all-with-value-predicate · for-all over repeating in-file blocks · a uniform `select:` / `allow:` selector |
| **⑤ Graph** | — (existing kinds are 1-level only) | acyclic · orphans · reachable · layered · fresh (**257 sources**) |

**Principle:** make ①–④ first-class composable layers and add ⑤. The next 50
findings then come from *combining* primitives, not adding kinds — and today's
1-level kinds (`registry_paths_resolve`, `import_gate`, `markdown_paths_resolve`,
`pair_hash`, `cross_file_value_equals`) become recognisable points in this space.

## How the backlog collapses

- **Already covered** (presets/options, not kinds): `no_filename_case_conflict`→
  `no_case_conflicts`; dangling-symlink→`no_symlinks`; no-exec-bit→`executable_bit`
  (inverse option); consistent-line-endings→`line_endings`; non-ASCII→`file_is_ascii`
  (+ `allow:`).
- **Extensions to existing layers** (~15): new ① sources, ② normalisers, ④
  selectors (the whole C-tuning cluster — sectioned `ordered_block`, `select:`
  line-filter, `file_is_ascii allow:`, fixture excludes).
- **Two genuinely-new foundational primitives:** the unified **`cross_file`**
  relation kind (③ over ①②) and **`file_graph`** (⑤).
- **Small dedicated kinds:** git-extract family (`git_commit_subject_matches`,
  `changeset_requires_path` — extend `git_commit_*`); `generated_file_fresh`
  mutating-mode (a flag); 2-3 niche per-file predicates (`path_length_cap`,
  `max_consecutive_spaces`, count-header).

---

## Primitive A — the unified `cross_file` kind (③ over ①②)

One kind, parameterised by `relation:`, over the shared `extract:` + `normalize:`.
Subsumes `cross_file_value_equals` (1:1), `value_set_membership` (set), and
`files_equal` (identity); the forward half of `registry_paths_resolve`
(path-existence) folds in as `relation: resolves`.

```yaml
- id: <id>
  kind: cross_file
  source:  { file: <path>,        extract: <ExtractSpec> }   # or files: <glob>
  targets: { files: <glob>,       extract: <ExtractSpec> }   # or a [{file,extract}] list
  relation: equals        # equals | identical | subset | superset | set_equals | resolves
  normalize: none         # shared transform applied to every extracted value
```

### The `relation:` enum (closed, ergonomic)

| `relation` | source ⇒ | target ⇒ | asserts | corpus example | replaces |
|---|---|---|---|---|---|
| `equals` | 1 value `v` | 1 value each | every target == `v` (after normalize) | tokio MSRV; airflow version-coherence | `cross_file_value_equals` |
| `identical` | whole content | whole content | byte-identity (opt. skip-header) | tokio README mirror; symfony | *gap* (`files_equal`) |
| `subset` | set `S` | set `T` | `S ⊆ T` (singleton `S` = membership) | pnpm catalog refs ⊆ keys; TS diagnostics used | `value_set_membership` |
| `superset` | set `S` | set `T` | `S ⊇ T` | registry covers all uses | `value_set_membership` |
| `set_equals` | set `S` | set `T` | `S == T` | rust features ↔ unstable-book; TF v1↔v2 goldens | `value_set_membership` |
| `resolves` | set of paths | *filesystem* | each path exists (1-level forward) | registry member paths exist | `registry_paths_resolve` (forward) |

**Cardinality.** `equals`/`identical` extract one value (today's "exactly one"
constraint); the set relations relax that — the extract may yield many values
which form the set. `resolves` targets the file tree, not another extract (the
bridge to ⑤: *graph* properties like orphans/cycles stay in `file_graph`).

### Backward-compat (no breaking changes)

Aliases are free (`registry.register("name", build_fn)`). Keep the legacy names as
**sugar that desugars to `cross_file`**:
- `cross_file_value_equals` → `cross_file` with `relation: equals` (default), so
  every existing config is byte-compatible.
- `value_set_membership` (new) ships *as* `cross_file` with a set relation — never
  a separate kind.
- `files_equal` (new) → `cross_file` `relation: identical`.
- `registry_paths_resolve` stays its own kind name (rich `base`/`must_contain`/
  `orphans` ergonomics) but its core forward check is `relation: resolves`; its
  `orphans` reverse-edge moves to `file_graph` `require: no_orphans` (documented as
  the canonical replacement; the option stays for compat).

This keeps per-relation error messages crisp (the engine knows the relation) while
the *implementation* is one composable core.

---

## Primitive B — `file_graph` (⑤)

The graph layer, already designed in
[`file_dependency_graph.md`](./file_dependency_graph.md): `nodes:` (glob) +
`edges:` (`from_content:` reusing `crate::extract`, or `derive_target:` name
template) + a closed `require:` enum (`acyclic | no_dangling | no_orphans |
forbidden_edges | fresh`). The study **validated it decisively (257 sources, every
edge shape)** — the gate flips from study-gated to GO.

**Boundary with `cross_file` (keep them distinct):** `cross_file` asserts
relations between *extracted values* (1-level); `file_graph` assembles *edges*
into a graph and asserts *global structural* properties (cycles, transitive
reachability, orphans-over-N-hops, layering). `import_gate` stays the cheap
per-file firewall; `file_graph` is the whole-repo layered version.

---

## Shared-layer extensions (enable both primitives)

- **① extract — new sources** (all in `crate::extract`, so every kind gains them):
  `whole_file` (→ `identical`); `filename`; `git: { subject | author | trailer }`
  (→ the git-extract kinds); `file_set` (a glob → the *set* of matching paths, for
  set relations over file lists, e.g. symfony `replace`-map ↔ sub-package dirs);
  a multi-capture template `{MAJOR}.{MINOR}.{PATCH}` (rails split-constant compose).
- **② normalize — promote + extend** (the [`cross_file_normalize.md`](./cross_file_normalize.md)
  plan): make `normalize:` a shared transform on any extract value, adding
  `semver` (full), `basename`, `casefold`. Used by `cross_file`, `ordered_block`,
  `unique_by`, etc.
- **④ selectors — make uniform:** a `select:` line/element filter on scan kinds
  (sectioned `ordered_block`, `every_matching_has`); an `allow:` exception list
  (`file_is_ascii` codepoints) — the recurring C-tuning cluster.

---

## Build order (design-doc-first per convention)

1. **`file_graph`** — #1 demand (257 sources), highest leverage, well-specified.
   Revise its draft against the corpus edge-shapes, prototype on 2-3 cases, build.
2. **`cross_file` unified kind** — fold in `value_set_membership` + `files_equal`;
   generalise `cross_file_value_equals` (add `relation:`, default `equals`);
   the ① `whole_file`/`file_set` sources + the ② normalize promotion land with it.
3. **git-extract kinds** (`git_commit_subject_matches`, `changeset_requires_path`)
   — small, planned, cheap; reuse the new ① `git:` source.
4. **`generated_file_fresh` mutating-mode** + the ④ selector polish + the niche
   per-file predicates; **dedup the already-covered** (presets, not kinds).

Each new kind: design-doc-first, then an atomic rule+wiring commit, on CHANGELOG
`[Unreleased]` toward the v0.12 minor — same cadence as the v0.10 rule-kind cut.
The 110 corpus configs + the study log are the regression/evidence base.

## Open questions / risks

- **Config surface of one kind with 6 relations** — mitigated by the legacy
  aliases (most users keep writing `cross_file_value_equals`) + per-relation schema
  validation + worked examples per relation.
- **Error-message clarity** — the engine must report relation-specific messages
  (`S ⊄ T: extra {…}`), not a generic "values differ"; the relation enum makes this
  tractable.
- **Performance** — set relations + `resolves` need the cross-file full-index
  path, not per-file dispatch (already the dispatch class for these kinds).
- **`resolves` vs `file_graph`** — keep the boundary firm: 1-level existence in
  `cross_file`, anything graph-structural in `file_graph`.
