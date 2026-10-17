# Design doc: Close the remaining fail-open sites

Status: Implemented in the v0.19 development tree.
Decisions: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md) (DEC-2)
Demand evidence: [`../general-linter.md` section 4.12](../general-linter.md#412-current-fail-open-and-leak-sites)

## 1. Problem

Three runtime dependency failures can still look like policy passes after W1's load-time field validation and W2's typed analysis-failure model:

- a manifest used by `scope_filter.include_manifest_paths` or `exclude_manifest_paths` can be missing, unreadable, malformed, or yield an unusable value, leaving an empty set; an empty include suppresses the rule and an empty exclude broadens it;
- a rule with an active Git capability can run outside a usable Git repository and return no findings;
- the shared structured extractor returns only strings, so a JSONPath that selects a JSON/YAML/TOML/HCL number, boolean, null, array, or object silently loses that match.

All three mean alint did not obtain the configured verdict. They must remain distinguishable from a successfully evaluated policy mismatch.

## 2. Surface area

No YAML, CLI, schema, or exit-code surface is added.

Existing configurations can change from pass to failure:

- an active Git-backed rule outside a Git work tree;
- a rule whose manifest-derived scope cannot be resolved;
- `cross_file`, `file_graph`, or `registry_paths_resolve` when a structured query selects a non-string value.

`scope_filter` fields rejected by W1 remain load errors. `structured_path` kinds already compare typed `serde_json::Value` nodes and do not use the string-only extraction adapter.

## 3. Semantics

### Manifest-derived scope

The engine resolves and caches each canonical `(source, extract, derive_target)` manifest predicate once. A missing, non-regular, out-of-root, over-limit, or unreadable source is a `resource_read` analysis failure. A document parse/extract error is a `resource_parse` analysis failure. A matched non-string node is also a `resource_parse` analysis failure because no path set can be derived without inventing a string coercion.

The failure is attached to every active owning rule, names the repository-relative manifest path, and short-circuits that rule before scope matching or policy evaluation. Shared predicates still read and parse once. `when: false` and rules excluded by the invocation remain skipped rather than reporting an unused dependency failure. Analysis failures are error-bearing, non-baselinable, and non-fixable under DEC-2.

### Git dependency

The engine derives active Git dependencies from each kind's W1 `KindMeta` capability record, including nested rules and conditional capabilities such as `git_tracked_only`, `since`, `scope_filter.changed_since`, and `git_untrack`. If any active rule needs Git, the engine performs one repository probe and reuses its tracked-path result for tracked-set filtering and blame setup.

If Git cannot be spawned or the lint root is not a usable Git work tree, each otherwise-active Git-backed rule emits one pathless `resource_read` analysis failure and does not evaluate. A Git-capable option that is not active does not create a dependency. Operation-specific errors after a successful repository probe continue through their existing error paths; invalid diff/log ranges remain evaluation failures with their existing migration hints.

An initialized repository with no commits is still a usable repository for tracked-set operations. Rules whose selected commit range contains no commits may pass; absence of policy input is not conflated with absence of Git itself.

### Non-string structured extraction

The shared extractor returns two channels in one traversal:

- string lexemes, preserving existing relation behavior;
- the stable JSON types of non-string matches: `null`, `boolean`, `number`, `array`, and `object`.

It never serializes or reports the matched value. Each string-only consumer emits one ordinary, baselinable, non-fixable policy violation per extraction site, containing only the match count and sorted distinct type names. It may continue evaluating any string matches from the same query, so a mixed result cannot hide either the type mismatch or an independent relation mismatch. Fixers decline any finding whose source extraction includes non-string matches.

Structured JSONPath expressions are compiled once when configuration is built and reused for each file. An invalid query is therefore a load error rather than a repeated runtime parse.

## 4. False-positive surface

- Git is required only when the capability condition is active. A plain `file_exists` rule does not fail outside Git merely because its kind optionally supports `git_tracked_only`.
- The Git probe checks repository usability, not `HEAD`, so a new repository can still enforce tracked-set policy.
- Manifest failures are reported only for active owning rules. A false `when:` condition does not touch or diagnose its manifest.
- No scalar coercion is attempted. Treating `1`, `true`, or `null` as path/text values would hide authoring mistakes and vary across formats.
- Diagnostics expose types and counts only. Repository-controlled values, including secrets, do not enter output or baseline identities.
- `whole_file`, `lines`, and regex extraction remain string-producing and unchanged.

## 5. Implementation notes

- `crates/alint-core/src/extract.rs` owns compiled structured extractors and the partitioned result.
- `crates/alint-core/src/scope_filter.rs`, `walker.rs`, and `engine.rs` own manifest resolution failures and active-rule routing.
- `crates/alint-core/src/engine.rs` uses `KindMeta::effective_capabilities`; no second Git-kind allow-list is introduced.
- `crates/alint-rules/src/lib.rs` owns the shared value-free non-string diagnostic; `cross_file`, `file_graph`, and `registry_paths_resolve` call it.
- No dependency is added. Extraction remains linear in query matches; JSONPath compilation moves from per-file evaluation to config load.
- Constitution invariants 1 (determinism), 4 (path confinement), 7 (bounded analysis), 11 (secret-safe output), and 12 (positive-completeness reporting) apply.

## 6. Tests

Required gates:

- every JSON value type is partitioned correctly, mixed matches retain both channels, invalid JSONPath fails at build, and diagnostics contain no selected value;
- source and target extraction scenarios fire for `cross_file`; edge extraction fires for `file_graph`; registry and exclusion extraction fire for `registry_paths_resolve`;
- malformed and non-string manifest-derived scopes become analysis failures for both include and exclude predicates, including `level: warning`, and baseline generation refuses them;
- every unconditional Git-aware kind and each conditional activation reports an analysis failure outside Git, while inactive conditional capabilities remain silent;
- human and machine-format scenarios prove the outcomes survive reporting, and repeated runs are byte-identical.

## 7. Open questions

All decisions are resolved for W3.

- **Compare scalar lexemes or report?** Report types without values. Lexeme conversion would leak values and invent cross-format coercion rules; typed comparison belongs in a future relation IR.
- **Violation or analysis failure for a non-string relation value?** Ordinary policy violation. The document and query evaluated successfully; the selected value violates the current string-only relation contract.
- **Manifest non-string classification?** Analysis failure. A manifest predicate cannot construct its scope, so the host policy did not run.
- **Gate Git centrally or in each rule?** Centrally from `KindMeta`, then retain operation-specific rule errors. This covers nested and future registered kinds without another hand-maintained list.
