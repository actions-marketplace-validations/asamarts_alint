# Generic file-dependency-graph family

Status: **Planned (v0.12), study-gated.** Decision recorded 2026-05-30:
pursue the language-agnostic, user-defined *file*-reference graph as the
on-mission generalisation of the (now decoupled) package-graph
`dependency_graph_allowlist`; **gate the edge-DSL commitment on the
100-repo study + a 2-3 case prototype** before building. See
[`dependency_graph_allowlist.md`](./dependency_graph_allowlist.md) for
the narrow package-graph item this splits from.

## The reframe

The roadmap's `dependency_graph_allowlist` (rust `tidy::deps`, go
`deps_test.go`) operates on the **resolved package graph** — nodes are
packages, edges come from a toolchain resolving `Cargo.lock` / the Go
module DAG. That sits on alint's **explicit non-goal** list
(`cargo deny`, `bazel mod`) and forces either a `cargo metadata`
shell-out or a bespoke lockfile parser.

This item operates instead on the **filesystem reference graph**: nodes
are *files*; edges are *file → file references visible in the repo
itself* — a path inside a JSON/manifest file, `foo.pb.go` generated from
`foo.proto`, an `include`/`import` line resolved to a path. That is
squarely "the filesystem shape and contents of a repository" (alint's
stated scope). No toolchain, no resolver, **no spawn-gate**. The generic
version moves the feature from the wrong side of alint's positioning
line to the right side — it is *more* on-mission than the narrow one,
not merely broader.

**It does not subsume the package-graph sources.** rust/go want
package → package allowlists (semantic nodes); a file → file graph
cannot express them. The two items are decoupled: this one is on-mission
and study-gated; the package-graph allowlist stays parked.

## Demand

