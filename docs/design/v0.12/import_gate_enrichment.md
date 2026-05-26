# Richer `import_gate`: default-deny + glob-discovered rule files

Status: **Planned (v0.12).** 2 high-value corpus signals; an extension
of the shipped `import_gate`, not a new kind.

## Motivation / demand

`import_gate` today is forbid+allow regex over a captured import
target, one rule per `paths:` scope. Two corpus repos need more:

- **vscode** — `code-import-patterns` is a *generated default-deny
  per-file allowlist*: hundreds of entries plus a layer ordering
  (base → platform → editor → workbench). `import_gate`'s forbid+allow
  only ports the *uniform* cross-layer bans; the full layered allowlist
  doesn't fit. Demands a **default-deny / table-driven mode**.
- **kubernetes** — 66 per-directory `.import-restrictions` files
  (each a YAML allow/forbid list scoped to its subtree). Today that is
  66 hand-written `import_gate` rules. Demands **glob-discovered
  per-directory rule files** — point one rule at `**/.import-restrictions`
  and have alint apply each file to its own subtree.

## Sketch

```yaml
# default-deny / table-driven mode (vscode)
- id: layer-firewall
  kind: import_gate
  language: js
  paths: "src/vs/base/**"
  mode: default_deny          # anything not in `allow:` is a violation
  allow:
    - "vs/base/**"
    - "vs/nls"

# glob-discovered per-directory rule files (kubernetes)
- id: import-restrictions
  kind: import_gate
  language: go
  rules_from: "**/.import-restrictions"   # each file scopes its own dir
  format: k8s_import_restrictions          # known schema, or `generic` + a spec
```

- `mode: default_deny` — invert the default; `allow:` becomes the
  whitelist, everything else fails.
- `rules_from:` — discover per-directory rule files via glob; each
  file's forbid/allow applies to *its* directory subtree. Needs a
  declared `format:` (a couple of known schemas + a generic
  field-mapping spec).

## Open questions

- `default_deny` ordering: vscode's layering implies allow-lists that
  *cascade* by layer. Start with flat per-scope default-deny; cascading
  layers may be a follow-up.
- `rules_from:` schema coupling: hard-code the k8s `.import-restrictions`
  shape, or a generic "this glob yields allow/forbid lists keyed by
  dir" spec? (Lean: generic spec; k8s is the worked example.)
- This stays *source-text* import analysis. The resolved-graph /
  lockfile allowlist (rust PERMITTED_DEPENDENCIES, go transitive
  closure) is a **separate** kind — see
  [`dependency_graph_allowlist.md`](./dependency_graph_allowlist.md).
