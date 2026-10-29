# Design doc: C2 toolchain-pin bundled ruleset

Status: Implemented (2026-10-17)

Decisions: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md) (W9 delivery order); no new architectural decision.

Demand evidence: [`../rule-coverage-gaps.md`](../rule-coverage-gaps.md), Family C / C2.

## 1. Problem

A repository often declares one language or SDK version in several places: a local version-manager file, an ecosystem manifest, `.tool-versions`, a Docker base image, and a CI setup action. Drift means local development, containers, and CI compile or test different inputs.

C2 is exact-pin consistency. Semver ranges, moving channels, matrices, and computed expressions need semantic or evaluation capabilities that `cross_file` does not provide. Silently treating those values as exact strings would create false confidence, so the bundle recognizes only literal exact forms and leaves range policy to C4.

## 2. Surface area

The bundle is embedded and resolved offline as:

```yaml
extends:
  - alint://bundled/toolchain-pins@v1
```

It is opt-in and does not alter the existing Node, Rust, Python, Java, or .NET bundles.

Each ecosystem has one root authority. The source is never selected by whichever candidate file happens to exist:

| Ecosystem | Authority | Compared representation |
|---|---|---|
| Node.js | `.nvmrc` | exact `major.minor.patch`, optional prerelease/build; leading `v` ignored |
| Rust | `rust-toolchain.toml` `[toolchain].channel` | exact release or dated nightly |
| Python | `.python-version` | exact `major.minor.patch`, optional prerelease/build; leading `v` ignored |
| Ruby | `.ruby-version` | exact `major.minor.patch`, optional prerelease/build; leading `v` ignored |
| JVM | `.java-version` | exact three-component release core; vendor/build suffix retained in files but excluded from cross-vendor comparison |
| .NET | `global.json` `sdk.version` | exact SDK version without a leading `v` |

Source presence activates only that ecosystem. The bundle declares namespaced `*_anchor_present` and `*_anchor_exact` facts. Structured authorities are validated from the canonical node itself, not from raw-text lookalikes elsewhere in the document. Stable-release facts additionally guard Docker comparisons whose tags encode only a stable three-component core.

The public rule surface is 24 stable IDs: four per ecosystem.

| Ecosystem | Source validation | Declarations | Dockerfiles | GitHub Actions |
|---|---|---|---|---|
| Node.js | `toolchain-node-anchor-exact` | `toolchain-node-declarations-coherent` | `toolchain-node-dockerfiles-coherent` | `toolchain-node-github-actions-coherent` |
| Rust | `toolchain-rust-anchor-exact` | `toolchain-rust-declarations-coherent` | `toolchain-rust-dockerfiles-coherent` | `toolchain-rust-github-actions-coherent` |
| Python | `toolchain-python-anchor-exact` | `toolchain-python-declarations-coherent` | `toolchain-python-dockerfiles-coherent` | `toolchain-python-github-actions-coherent` |
| Ruby | `toolchain-ruby-anchor-exact` | `toolchain-ruby-declarations-coherent` | `toolchain-ruby-dockerfiles-coherent` | `toolchain-ruby-github-actions-coherent` |
| JVM | `toolchain-jvm-anchor-exact` | `toolchain-jvm-declarations-coherent` | `toolchain-jvm-dockerfiles-coherent` | `toolchain-jvm-github-actions-coherent` |
| .NET | `toolchain-dotnet-anchor-exact` | `toolchain-dotnet-declarations-coherent` | `toolchain-dotnet-dockerfiles-coherent` | `toolchain-dotnet-github-actions-coherent` |

Declaration targets are:

- Node.js: `.node-version`, `.tool-versions` `node`/`nodejs`, and exact root `package.json` `engines.node` or `volta.node`;
- Rust: legacy `rust-toolchain` `[toolchain].channel` or one-line form, and `.tool-versions` `rust`;
- Python: `.tool-versions` `python`, `runtime.txt`, and `Pipfile` `[requires].python_full_version`;
- Ruby: `.tool-versions` `ruby`, `Gemfile` `ruby`, and `Gemfile.lock` `RUBY VERSION`;
- JVM: `.tool-versions` `java` and `.sdkmanrc` `java=`;
- .NET: `.tool-versions` `dotnet`/`dotnet-core` and root `global.json` `tools.dotnet`.

Docker targets are `**/Dockerfile*`. Recognized images are the official Node, Rust, Python, and Ruby repositories; Eclipse Temurin/Azul/Amazon Corretto/OpenJDK JVM images; and `mcr.microsoft.com/dotnet/sdk`. Registry prefixes, distro/vendor suffixes, digests, and multi-stage `AS` clauses are preserved. Candidate regex captures are accepted only on lines the existing Dockerfile parser classifies as top-level `FROM` instructions, so continuations and heredoc payloads cannot impersonate a base image.

GitHub Actions targets are root `.github/workflows/*.{yml,yaml}` steps using the ecosystem setup actions. JSONPath filters bind the action and input in one step; an additional exact-token filter admits only the bundle's supported literal representation. The bundle does not compare leading-`v` forms, moving aliases, ranges, expressions, unrelated `with` objects, or similarly named actions.

There is no new Rust rule kind, extractor kind, fix operation, template, dependency, subprocess, Git read, or network access. Two narrow existing-primitive extensions are reusable outside this bundle: `file_content_matches.extract` applies any existing structured extractor before matching, and `cross_file.targets.matches` filters extracted target strings before comparison and repair.

