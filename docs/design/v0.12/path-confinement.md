# Path confinement — keep config-derived paths inside the repo root

**Status: SHIPPED (v0.12, pre-release hardening).** Closes a v0.12 audit
finding: a config could make a rule **read or resolve a path outside the repo
root**. A 2026-06-05 re-audit found the first pass had confined only
`file_graph` + `cross_file` (+ registry's entry resolution); the convergence
now covers **every** config-derived path read/resolve (see Scope).

## Threat

alint already gates process-spawning rule kinds (`SPAWNING_RULE_KINDS` /
`reject_command_rules_in`) so an untrusted `extends:`'d ruleset cannot shell
out. But several rule kinds turn a **config-author-controlled string** into a
filesystem path that is then **read** or **resolved**:

- `file_graph` `require: fresh` — reads the `derive_target` output and scans it
  for a hash marker.
- `file_graph` `derive_target` (`no_dangling`) — resolves the derived sibling.
- `file_graph` `from_content` edges — resolve extracted references.
- `cross_file` `relation: identical` — reads `source.file` + each target file.
- `cross_file` value relations (`equals`/set) — read source + target files.
- `cross_file` `relation: resolves` — resolves extracted path values.
- `registry_paths_resolve` — resolves each declared entry **and reads the
  `source:` registry file itself**.
- `json_schema_passes` — reads the `schema_path:` schema file.
- `pair_hash` — reads the `target:` digest-manifest file.
- `generated_file_fresh` `file:` — reads the committed output (this kind is
  spawn-gated, so confining it is defense-in-depth).

The old per-kind `normalise()` helpers had two escapes (both reproduced):

1. **Absolute path** (`to: '/etc/passwd'`, `file: '/home/u/.ssh/id_rsa'`):
   `normalise` *preserved* the `RootDir` component, and `ctx.root.join(abs)`
   **discards `root`** (Rust `Path::join` semantics) — so `read_capped` read an
   arbitrary host file. `fresh`'s marker check then leaks an existence/content
   oracle through the violation. Reachable from an `extends:`'d ruleset (these
   kinds are not spawn-gated).
2. **`..` double-dot cancellation** (`../../escape`): `normalise` preserved a
   *leading* `..` but cancelled an even number of them, so `../../escape`
   collapsed to the in-tree `escape`. The escape guard only inspected the first
   post-normalise component (`== ParentDir`), so a net-escaping reference
   slipped through and resolved to an **unintended in-tree file** — wrong
   `no_dangling`/`forbidden_edges`/`resolves` verdicts.

## Fix — one confining normaliser

`crate::pathsafe::normalize_confined(p) -> Option<PathBuf>`: a single pure
lexical normaliser that returns `None` exactly when the path leaves the root —

- any absolute component (`RootDir` / Windows `Prefix`) ⇒ `None`;
- a `..` that cannot pop a real component (empty stack) ⇒ `None` — caught
  *during* the walk, so `../../escape` and `a/../../x` are rejected, not
  inspected after the fact;
- a result that collapses to empty (`.`, `a/..`) ⇒ `None` (the root itself is
  never a valid edge/target/reference).

A `Some(_)` result is guaranteed root-relative and safe both to `root.join(..)`
and to look up in the `FileIndex`.

Every read/resolve site routes its config-derived path through
`normalize_confined`. On `None`:

- the **read** sites (`check_fresh`, `check_identical`, `read_rel`) refuse to
  read and emit a clear "escapes the repo root" violation — **no filesystem
  access outside the tree**;
- the **resolve** sites (`derive_target`/`from_content`/`resolves`) treat the
  path as unresolved (it cannot point at an in-tree file), so a dangling /
  resolves rule fires rather than silently matching the wrong file.

## Scope

**Every** config-derived path read/resolve routes through `normalize_confined`:

- `file_graph` — all four sites (`derive_target`, `fresh`, `from_content`
  resolve, and the `fresh` source/target reads).
- `cross_file` — `identical` + value relations (via `read_rel`) + `resolves`.
- `registry_paths_resolve` — the declared-entry resolution **and** the `source:`
  registry-file read (the literal-source arm; the glob-source arm only iterates
  in-tree index paths). The earlier claim that registry "never reads the
  resolved path" was wrong: it reads the `source:` file, and a literal
  absolute/`../../` `source:` was an out-of-tree read oracle until this
  convergence.
- `json_schema_passes` — the `schema_path:` read.
- `pair_hash` — the `target:` read.
- `generated_file_fresh` — the `file:` read (defense-in-depth; the kind is
  spawn-gated, so an untrusted `extends:` can't introduce it, but confining it
  keeps the invariant total: *no config-derived path read escapes the root*).

Inherently safe (no change needed): the `cross_file` glob-union `source.files`
form and every other index-driven rule only iterate `ctx.index.files()`
(in-tree paths). Symlink targets that escape the root are a **separate** vector
(the walker's `follow_links`) — now closed by pruning out-of-tree symlinks in the
walker itself (`build_walk_builder`'s `filter_entry`): a symlink whose
canonicalized target is not under the canonicalized root is dropped, and pruning
a symlink-dir also stops descent, so a committed `link -> /etc/passwd` (or
`link -> /some/dir`) can't pull out-of-tree files into the index. In-tree
symlinks are still followed.

Each newly-confined site has a "fires and is never read" regression test
(`source_escape_fires_without_reading`,
`schema_path_escape_fires_without_reading`,
`target_escape_fires_without_reading`,
`stdout_file_escape_fires_without_spawning`).
