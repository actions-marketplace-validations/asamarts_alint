# Generic file-dependency-graph family

Status: **Four `require:` modes shipped — 2026-06-02** (`forbidden_edges` +
`acyclic`, then `no_dangling` + `no_orphans`; the `crate::file_graph` kind, rule
count 83 → 84, CHANGELOG `[Unreleased]`). Only the content-hash `fresh` mode
remains, plus a 1M-file macro bench scenario before the v0.12 release. The study
gate is satisfied: the
[100-repo study](./case_study_log.md) surfaced **257 file-reference-graph edge
sources across 56 repos** — `file_graph` is the **#1 demand-ranked new-kind** of
the cut ([`architecture_synthesis.md`](./architecture_synthesis.md) primitive ⑤).
Ships **standalone**: the value-relation `cross_file` unification is a separate
build, and per "overlap discipline" below the shipped 1-level kinds are NOT folded
in pre-1.0. **Scope (sharpened by the study):** `file_graph` owns the *path-based*
reference graph — relative-import firewalls, derive-target codegen, content→path
references. **Module-NAME resolution** (kubernetes import-boss over `k8s.io/…`,
Go / TS bare specifiers) stays on the package-graph non-goal side; **set-equality**
of extracted values (curl option↔man-page) is the `cross_file` `set_equals`
relation, not a graph. See [`dependency_graph_allowlist.md`](./dependency_graph_allowlist.md)
for the narrow package-graph item this splits from.

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

## Demand — VALIDATED (257 edge sources, 56 repos)

The study (was "0 sources today") harvested **257 file-reference-graph edge
sources**, in nearly every repo — the decisive #1 signal. By edge-SOURCE type,
with the *path-based subset this kind owns* called out:

- **Content-regex import firewalls → `forbidden_edges` / layering** (path-based,
  ours): flask `sansio/` cannot import the Flask globals (relative `from ..globals`);
  rails `rail_inspector` require-graph; uv's `uv-fs` wrapper firewall; vscode /
  eslint *relative* module boundaries.
- **Derive-target codegen → `fresh`** (path-based, ours): proto → `*.pb.go`;
  redis `commands.def` ← `src/commands/*.json`; prometheus ×5, terraform ×3,
  aspnetcore ×3, uv ×3, llvm/roslyn/react/spark. *(The repos enforce these by
  re-running the generator (= D); `file_graph`'s `fresh` is the alint-native
  content-hash-marker variant — same intent, no generator run, no spawn-gate.)*
- **Content→path reference resolution → `no_dangling` / `no_orphans`**
  (path-based, ours): doc cross-links that must resolve to a file (git `gitlink:`,
  rubocop implicit links, markdown link targets); registry orphans (next.js, k8s
  staging).
- **Acyclicity → `acyclic`** — the clearest *capability gap* (nothing today does
  it). Direct file-level sources were thinner than the above (most cycle checks
  are package-graph / AST = D), but the proto-`import` shape is path-based and the
  lead demo.

**Explicitly NOT `file_graph`** (the honest denominator, per Scope above):
module-NAME firewalls (kubernetes import-boss `.import-restrictions` over Go module
paths — package-graph) and value set-equality (curl `tests/test1139.pl`
option↔man-page — `cross_file` `set_equals`). The 257 is the broad reference-graph
harvest; `file_graph` claims the path-based majority.

The edge SOURCES vary (content-regex · naming-convention · manifest · generated-
diff) — exactly the case for ONE generic kind over per-ecosystem rules.

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

## Prototype — the model on 3 confirmed corpus cases (gate satisfied)

The gate required validating the `edges:`/`require:` DSL against real repos before
building. Three path-based corpus cases, each a different `require:` mode:

