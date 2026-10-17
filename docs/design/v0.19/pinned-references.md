# Design doc: `pinned_references`

Status: Implemented (2026-10-17).
Decisions: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md)
(Phase 0/1a gates). No new architectural decision.
Demand evidence: H1 in [`../rule-coverage-gaps.md`](../rule-coverage-gaps.md), the two holes in
`ci/github-actions@v1`, and the March 2025 `tj-actions/changed-files` incident cited there.
Part of [v0.19](README.md) work item W9.

## 1. Problem

A repository can lock its package manifests and still execute mutable third-party code before the
build starts. GitHub Action tags, container tags, CircleCI development orb references, pre-commit
tags, and remote Dockerfile downloads all let a publisher replace what the same commit executes.
The shipped `ci/github-actions@v1` ruleset checks `steps[*].uses` only in workflow jobs. It does not
cover reusable workflows (`jobs.*.uses`) or composite actions (`action.yml` `runs.steps[*].uses`),
and there is no equivalent kind for the other CI/container formats.

The check must be semantic enough to distinguish the reference-bearing fields from arbitrary YAML
strings and must point at the exact offending scalar. A repository-wide regex would confuse local
Actions, stage names, output image names, comments, examples, and unrelated `image` keys with
executable dependencies.

## 2. Surface area

One check-only kind:

```yaml
- id: immutable-build-inputs
  kind: pinned_references
  paths:
    - .github/workflows/*.{yml,yaml}
    - action.{yml,yaml}
    - .pre-commit-config.yaml
    - .circleci/config.yml
    - .gitlab-ci.yml
    - cloudbuild.yaml
    - Dockerfile
  level: error
```

Options:

| Field | Type | Default | Meaning |
|---|---|---|---|
| `format` | `auto`, `github_actions`, `pre_commit`, `circleci`, `gitlab_ci`, `cloud_build`, or `dockerfile` | `auto` | Parser and selector set. `auto` uses the repository-relative path. |

`paths` is required. Common fields `scope_filter`, `when`, `message`, `policy_url`, and
`expect_matches` retain their normal meaning. `git_tracked_only`, change selectors, nested use, and
all `fix` operations are rejected by `KindMeta`/the builder. Explicit `format` exists for nonstandard
filenames; it never changes which fields within the document are dependency references.

`auto` recognizes:

| Format | Paths |
|---|---|
| GitHub Actions | `.github/workflows/*.yml`, `.github/workflows/*.yaml`, `action.yml`, `action.yaml` |
| pre-commit | `.pre-commit-config.yaml`, `.pre-commit-config.yml` |
| CircleCI | `.circleci/config.yml`, `.circleci/config.yaml` |
| GitLab CI | `.gitlab-ci.yml`, `.gitlab-ci.yaml` |
| Cloud Build | `cloudbuild.yml`, `cloudbuild.yaml` |
| Dockerfile | a basename equal to `Dockerfile` or starting with `Dockerfile.` |

An auto-selected path with no recognized format is an evaluation analysis failure, not a silent
pass. Malformed/non-UTF-8 YAML or Dockerfile input is a resource-parse analysis failure. YAML goes
through the existing flow-depth and alias-expansion guards before the span-preserving parser.

### References and accepted pins

| Format | Selected references | Accepted immutable form | Exemptions |
|---|---|---|---|
| GitHub Actions | `jobs.*.steps[*].uses`, `jobs.*.uses`, `runs.steps[*].uses`, `jobs.*.container.image`, `jobs.*.services.*.image` | external action/workflow: `@` plus exactly 40 or 64 hexadecimal characters; container image: `@sha256:` plus 64 lowercase hexadecimal characters | `uses: ./…`; `scratch` image |
| pre-commit | each external `repos[*]` pair `repo`/`rev` | `rev` is exactly 40 or 64 hexadecimal characters | `repo: local`, `repo: meta` |
| CircleCI | `orbs.*`; `jobs.*.docker[*].image`; `executors.*.docker[*].image` | orb: an exact numeric `major.minor.patch` version, optionally with a SemVer prerelease/build suffix; image: `@sha256:` plus 64 lowercase hexadecimal characters | `scratch` image |
| GitLab CI | top-level and `default.image`; top-level/default/job `services`; each job's `image` | image: `@sha256:` plus 64 lowercase hexadecimal characters | `scratch` image |
| Cloud Build | `steps[*].name` | image: `@sha256:` plus 64 lowercase hexadecimal characters | `scratch` image |
| Dockerfile | every stage's `FROM` image; every HTTP(S) source of `ADD` | image: `@sha256:` plus 64 lowercase hexadecimal characters; remote `ADD`: a `--checksum=sha256:` option plus 64 lowercase hexadecimal characters | `FROM scratch` |

Mapping forms such as GitLab `image: { name: … }` and `services: [{ name: … }]` are included.
GitHub `docker://…` action references use the image rule. Tags may remain before a digest for human
readability. Template/variable references are not immutable evidence and therefore fire.

CircleCI registry orb releases are immutable once published, so an exact SemVer is sufficient;
`volatile`, development aliases, ranges, and partial versions fire. This differs deliberately from
Git commits, where only a full object ID is accepted.

Dockerfile parsing joins escaped physical lines, honors the leading `# escape=` directive, ignores
comments, accepts shell and JSON `ADD` forms, and handles options before operands. `ADD` only checks
HTTP(S) operands; local files, directories, and heredocs are not remote fetches. No shell instruction
(`RUN`, shell expansion, command substitution) is interpreted.

