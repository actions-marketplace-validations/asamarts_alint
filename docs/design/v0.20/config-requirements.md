# Design doc: configuration requirements and source-version compatibility

Status: Accepted design (2026-10-18); implementation not started.
Decisions: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md)
question Q11 and [ADR-0022](../../adr/0022-policy-authoring-loop-contracts.md).
Demand evidence: Phase 1b publishes reusable policy tests and bundled rulesets which need an
enforceable minimum binary version before `version: 2` exists.

## 1. Problem

The effective config loader validates only the final merged `version`. During `extends` resolution,
an extended source's `version` is deliberately cleared before merge because it describes that
source rather than its consumer. The source version is not first validated or retained. A v1 root
can therefore absorb a source which declares another version as long as the root's final version is
1. This is harmless while only v1 exists, but it would make the v2 direction rule unenforceable and
could reinterpret newer syntax under older semantics.

There is also no way for a reusable ruleset to say which alint release first implements the kinds,
options, test schema, or behavior it relies on. An older strict parser will reject an unknown
`requires` key safely, but a compatible parser needs a precise source-local diagnostic and future
rulesets need a durable minimum-version contract.

v0.20 fixes both before Phase 2/3 add another config language. Every complete source is versioned
and checked before composition; every source may state a literal minimum alint version which cannot
be overridden or merged away.

## 2. Surface area

### 2.1 YAML syntax

Any complete config source and any local drop-in may declare:

```yaml
version: 1
requires:
  alint: "0.20.0"
```

`requires` is an optional closed mapping. When present it must contain `alint`; v0.20 recognizes no
other keys. The value is a minimum stable release with exactly three unsigned decimal components:

```text
MAJOR.MINOR.PATCH
```

Each component has no sign and no leading zero except the single digit `0`, and must fit `u64`.
Prefixes (`v0.20.0`), operators (`>=0.20.0`), missing components, wildcards, prerelease/build
suffixes, whitespace, interpolation placeholders, YAML tags, sequences, and numbers are rejected.
The deliberately narrow grammar avoids accepting a SemVer range whose upper bounds or prerelease
semantics could later be misread. A range language, if ever needed, requires a new config version.

The meaning is fixed: the running alint release must be at least the declared version. Comparison is
numeric by component. A prerelease build does not satisfy the stable release with the same numeric
triplet; it does satisfy a lower triplet. Build metadata does not affect ordering. alint reads its
own compile-time version components and prerelease marker, so no runtime parser dependency or
environment input controls the comparison.

`requires` is optional. Absence means no minimum beyond the config language and fields which the
parser already supports. Existing v1 configs therefore keep working.

### 2.2 Which sources carry a version

Sources are classified before recursive loading:

| Source | `version` rule | `requires` rule |
|---|---|---|
| Root `.alint.yml` / explicit config | Required; selects the consumer language. | Optional; checked for that source. |
| Local `extends` target | Required; describes that complete source. | Optional; checked for that source. |
| Bundled `alint://` source | Required. | Optional; checked for that source. |
| Remote `https://` source | Required. | Optional; checked after fetch/cache read. |
| Nested config root | Required; its immediate consumer is the parent config version. | Optional; checked for that source. |
| `.alint.d` drop-in | May omit and inherit the root version; if present, must equal it. | Optional; checked independently. |

A complete extended source which omits `version` becomes an error in v0.20. This closes ambiguity
before v2 and is a deliberate pre-1.0 migration. Repository-owned rulesets, examples, scenarios, and
bundled files are migrated in the same implementation change. Drop-ins remain fragments and may
omit it because they are explicitly merged into one root under the same trust authority.

A source's version is metadata, not a mergeable top-level setting. It is validated and recorded,
then removed from field merge exactly as today. A child cannot override the consumer's version by
appearing later in an extends list or drop-in order.

### 2.3 Compatibility direction

Compatibility is checked at every immediate composition edge:

- a v1 source may extend only v1 sources;
- a future v2 source may extend v1 or v2 sources;
- a v1 source can never extend a v2 source, even when that v1 source is itself included by a v2
  root;
- a source with a version unsupported by the running binary is rejected before its rules are
  merged or its own extends are followed.

The recursive edge rule matters. Given `v2 root -> v1 base -> v2 leaf`, the second edge is invalid;
the outer v2 consumer does not retroactively give the v1 base authority to interpret v2.

v0.20 supports only v1 and therefore accepts only v1 at every complete source. Phase 3 must parse
each source with its declared language into a version-neutral composition form and define the
v1-to-v2 upgrade. This design commits only the safe direction, not the future field mapping.

### 2.4 Per-source requirement semantics

A requirement belongs to the source which declares it. The loader checks it immediately after that
source passes guarded parsing and before:

- resolving that source's own extends;
- merging its templates, facts, variables, rules, or top-level fields;
- expanding templates or building rules;
- walking the repository or invoking any capability.

