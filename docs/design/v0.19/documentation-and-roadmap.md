# Design doc: documentation, recipes, and roadmap

Status: Implemented (2026-10-17).
Decisions: none (documentation-only closeout of the accepted v0.19 program in
[ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md)).
Demand evidence: W10 in [`README.md`](README.md) and the flat-format key-parity gap recorded in
[`../general-linter.md`](../general-linter.md) section 11.8.

Part of [v0.19](README.md) (work item W10).

## 1. Problem

v0.19's implementation work is complete, but its hand-off still calls W10 the next item, the
public roadmap describes the standalone kinds as conditional work, and the cookbook does not show
the already-supported `.env` / `.env.example` key-parity pattern. The two `cross_file`
`register_as` field comments also name only three of the six supported path-template tokens.
Leaving those surfaces stale would make shipped behavior harder to discover and would misstate
the release's completion.

The getting-started page also says alint ships 22 bundled rulesets while the generated
[`facts.json`](../../../facts.json) contract reports 24. The root README already matches the
contract: 102 canonical kinds, 11 aliases, 13 families, and 24 bundled rulesets.

## 2. Surface area

This item changes documentation, two Rust doc comments, and generated schema descriptions only:

- add a cookbook recipe using `cross_file`, `relation: set_equals`, and a capture-group regex to
  compare dotenv key sets;
- list every supported `register_as` token in both current `cross_file` field comments;
- regenerate both committed schema copies so the `register_as` description follows its Rust
  source;
- close W10 in the v0.19 hand-off and replace provisional roadmap wording with the delivered W8,
  W9, and bundled-ruleset surfaces;
- regenerate `roadmap.json` from the canonical roadmap;
- remove the stale duplicated bundled-ruleset count from the getting-started page;
- replace the general-linter proposal's future-tense key-parity note with a link to the recipe.

There is no DSL validation, schema-shape, runtime, output, fix, baseline, dependency, subprocess,
or network change. The changelog therefore needs no behavior or migration entry.

## 3. Semantics and compatibility

The recipe makes `.env.example` the source of truth and compares its unique captured key set with
`.env`:

```yaml
version: 1
rules:
  - id: env-key-parity
    kind: cross_file
    source:
      file: .env.example
      extract:
        regex: '(?m)^[ \t]*(?:export[ \t]+)?([A-Za-z_][A-Za-z0-9_]*)[ \t]*='
    targets:
      - file: .env
        extract:
          regex: '(?m)^[ \t]*(?:export[ \t]+)?([A-Za-z_][A-Za-z0-9_]*)[ \t]*='
    relation: set_equals
    level: error
```

Anchoring at line start excludes commented examples; optional `export` accepts shell-compatible
dotenv declarations; capture group 1 returns only the key. `set_equals` deliberately compares
case-sensitive sets, so declaration order does not matter and duplicate declarations collapse.
A missing `.env` remains a finding: silently allowing it would weaken the advertised parity
check. Repositories that do not materialize `.env` in CI should target a generated or tracked
environment file that exists in that job, or omit the rule there.

Counts remain generated-contract facts, not estimates. W10 removes the duplicated count from the
getting-started page instead of adding another synchronization mechanism, and it does not edit a
correct root README count. The v0.19 release commit updated the roadmap's `Latest release` from
v0.18.4 to v0.19.0 through the normal release procedure.

## 4. Implementation notes

The cookbook placement is under its existing cross-file relationships section, avoiding a second
concept page for one pattern. The roadmap source remains `docs/design/ROADMAP.md`; `roadmap.json`
is regenerated with `xtask gen-roadmap`, never hand-edited. The schema descriptions derive from
the Rust option comment and are regenerated with `xtask gen-schema`. Prose that does not need a
count does not duplicate one or introduce a new count-synchronization mechanism.

## 5. Tests

- Run the documented rule through the actual CLI against one equal and one unequal dotenv pair;
  the equal case must pass and the unequal case must report the missing key.
- Run `xtask docs-export --check` so every fenced configuration, internal link, and generated docs
  surface remains valid.
- Run `xtask gen-schema --check`, `xtask gen-facts --check`, and `xtask gen-roadmap --check` to
  prove generated schema, count, and roadmap parity.
- Run the documentation, example, formatting, lint, workspace, and repository preflight gates.

## 6. False positives and false negatives

The identifier regex intentionally accepts the portable dotenv key subset. Projects that permit
dots, hyphens, or other non-portable key characters must widen the capture pattern in both places.
Commented declarations are ignored. Because this is set parity, duplicate declarations are not a
finding; a separate duplicate-key policy is required when multiplicity matters.

## 7. Open questions

None. The implementation uses the existing `cross_file` contract and the established generated
facts and roadmap sources of truth.
