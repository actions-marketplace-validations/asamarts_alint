# Design doc: C1 version-SSOT bundled ruleset

Status: Implemented (2026-10-17)

Decisions: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md) (W9 delivery order); no new architectural decision.

Demand evidence: [`../rule-coverage-gaps.md`](../rule-coverage-gaps.md), Family C / C1; release-tool survey cited there.

## 1. Problem

Repositories commonly repeat one release version in package manifests, language metadata, charts, API descriptions, and a changelog. Drift produces packages and release artifacts with conflicting identities. Existing version-bump tools solve this per ecosystem; alint's `cross_file` rule can enforce the language-agnostic file-to-file invariant, but it was not available as a reusable policy.

C1 also names Git tags. Local Git-tag inspection is a distinct repository-derived capability and remains C6. This bundle does not approximate it with a subprocess.

## 2. Surface area

The bundle is embedded and resolved offline as:

```yaml
extends:
  - alint://bundled/versioning@v1
```

The root file `VERSION` is the sole authority. Its presence opts the repository in. No manifest is auto-elected: choosing whichever candidate happens to exist would make source direction and an Unsafe repair depend on discovery order.

`VERSION` must contain one portable literal token of at most 256 bytes: an optional leading `v`/`V`, a decimal digit, then ASCII letters, digits, `.`, `_`, `+`, `!`, `~`, or `-`, with optional surrounding horizontal whitespace and one final newline. Backslashes, quotes, controls, common computed-value markers such as `$()`, `${}`, and `{{}}`, and multiple lines are rejected. This is a transport-safety grammar, not SemVer policy.

The bundle declares three namespaced facts:

- `versioning_has_version_file`: root `VERSION` exists;
- `versioning_has_valid_version`: root `VERSION` matches the one-token contract;
- `versioning_has_changelog_file`: root `CHANGELOG.md` exists, allowing the fixed changelog target
  to distinguish an optional absent file from a present file with no trustworthy release heading.

It ships one source validation rule and ten target rules:

| Rule ID | Target |
|---|---|
| `versioning-version-file-one-line` | malformed root `VERSION` |
| `versioning-package-json` | `**/package.json` `$.version` |
| `versioning-cargo-package` | `[package].version` in `**/Cargo.toml` |
| `versioning-cargo-workspace` | `[workspace.package].version` in `**/Cargo.toml` |
| `versioning-pyproject-project` | `$.project.version` in `**/pyproject.toml` |
| `versioning-pyproject-poetry` | `$.tool.poetry.version` in `**/pyproject.toml` |
| `versioning-python-module` | `__version__` in `**/{__init__,version,_version}.py` |
| `versioning-helm-chart` | `$.appVersion` in `**/Chart.yaml` |
| `versioning-openapi-json` | `$.info.version` in `**/{openapi,swagger}.json` |
| `versioning-openapi-yaml` | `$.info.version` in `**/{openapi,swagger}.{yml,yaml}` |
| `versioning-changelog` | first released `##` heading in root `CHANGELOG.md` |

There is no template, parameter surface, new kind, schema option, dependency, network access, or subprocess.

## 3. Semantics

All ten target rules use the existing `cross_file` kind with `relation: equals` and `allow_missing_target: true`.

- A missing `VERSION` makes every rule silent.
- A malformed `VERSION` produces exactly the source-validation finding; target comparisons do not run.
- Missing target files or missing target fields are silent. C1 is consistency policy, not mandatory-file policy. A present root changelog with no trustworthy release heading is the exception: it fails rather than letting inert fenced/HTML content hide release drift.
- Every extracted target value must equal the extracted `VERSION` token byte-for-byte. There is no case folding or semantic-version normalization.
- JSON, YAML, and TOML targets use structured extractors; Cargo package and workspace fields are
  root-qualified JSONPaths rather than section-looking raw-text regexes.
- Regex targets capture only the version token so fixes preserve surrounding syntax. Python
  `__version__` candidates are admitted only on lines the shared lexical guard classifies as
  top-level code; assignments inside module docstrings, strings, comments, continuations, or
  bracketed expressions are ignored. The changelog selector conservatively declines a file
  containing a backtick, tilde, or `<` before the first release heading; this prevents CommonMark
  fences and HTML blocks from impersonating releases.
- The changelog extractor skips prose and an optional `Unreleased` heading, then captures the
  version token from the first released level-two heading. Optional `Version`, brackets, and a
  leading `v` are part of the recognized heading syntax. The captured `v` is retained so a source
  such as `v2.4.0` cannot become `vv2.4.0` during repair.
- Malformed present structured data is an analysis failure under W2 rather than a policy pass.

All target rules are `error`. They expose the existing `fix: { sync_from: {} }`. ADR-0017 classifies source-to-target content propagation as **Unsafe**: bare `alint fix` suggests it; `alint fix --unsafe-fixes` applies it. Repairs only rewrite already-extracted target values and converge on the authority. They do not create absent files or fields.

## 4. False-positive budget

The false-positive policy is opt-in and source-directed:

1. A repository is unaffected unless it deliberately has root `VERSION`.
2. Optional targets do not turn ecosystem absence into a failure.
3. Only conventional, unambiguous target locations are inspected.
4. Equality is literal. A repository that intentionally publishes different component versions should not enable this bundle, or should demote/disable the specific public rule IDs.
5. Fixes require explicit Unsafe permission.

Known false negatives are deliberate boundaries:

- Git tags, signatures, ordering, and monotonicity: C6;
- semver range intersection/satisfaction: C4;
- values generated at build time or computed through template expressions;
- arbitrary changelog heading conventions;
- importing bump-tool configuration as the source manifest.

## 5. Implementation notes

- Bundle: `crates/alint-dsl/rulesets/v1/versioning.yml`.
- Registry: `crates/alint-dsl/src/bundled.rs`.
- Runtime substrate: `crates/alint-rules/src/cross_file/` and existing extractors in `alint-core`.
- Facts are namespaced because extended facts replace by ID.
- Rule IDs are stable override keys for `@v1` and must not be renamed in place.
- Evaluation remains bounded by the repository index, configured globs, parser limits, and `cross_file`'s deterministic path order.

No `KindMeta`, hand-written options type, JSON Schema branch, generated kind documentation, or kind dispatch change is needed: W9 permits bundles built from existing primitives.

## 6. Tests

Declarative scenarios cover:

- an irrelevant repository with no `VERSION`;
- malformed and unsafe source tokens producing one validation finding and no suggestion or application even in Unsafe mode;
- matching targets and absent optional targets;
- drift in every target rule, including both OpenAPI encodings;
- bare-fix suggestions, Unsafe application, and a clean post-fix check.

The normal bundle coverage audit, generated-schema/facts/docs gates, fingerprint golden, shell contracts, and workspace tests remain release gates.

## 7. Open questions

None for `@v1`.

Deferred work is explicit rather than hidden in this surface:

- **Git tag:** C6, because it needs local-Git capability and no fix.
- **Bump-tool manifest ingestion:** later ergonomics; it must not change source direction implicitly.
- **SemVer policy:** C4, separate from exact equality.