A remote source necessarily must be fetched or read from cache before its requirement is known.
v0.20 retains the existing rule that remote and bundled sources are leaves, so they cannot initiate
child fetches. For source classes that do support `extends`, an unsatisfied requirement is rejected
before any child source is resolved.

Requirements do not field-merge. A consumer cannot lower, delete, or replace an inherited source's
minimum. Every source in the composition graph must be satisfied independently. The effective
loaded config retains the maximum satisfied requirement and a source-provenance list for explain,
diagnostics, export, and tests, but that summary is informational; validation has already happened
per source.

`requires.alint` cannot enable command execution, trust a remote, lift path confinement, permit a
fix tier, turn on out-of-root reads, select a config version, or change feature flags. It is a
compatibility predicate only.

### 2.5 Diagnostics and command behavior

An unsatisfied requirement is a configuration error (CLI exit 2) in every config consumer: check,
fix, list, explain, baseline, validate-config, LSP, export, docs tooling, benchmark setup, and policy
tests. The diagnostic identifies:

- the normalized source (`.alint.yml`, a confined relative path, a bundled URI, or a sanitized
  remote URL);
- the source's config version;
- the required alint version;
- the running alint version.

Example:

```text
rules/team.yml: requires alint 0.21.0 or newer (running 0.20.0; config version 1)
```

Remote diagnostics remove URL userinfo and redact query/fragment data. Absolute local checkout
paths are shortened to repository- or config-root-relative paths when possible. A requirement
failure is emitted once per load source even when a diamond extends graph references it multiple
times.

There is no `--ignore-requires`, environment override, or warning mode. Continuing would evaluate a
policy under semantics its author explicitly declared insufficient. LSP publishes the same error at
the declaring config and does not retain stale diagnostics/results from a previously loadable
version.

### 2.6 Schemas and discovery

The generated v1 config schemas add:

```json
{
  "requires": {
    "type": "object",
    "additionalProperties": false,
    "required": ["alint"],
    "properties": {
      "alint": {
        "type": "string",
        "pattern": "^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$"
      }
    }
  }
}
```

Runtime numeric overflow and running-version comparison remain outside JSON Schema. The in-crate
schema copy and `schemas/v1/config.json` update atomically. Examples which demonstrate the field use
a quoted scalar for clarity.

Phase 1b bumps `facts.json` to format v5 and adds one generated object:

```json
{
  "compatibility": {
    "config_versions": [1],
    "policy_test_versions": [1],
    "rule_trace": "experimental"
  }
}
```

The same format bump carries `KindMeta`'s trace-stage metadata. The docs-site bundle/consumer parity
gate is updated in the release even though its facts loader ignores unknown keys. No hand-authored
compatibility list is added outside the loader/test-schema constants used by generation.

## 3. Semantics

### 3.1 Load order

For each source, the loader performs this sequence:

1. Apply the existing byte-size, YAML flow-depth, alias-expansion, duplicate/ambiguous-key, and tag
   guards.
2. Parse the source into its raw typed form with source-aware diagnostics.
3. Read the literal source `version` and `requires` metadata. Reject interpolation or a forbidden
   omission according to source class.
4. Reject a language version unsupported by the binary.
5. Check the immediate consumer/source direction rule.
6. Compare the running binary with `requires.alint`.
7. Record source compatibility metadata and only then resolve its extends or merge its content.

The checks occur before `drop_top_level_settings` clears the source-local version. Memoization stores
only an already validated source together with the compatibility context which affects it. Its key
therefore includes the immediate consumer version in addition to path, confinement, and trust data;
a source loaded under v1 cannot reuse a cache entry validated only under a future v2 consumer.

v0.20 can retain the current single v1 typed parser. When multi-version parsing arrives, guarded
version dispatch must happen before version-specific typed deserialization. It must not deserialize
v2 fields through the v1 type and reinterpret unknown content.

### 3.2 Version and requirement provenance

The loader records one immutable compatibility record per canonical source:

```text
source identity
source class
config version
optional minimum alint version
immediate consumer version
```

Records use canonical identities for memoization but safe normalized display identities for output.
They survive composition in load metadata even though source `version` and `requires` never enter
rule field merge. The effective maximum minimum-version is derived from these records.

Diamond references to the same canonical source and same compatibility context share one record.
The same file reached under a different consumer version is validated separately. Conflicting
content at one URL/cache identity remains governed by the existing immutable cache and integrity
rules; requirements do not weaken them.

### 3.3 Bundled rulesets

Existing bundles need no requirement merely because v0.20 implements this field. A bundle declares
`requires.alint` only when it uses a kind, option, bug-fixed semantic, or composition feature which
would otherwise parse on an older supporting binary and behave incorrectly. The value is the first
release which provides the complete required behavior, not the latest release at edit time.

The bundle generator preserves the requirement in embedded bytes and the source tree. Schema,
embedded, packaged, and docs copies are parity-gated. Policy-test bundle scenarios load the bundled
URI so tests exercise the retained metadata.

## 4. False-positive surface

Treating a minimum version as a general SemVer range would invite ambiguous upper bounds and
prerelease handling. The stable triplet grammar has one comparison and can be implemented/tested
without a range evaluator.