- **Confirmed corpus sources for the *file-graph* framing: 0 today.**
  The motivating examples (codegen acyclicity, "a path referenced in a
  JSON file is a dependency", generated-file freshness) are plausible
  but unvalidated; the 2 sources on record (rust/go) are *package*-graph.
  This is exactly why the item is study-gated — the 100-repo study must
  surface real file-graph edge shapes before the edge DSL is committed.
- Adjacent shapes already in the corpus that hint at latent demand:
  protobuf codegen trees, generated-binding parity (the
  `cross_language_implementation_complete` long-tail), config-include
  chains.

## Why it's a real gap (existing kinds are 1-level only)

alint already extracts file → file edges, but has no layer that
assembles them into a graph and asserts a *global* property:

| Kind | In graph terms |
|---|---|
| `registry_paths_resolve` (+ `orphans`) | 1-level bipartite graph (manifest → files); `orphans` is reverse-edge detection |
| `import_gate` | per-edge allow/deny; no transitivity, no cycle awareness |
| `markdown_paths_resolve` | content-reference edges, existence-checked only |
| `pair_hash` / `generated_file_fresh` | pairwise freshness, no graph |

Cycles, transitive reachability, orphan-over-N-hops, and global edge
constraints are inexpressible today. **Acyclicity** in particular is
both the clearest demo and a true capability gap.

## Prior art

- **`dependency-cruiser`** (JS) is the closest analog and the model to
  borrow: `forbidden` / `allowed` / `required` rule sections over
  node/edge predicates (`orphan`, `circular`, `reachable`, `from`/`to`
  path matchers). It is bound to a JS module resolver — the *rule model*
  is portable; the *edge source* is not.
- **Architecture-as-code** — `import-linter` (Python "layers contract":
  lower layers must not import higher), `ArchUnit` (Java; cycle + slice
  detection), `deptrac` (PHP): all "build an import graph, check edges
  against contracts," all language-specific / AST-based.
- The few **language-agnostic** tools (CodeGraph, sandstorm `sda`,
  Emerge) are AST / Tree-sitter visualisers, not config-defined
  filesystem-reference linters.
- **Conclusion:** a config-defined, filesystem-reference, no-parser
  graph linter is an open niche — consistent with how alint already
  beats AST tools by staying at the filesystem layer.

## Model (bounded — not a query language)

One kind, a bounded `edges:` block + a fixed `require:` enum (mirrors the
`value_set_membership` "one kind, a `relation:` knob" lean and
dependency-cruiser's unified rule model):

```yaml
# 1. Acyclicity over content-reference edges — the lead example,
#    inexpressible with any current kind.
- id: no-proto-import-cycles
  kind: file_graph
  nodes: "proto/**/*.proto"
  edges:
    from_content:                                   # reuse crate::extract
      extract: { regex: 'import\s+"([^"]+)"' }      # capture group = referenced path
      resolve: relative_to_repo_root                # | relative_to_file
  require: acyclic

# 2. Hash-freshness (the redesigned "mtime" idea) over naming-convention edges
- id: generated-stays-fresh
  kind: file_graph
  nodes: "**/*.proto"
  edges:
    derive_target: { from: '(.*)\.proto', to: '$1.pb.go' }   # edge A(proto) -> B(generated)
  require:
    fresh: { hash: sha256, marker: '// source-sha256: ([0-9a-f]{64})' }

# 3. Layered / forbidden edges (the original allowlist intent, file->file)
- id: domain-not-depend-on-infra
  kind: file_graph
  nodes: "src/**/*.ts"
  edges:
    from_content: { extract: { regex: 'from\s+"(\.[^"]+)"' }, resolve: relative_to_file }
  require:
    forbidden_edges: [{ from: "src/domain/**", to: "src/infra/**" }]

# 4. Integrity: every referenced node exists / no unreferenced node
  require: no_dangling        # or: no_orphans, with roots: [...]
```

- **Nodes** = repo files (paths), selected by a glob. Path-based only.
- **Edges** from two extractors, both reusing `crate::extract`:
  `from_content:` (regex/structured capture → resolve relative to the
  referencing file or repo root) and `derive_target:` (name → name
  template, the generated-from case).
- **`require:`** is a closed set:
  `acyclic | no_dangling | no_orphans | forbidden_edges | fresh`.

## Critique / constraints (must hold)

1. **No mtime.** Git writes checkout-time mtime, so "generated file
   newer than its source" is meaningless on a CI clone (files often land
   with equal or arbitrary mtimes). alint already uses git author-time,
   not mtime, in `git_blame_age`; Sphinx and PHPStan both removed mtime
   from their caches; content hashing is the portable answer. The
   `fresh:` assertion is therefore **content-hash-based** — B embeds A's
   current hash via a marker (a `pair_hash`-shaped check over an edge),
   so a stale B is caught on a fresh clone. mtime, if ever offered, is an
   opt-in local-only mode, never the default.
2. **Nodes stay path-based.** Resolving module *names* (Go import paths,
   JS bare specifiers, Java FQNs) needs a per-ecosystem resolver and
   crosses back into the language-aware non-goal. File-path resolution
   only. (This is also *why* the package-graph cases don't fit — by
   design.)
3. **Determinism.** Canonical cycle representation (sorted,
   rotation-normalised so the same cycle always reports identically) +
   stable node/edge ordering, so violation output stays byte-identical
   (the snapshot-test discipline the parallel walker already upholds).
4. **Scale.** Edge *extraction* is the cost (parse/grep across the tree);
   the graph algorithms are O(V+E) except all-pairs reachability /
   transitive closure (the go forbidden-transitive-edge shape) — bound
   or gate that case explicitly. This is a `requires_full_index`,
   cross-file dispatch kind (no per-file hot path) and needs a 1M-file
   bench scenario before it ships.
5. **Security (a win).** Pure-parse, extraction-based ⇒ never shells out
   ⇒ stays out of `SPAWNING_RULE_KINDS` — the exact trap the
   `cargo metadata` design was caught in.
6. **Overlap discipline.** The graph engages *only* for multi-node /
   transitive / cycle / global properties; the shipped 1:1 and 1-level
   kinds (`registry_paths_resolve`, `import_gate`, `pair_hash`,
   `generated_file_fresh`) stay as they are. Revisit unification at the
   v1.0 DSL-freeze boundary, not before — folding four shipped kinds into
   one engine pre-1.0 is destabilising for elegance's sake.

## Plan (study-gate + prototype, then decide)

1. **Decouple** the package-graph allowlist (done: its doc is now scoped
   to rust/go and deferred).
2. **Harvest file-graph edge shapes in the 100-repo study** — briefed in
   [`case_study_100_repos.md`](./case_study_100_repos.md): per repo,
   record any file → file reference graph the project enforces (cycles /
   dangling / orphans / freshness / layering) and the *edge source*
   (content regex, naming convention, or manifest declaration).
3. **Prototype the `edges:` model on 2-3 confirmed cases** before
   generalising — the block most likely to feel wrong on contact with
   reality.
4. **Decide commit-vs-slip.** If the study confirms demand, ship
   `acyclic` first (design-doc-first, one atomic commit — clearest value,
   real gap, O(V+E), pure-parse), then `no_dangling` / `no_orphans`,
   then `forbidden_edges` / layering, then hash-`fresh`, each as a source
   confirms. If the study finds no file-graph demand, it slips to v0.13+
   without harm.

## Open questions

- Single `kind: file_graph` with a `require:` enum vs a `graph_*` family
  — lean single-kind (one edge model, many assertions); revisit only if
  the assertions' option sets diverge sharply.
- Edge-resolution base: relative-to-file vs relative-to-repo-root vs a
  configured root set — probably a per-rule `resolve:` knob (sketched
  above).
- Does `no_dangling` simply *become* `registry_paths_resolve` at graph
  scale (replace it for the multi-hop case) or sit beside it? Defer to
  the v1.0 unification question.
- Virtual / non-file nodes (e.g. a package name as a node) — explicitly
  **out** for v0.12; would reopen the resolver / non-goal line.
