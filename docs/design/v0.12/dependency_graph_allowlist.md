# Dependency-graph allowlist kind

Status: **Planned (v0.12).** Distinct from `import_gate`; 2 corpus
signals, both high-stakes.

## Motivation / demand

`import_gate` reads *source text* — it expresses "directory X must not
`import` Y" well, but cannot see the *resolved dependency graph*. Two
corpus repos enforce allowlists over the resolved graph:

- **rust** — `tidy::deps` hard-codes per-tier `PERMITTED_DEPENDENCIES`;
  `Cargo.lock` must not introduce any crate outside the list. This is a
  `cargo metadata` graph walk, not a source scan — the 30-repo pass
  initially mis-mapped it to `import_gate` and corrected course.
- **go** — `deps_test.go`'s full *transitive* package-dependency
  closure (runtime ↛ fmt/os/reflect, computed over the import DAG).
  `import_gate` catches the flat per-file edges (the bulk of the
  value) but not the transitive closure.

## Sketch

A new kind that consumes a resolved-graph manifest rather than source:

```yaml
- id: permitted-deps
  kind: dependency_graph_allowlist
  ecosystem: cargo            # cargo (Cargo.lock) | go (go.mod/go.sum) | ...
  allow:                      # crates permitted in the resolved graph
    - "serde"
    - "regex"
  # tier:                     # optional: per-target/per-workspace-member tiers
```

- `ecosystem:` selects the lockfile/manifest parser (`Cargo.lock`,
  `go.mod` + module graph, later `package-lock.json` / `pnpm-lock.yaml`).
- `allow:` is the permitted set; anything else in the *resolved* graph
  is a violation.
- Detection-only (like the hash/registry kinds); no graph mutation.

## Open questions

- Parse `Cargo.lock` directly, or shell to `cargo metadata`? Direct
  parse keeps alint's "no toolchain required" property; `cargo
  metadata` is more accurate for feature-resolved graphs. (Lean:
  direct `Cargo.lock` parse first — it is what `tidy::deps` reads.)
- Transitive closure for go is a real graph traversal (`go.sum` lists
  the flat set; the *forbidden-edge* form needs the import graph).
  Scope v0.12 to the flat-allowlist case (rust); leave transitive
  edge-firewalls as a follow-up if the 100-repo study confirms demand.
- This is a **spawning-or-parsing** decision: if it ever shells out it
  must join `SPAWNING_RULE_KINDS`; a pure-parse design avoids that.