Checking only the final merged version would repeat the current blind spot. Validation is per source
and per immediate edge before version metadata is cleared. Checking only the outer root is
insufficient for diamond and nested chains.

A strict rule for version omission could incorrectly reject drop-in fragments which have never been
standalone configs. Source classification keeps that established fragment behavior while requiring
all reusable complete sources to be explicit.

A requirement error can leak a credential-bearing remote URL or host checkout path. Source display
normalization happens before diagnostic construction, and tests plant secrets in userinfo,
query/fragment, temp roots, and environment values.

## 5. Implementation notes

Add a small stable-release version type with checked decimal parsing and ordering based on compile-
time `CARGO_PKG_VERSION_{MAJOR,MINOR,PATCH,PRE}` values. It is not a general SemVer parser and should
not acquire range syntax. Schema regex and runtime parser share golden valid/invalid tables.

`RawConfig` gains source-local `requires`; `LoadState` gains validated compatibility records; the
recursive loader receives `SourceClass` and immediate consumer version. Split the current
`drop_top_level_settings` operation so compatibility validation always precedes clearing. Do not
leave a second call path which merges a raw source directly.

Root/drop-in loading must establish the root version before drop-ins are composed. A drop-in with an
explicit different version fails with both source names and versions. Nested and bundled/remote
loaders call the same compatibility function, not copies. The nested-config source allowlist expands
from `version`/`rules` to `version`/`requires`/`rules`; all other root-only fields remain forbidden.

Keep requirement interpolation forbidden. The local interpolation pass currently traverses the
whole raw YAML value; it must inspect and reject placeholders in `requires` before substitution so
a host environment cannot decide whether a config is considered compatible. Remote/bundled sources
already do not interpolate against consumer environment, but receive the same literal-field check.
P4 is a medium loader change and adds no third-party dependency: the narrow stable-triplet parser
uses checked standard-library integer parsing rather than a general SemVer crate. Existing schema
generation and cargo-deny policy remain unchanged.

Generated schema, facts compatibility data, bundled bytes, and docs examples are updated from the
same Rust model/constants. Release notes call out the new required `version` on complete extended
files and show the drop-in exception.

## 6. Tests

Permanent coverage includes:

- accepted stable triplets at zero/component/max boundaries; rejected leading zeros, missing/extra
  components, signs, whitespace, overflow, `v`, operators, ranges, wildcards, prerelease/build text,
  non-string YAML, an empty `requires` mapping, interpolation, tags, aliases/merge ambiguity,
  unknown `requires` keys, and duplicate fields;
- running-version comparisons below/equal/above requirements and prerelease cases, using injected
  build-version data in unit tests rather than rebuilding binaries;
- required/missing/mismatched versions for roots, local extends, bundled sources, remote sources,
  nested roots, and drop-ins;
- composition matrices for `v1->v1`, future simulated `v1->v2` rejection, `v2->v1` acceptance,
  `v2->v2` acceptance, and `v2->v1->v2` rejection, without enabling v2 rule parsing in production;
- proof that every source is checked before merge and, for source classes which support it, before
  its own child extends by placing a forbidden rule and a fetch-counting remote child behind an
  unsatisfied local source requirement;
- diamond/cycle/depth chains, memoization under same and different consumer versions, and exactly
  one diagnostic per canonical source/context;
- root plus sorted drop-ins, an omitted drop-in version inheriting root, an explicit equal version,
  and an explicit mismatch;
- local/bundled/remote requirement retention, effective maximum derivation, source provenance, and
  sanitized diagnostics with userinfo/query/path/environment canaries;
- all production consumers (check, fix, list, explain, baseline, validate-config, LSP, export,
  docs export, benchmark, and public policy test) rejecting the same unsatisfied config before
  evaluation;
- stable config schema and in-crate schema parity, generated facts compatibility data, bundled-byte
  parity, and old-schema rejection where required;
- packaged-binary smoke with a satisfied root, an unsatisfied local base, a satisfied bundled base,
  and a cached remote source served by a deterministic local fixture;
- migration gate that every complete repository config/ruleset declares a version and all existing
  config/scenario suites retain their expected behavior apart from the intentional omission error.

A compatibility smoke runs the last released binary against a v0.20 config containing `requires`.
Because that binary's v1 schema is closed, it must reject the unknown field rather than silently
ignore it. The v0.20 binary then produces the precise minimum-version diagnostic for a requirement
above itself.

## 7. Open questions

Q11 is resolved:

- `requires.alint` lands in v1 before v2;
- it is a minimum stable triplet, not a range;
- every source is checked independently before merge;
- complete extended sources must declare `version`, while drop-in fragments may inherit;
- v2 consumers may extend v1, but v1 consumers may never extend v2;
- the compatibility rule applies at every recursive edge;
- no bypass flag exists.

Deferred to Phase 3: the v1-to-v2 intermediate representation and exact field upgrade rules. Those
decisions cannot weaken or reverse the composition direction fixed here.