## 3. Semantics

Every source validation rule runs when its authority exists. A malformed or moving source produces one warning and suppresses that ecosystem's consistency rules.

Every consistency rule uses existing `cross_file` `relation: equals`, optional targets, exact capture/structured extraction, and—where extraction syntax is necessarily broader than policy—the target's `matches` filter:

- missing authority: ecosystem silent;
- valid authority plus missing target: silent;
- present recognized exact target equal to source: pass;
- present recognized exact target different from source: warning;
- present range, moving alias, variable, matrix expression, leading-`v` target, or unsupported syntax: not selected by C2; a target whose values are all filtered out is treated as absent when `allow_missing_target` is true;
- malformed selected structured input or Dockerfile: W2 analysis failure, not a policy pass.

JVM is the intentional heterogeneous exception. `.java-version` may carry a vendor/build identifier such as `temurin-21.0.8+9`, a Docker tag may encode `21.0.8_9`, and `setup-java` normally receives `21.0.8`. All compare the exact `21.0.8` release core. Suffixes remain untouched when no drift exists.

All 24 rules default to `warning`: these are opt-in consistency signals across diverse ecosystem conventions, not unconditional repository requirements. Consumers can promote public IDs to `error`.

The 18 consistency rules expose existing `fix: { sync_from: {} }`; six source-validation rules do not. `sync_from` remains **Unsafe** under ADR-0017. Bare `alint fix` suggests it; `--unsafe-fixes` applies it. Regex and structured fixers replace only the captured/value node, preserve surrounding syntax and unrelated suffixes, and do not create missing values.

## 4. False-positive budget

Controls:

1. Enabling the bundle alone is insufficient; each ecosystem needs its named authority.
2. No authority fallback or auto-election. This keeps repair direction deterministic.
3. Targets are optional and syntax-filtered.
4. CI matching binds the setup action identity and its own version input in the same step.
5. Docker matching is restricted to known toolchain image families and literal tags.
6. Ranges and computed values are silent rather than compared incorrectly.
7. Fixes require explicit Unsafe permission.

Accepted false negatives:

- Node range compatibility in `engines.node` or Volta policies that are not exact: C4;
- aliases such as `lts/*`, `stable`, `latest`, `3.13`, or `21`;
- matrix/interpolation/expression-based setup inputs and Docker build arguments;
- CI systems other than GitHub Actions;
- project-specific version files not listed above;
- vendor/build-level JVM drift beyond the comparable three-component release core.

These are capability boundaries, not claims of consistency.

## 5. Implementation notes

- Bundle: `crates/alint-dsl/rulesets/v1/toolchain-pins.yml`.
- Registry: `crates/alint-dsl/src/bundled.rs`.
- Runtime substrate: built-in repository facts, `cross_file`, regex extraction, and JSON/YAML/TOML extraction.
- `file_content_matches.extract` validates the Rust and .NET authority nodes before consistency rules are enabled; malformed or non-string nodes are non-matches, while parser failures remain bounded and silent at fact-evaluation time like the pre-existing fact.
- `cross_file.targets.matches` is an optional Rust-regex predicate over extracted strings. Non-matches are absent optional values; matched values retain their original spans, and the same predicate runs before fixer correlation.
- Root-qualified JSONPath unions keep Node declaration selection to `$.engines.node` and `$.volta.node`; nested lookalikes are excluded.
- One rule is used per source/target syntax family because a `cross_file` target glob has one extractor. Heterogeneous fixed targets use an ordered target list.
- Regex capture group 1 is the compared and repaired token; every structural group is noncapturing.
- Duplicate target paths use one extractor per rule so fixer correlation cannot select a different extractor for the same file.
- Rule and fact IDs are public override keys. Do not rename them within `@v1`.
- Evaluation is deterministic and bounded by the repository index, parser limits, fixed rule count, and sorted path traversal.

No `KindMeta` record or rule dispatch changed. The shared fact and `cross_file` option schemas, generated JSON Schema, and documentation include the two added optional fields. The bundle itself remains the existing-primitive W9 route approved by the general-linter program.

## 6. Tests

Declarative scenarios cover:

- a repository with target-like files but no authorities (silent);
- malformed/moving authorities for all six ecosystems (one warning each);
- a fully synchronized polyglot repository across every supported target class;
- drift across every declaration, Docker, and GitHub Actions target class;
- exact Node drift with bare-fix suggestions, Unsafe application, clean convergence, and selective
  repair when a structured union query also selects an excluded range or duplicate same-value
  declaration;
- ranges, computed targets, leading-`v` workflow inputs, nested package lookalikes, and non-exact
  optional .NET declarations remaining outside C2 without false failures;
- Rust legacy TOML with fields after `channel` and unrelated-table or multiline-string
  lookalikes, Python prerelease comparison and string/docstring filtering, nested
  structured-authority spoof rejection, Gemfile comments/strings/heredocs, and Docker heredoc
  `FROM` text ignored during both checking and repair;
- fail-loud extraction and syntax-scanner ceilings, plus fix-time edit-count and projected-output
  ceilings.

The bundle coverage audit, fingerprint golden, generated schema/facts/docs gates, shell contracts, and workspace tests remain release gates.

## 7. Open questions

None for `@v1`.

Future additions use new stable rule IDs rather than broadening existing matchers incompatibly. Range semantics remain C4. New CI providers or source authorities need their own evidence, exact syntax, scenarios, and false-positive analysis.