```yaml
# A. forbidden_edges — flask sansio import firewall (study: flask, file-graph #1).
#    sansio code must not reach back into the IO-bound globals module.
- id: flask-sansio-layering
  kind: file_graph
  nodes: "src/flask/**/*.py"
  edges:
    from_content: { extract: { regex: 'from\s+(\.[\w.]+)\s+import' }, resolve: relative_to_file }
  require:
    forbidden_edges: [{ from: "src/flask/sansio/**", to: "src/flask/globals.py" }]

# B. fresh — redis commands.def derived from the per-command JSON specs
#    (repo runs the generator = D; here the hash-marker variant, no spawn).
- id: redis-commands-def-fresh
  kind: file_graph
  nodes: "src/commands/*.json"
  edges:
    derive_target: { from: 'src/commands/(.*)\.json', to: 'src/commands.def' }
  require:
    fresh: { hash: sha256, marker: '/\* @generated from .* sha256:([0-9a-f]{64}) \*/' }

# C. no_dangling — every doc cross-link resolves to a file (study: rubocop/git xref)
- id: docs-links-resolve
  kind: file_graph
  nodes: "docs/**/*.md"
  edges:
    from_content: { extract: { regex: '\]\((\.[^)]+\.md)\)' }, resolve: relative_to_file }
  require: no_dangling
```

**Findings that shaped the build:** (1) `from_content` + `resolve` cleanly express
the relative-import and content-link edges (the common case). (2) `derive_target`
+ hash-`marker` expresses codegen-freshness *without* re-running the generator —
the alint-native answer to the corpus's "generate-then-`git diff`" pattern.
(3) Bare/absolute specifiers (`from "vscode"`, `k8s.io/…`) are **dropped, not
mis-resolved** — nodes are path-based; name resolution is the package-graph
non-goal. The DSL held; no reshape needed.

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

## Plan — gates 1-3 satisfied; building (GO)

1. **Decouple** the package-graph allowlist — **done** (its doc is scoped
   to rust/go and deferred).
2. **Harvest file-graph edge shapes in the 100-repo study** — **done:** 257
   edge sources across 56 repos (see Demand + [`case_study_log.md`](./case_study_log.md)).
3. **Prototype the `edges:` model on 2-3 confirmed cases** — **done** (the
   Prototype section above; the DSL held, no reshape).
4. **Build (GO).** Ship in this sub-order, each `require:` mode as an atomic
   increment on CHANGELOG `[Unreleased]`:
   1. **`acyclic` + `forbidden_edges`** first — **SHIPPED 2026-06-02** (the two
      clearest, both pure-parse O(V+E), the best-evidenced layering case). One
      node glob + the `from_content` edge extractor (`relative_to_file` /
      `relative_to_repo_root`) + iterative-DFS rotation-canonical cycle output.
      Path-based resolution only: explicitly-relative refs (leading `.`) under
      `relative_to_file`, repo-root paths under `relative_to_repo_root`; bare
      module names / absolute / URL / computed refs are dropped. (Python dotted
      relative imports — flask `from ..globals` — need a future Python-aware
      `resolve:` mode and are *not* covered by this path-based increment.)
   2. **`no_dangling` / `no_orphans`** — **SHIPPED 2026-06-02** — reference
      integrity (doc-xref, registry orphans); shares the edge extractor.
      `no_dangling`: every path-shaped edge must resolve to a path in the index
      (file or dir). `no_orphans`: reverse-edge analysis over the node→node
      sub-graph — a node referenced by no *other* node is an orphan, unless it
      matches a `roots:` glob (a bare `require: no_orphans` = no roots; the map
      form `{ no_orphans: { roots: [...] } }` declares entry points). Both reuse
      the per-node read+extract+resolve from increment 1.
   3. **`fresh`** (content-hash-marker) — codegen freshness via `derive_target`;
      reuses the `pair_hash` digest machinery over an edge.
   Standalone kind (`crate::file_graph`), `requires_full_index` cross-file dispatch,
   never in `SPAWNING_RULE_KINDS` (pure-parse). Needs a 1M-file bench scenario
   (a new macro scenario, extends the S11-S13 pattern) before the v0.12 release.

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
