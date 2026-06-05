# Path confinement — keep config-derived paths inside the repo root

**Status: SHIPPED (v0.12, pre-release hardening).** Closes a v0.12 audit
finding: a config could make a rule **read or resolve a path outside the repo
root**.

## Threat

alint already gates process-spawning rule kinds (`SPAWNING_RULE_KINDS` /
`reject_command_rules_in`) so an untrusted `extends:`'d ruleset cannot shell
out. But three rule kinds turn a **config-author-controlled string** into a
filesystem path that is then **read** or **resolved**:

- `file_graph` `require: fresh` — reads the `derive_target` output and scans it
  for a hash marker.
- `file_graph` `derive_target` (`no_dangling`) — resolves the derived sibling.
- `file_graph` `from_content` edges — resolve extracted references.
- `cross_file` `relation: identical` — reads `source.file` + each target file.
- `cross_file` value relations (`equals`/set) — read source + target files.
- `cross_file` `relation: resolves` — resolves extracted path values.

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

v0.12 pre-release: `file_graph` (all four sites) + `cross_file` (identical,
value relations via `read_rel`, resolves). `registry_paths_resolve` is **not** a
read oracle (index-lookup only, never reads the resolved path) but shares the
same `..`-cancellation correctness bug; converging its `normalise` onto
`normalize_confined` is the follow-up refactor (the three copies become one).
The `cross_file` glob-union `source.files` form is inherently safe — it only
iterates `ctx.index.files()` (in-tree paths).
