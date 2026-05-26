# Value-set membership family

Status: **Planned (v0.12).** ~5 corpus signals; the recurring N-in-1 /
set-relation gap that `cross_file_value_equals` (1:1) and `pair_hash`
(digest) cannot express.

## Motivation / demand

- **`registry_value_used`** — every key/value in a registry must be
  referenced ≥1× across a target file set. TypeScript
  `diagnosticMessages.json` (every code used by `errorCheck` /
  `find-unused-diagnostic`); react `codes.json` (every thrown error
  literal ∈ the registry).
- **`cross_file_keys_cover`** — value-set ⊆ key-set. pnpm
  `catalogMode: strict`: every catalog *reference* must resolve to a
  catalog *key*.
- **`cross_file_set_equals`** — two derived sets must be equal. rust
  `features` ↔ `unstable-book` pages; tf v1 ↔ v2 API goldens; protobuf
  cross-language binding parity.

## Prerequisite: audit what already exists

`registry_paths_resolve` already accepts `orphans`, `must_contain`,
and `exclude_query` (seen in the schema). **Before designing new
kinds, determine how much of the above these already cover** — e.g.
`orphans` may already express "every file under X must be referenced
by the registry" (the bidirectional/orphan case next.js raised). The
design work is partly *documentation + worked examples* of existing
capability, partly genuinely new (the value↔value, non-path set
relations).

## Sketch (pending the audit)

```yaml
# every key in a registry is referenced somewhere in a target set
- id: no-orphan-diagnostics
  kind: registry_value_used
  registry: src/compiler/diagnosticMessages.json
  extract: { json: "$.*" }          # the keys/values that must be used
  used_in: "src/**/*.ts"            # target file set (regex-scanned)
  # direction: forward (key must appear) | reverse (orphan detection)

# set(A) == set(B) after extraction
- id: features-match-book
  kind: cross_file_set_equals
  left:  { file: "compiler/.../feature_gates.rs", extract: { regex: '...' } }
  right: { dir:  "src/doc/unstable-book/src/**", extract: { ... } }
```

## Open questions

- One unified kind with a `relation: used | covers | equals` knob, or
  three distinct kinds? (Lean: start with the audit; a single
  set-relation kind may subsume all three.)
- Value extraction reuse: these want the same `extract:` block as
  `cross_file_value_equals` / `registry_paths_resolve` — share the
  `crate::extract` helper.
- Performance: "referenced ≥1× across a glob" is a whole-tree grep per
  registry entry; needs the cross-file full-index path, not per-file
  dispatch.