Each failing reference produces one finding at the reference scalar/instruction with a stable
baseline key derived from the format, semantic selector, and reference value. A missing pre-commit
`rev` is located at its `repo` scalar. A custom `message` changes display text but not finding
cardinality, locations, or baseline identity.

## 3. Semantics and false-positive analysis

The rule answers one question: **does this source file itself identify the exact external artifact
that the supported consumer will execute or download?** It does not resolve tags, consult a
registry, infer variable values, or trust lock data stored elsewhere. Those operations require
network or runtime context and would make the same commit time-dependent.

YAML is parsed into a small ordered, spanned tree. Aliases resolve within that tree and retain the
use site's location. Mapping lookups follow YAML merge keys and explicit-key precedence so an
unused value in an overridden anchor does not fire. Duplicate explicit keys use the last value,
matching the existing YAML value pipeline; malformed structure that cannot contain a selected
reference is ignored rather than guessed.

Expected false positives and their migrations:

- A centrally controlled mutable tag is still mutable. Pin it to its digest/SHA or exclude that
  path with `scope_filter`.
- A variable whose value is fixed by an external CI setting cannot be proved from repository state.
  Put the immutable value in the checked file or exclude the file.
- A private CircleCI orb that is immutable under an organization-specific naming convention must
  use an exact SemVer or be excluded.
- A nonstandard file intentionally matched by a broad `paths` glob must set the correct explicit
  `format`; unsupported files must not be mixed into this rule.

False negatives deliberately accepted for this increment:

- GitLab `include` expansion, Docker Compose, Kubernetes, devcontainers, and arbitrary shell
  downloads are not selected. They need distinct consumer semantics rather than key-name guesses.
- Dockerfile variables are reported as unpinned; the rule does not evaluate `ARG`, because build
  arguments can be overridden by callers.
- Registry authenticity, digest existence, compromised immutable artifacts, and dependency
  provenance are outside this syntactic immutability check.

## 4. Implementation notes

Module: `crates/alint-rules/src/pinned_references.rs`; registration and option schema in
`crates/alint-rules/src/lib.rs`; generated schema/facts/category/docs outputs follow the normal W1
parity gates.

The rule is `DispatchClass::PerFile { full_index: false }`, requires `paths`, supports full
`scope_filter`, reads `ContentAccess::WholeFile`, and has no external capabilities, generic
`Format` entries, dependencies, or fix operations. Its consumer-specific YAML/Dockerfile parser
is linear in input bytes plus selected YAML nodes. The YAML parser uses the already-workspace-pinned
`saphyr-parser`; no dependency or network access is added.

The pin recognizers are byte predicates, not regexes in hot loops:

- Git object ID: exactly 40 or 64 ASCII hexadecimal bytes.
- OCI image digest/checksum: literal `sha256:` plus exactly 64 lowercase ASCII hexadecimal bytes.
- exact CircleCI orb: three nonempty decimal components, with an optional syntactically bounded
  SemVer prerelease/build suffix.

Findings never echo unrelated document values. Reference values are not classified as secret, but
control characters still pass through the shared output sanitizer.

## 5. Tests

Unit tests cover:

- every selector in the table, both accepted and rejected forms;
- GitHub local actions, reusable workflows, composite actions, Docker actions, containers, and
  services;
- YAML quoted/plain scalars, mapping forms, aliases/merge overrides, UTF-8 before a finding,
  malformed input, depth/alias limits, and source line/column ranges;
- pre-commit missing `rev` and local/meta repositories;
- exact, prerelease, partial, range, development, and volatile CircleCI orb versions;
- Docker image ports/tags/digests and `scratch`;
- Dockerfile multi-stage `FROM`, options, aliases, continuations with both escape characters,
  remote/local `ADD`, JSON form, checksum validity, comments, and variables;
- custom-message cardinality and stable baseline keys.

Declarative end-to-end scenarios provide one firing and two silent cases. The firing scenario
mixes all six formats and checks exact per-reference locations; the documented silent scenario
exercises all exemptions and valid pin forms, while a separate explicit-format scenario covers a
nonstandard filename.

Acceptance gates:

1. `cargo test -p alint-rules pinned_references` and all three declarative scenarios pass.
2. Generated schema, facts, category, rule documentation, and all-kinds fixtures are current.
3. The former `ci/github-actions@v1` `steps[*].uses` rule is replaced by equivalent workflow
   coverage in the new bundled migration; its id, level, and existing finding fingerprint remain
   stable, while reusable workflows and composite actions gain coverage.
4. `cargo test --workspace`, preflight, shell contracts, examples, packaging, documentation, and
   deterministic benchmark smoke gates pass.
5. No network call occurs and no fix is advertised.

## 6. Coverage plan

| Layer | Evidence |
|---|---|
| Unit | parsers, selectors, pin predicates, aliases/merges, location/range, failure disposition |
| E2E firing | one scenario reports every supported mutable reference and missing pre-commit rev |
| E2E silent | immutable references, local Actions/repos, `scratch`, and local `ADD` remain silent |
| Compatibility | migrated `ci/github-actions@v1` keeps its rule id/level and gains reusable/composite coverage |
| Generation | W1 schema/facts/docs/category/all-kinds parity gates include the 102nd canonical kind |
| Performance | 1k/10k pinned-reference parser scaling cells exercised by benchmark smoke, plus the existing deterministic gate |

No permanent test asserts prose formatting or implementation wiring.

## 7. Open questions

None. The format set, immutable forms, no-network boundary, mapping-form support, Dockerfile
subset, CircleCI orb rule, failure behavior, and migration are decided above.
