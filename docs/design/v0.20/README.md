# v0.20: Policy authoring loop

Status: Accepted design (2026-10-18); implementation not started.
Decisions: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md)
(DEC-6) and [ADR-0022](../../adr/0022-policy-authoring-loop-contracts.md)
(Phase 1b questions Q7, Q11, and Q14).
Program: [`../general-linter.md`](../general-linter.md), Phase 1b.

v0.20 turns the internal authoring harness and static rule description into a supported policy
feedback loop. It ships three contracts together:

1. a public, versioned `alint test` scenario format and runner;
2. repository-backed `alint explain <rule-id> --trace`, with an explicitly experimental,
   redaction-safe extraction preview;
3. per-source configuration compatibility checks, including `requires.alint`.

The detailed contracts are:

- [`policy-tests.md`](policy-tests.md);
- [`rule-trace.md`](rule-trace.md);
- [`config-requirements.md`](config-requirements.md).

## Why these three belong in one release

The public test runner lets a ruleset author state the behavior they intend. Trace explains why a
configured rule did or did not produce that behavior. Per-source compatibility prevents a ruleset
which uses a new authoring or rule feature from being silently loaded by an older binary. Shipping
only one or two leaves a broken loop: a test failure without trustworthy diagnostics, diagnostics
without a repeatable contract, or a published ruleset with no enforceable minimum binary version.

The release promotes existing machinery rather than introducing parallel conventions. The current
repository has 578 scenario documents, including 207 executable documentation examples and 48
bundled-ruleset scenarios, plus 24 shipped v1 bundled rulesets. Those assets are the migration
corpus and compatibility oracle. The public format remains one scenario per YAML document and the
existing `given` / `when` / `expect` vocabulary remains authoritative.

## Scope

### Public policy tests

`alint test [PATH ...]` discovers and executes policy scenarios in isolated temporary repositories.
With no path, it searches `alint-tests/**/*.yml` and `alint-tests/**/*.yaml`. A document declares
`test_version: 1`, exactly one repository fixture (`given.tree` or `given.fixture`), exactly one
configuration source (`given.config` or `given.config_file`), ordered check/fix steps, and explicit
per-step expectations. The command supports human and versioned JSON reports.

The existing `alint-testkit` crate becomes the published runner library instead of creating a
second facade crate. Its scenario parser, runner, result model, and assertion diagnostics form the
public surface. Internal property-test strategies stay behind a non-default feature. The CLI and
`alint-e2e` consume the same parser and runner; there is no legacy scenario path.

Remote `https://` extends, host environment reads, out-of-repository reads, and undeclared process
or keyring access are denied. Process and keyring access each require both a declaration in the
test document and an explicit host CLI grant. Fixtures and fixes remain in a temporary repository;
config reads and explicitly requested snapshot writes remain confined to the scenario package root.

### Rule trace and extraction preview

Plain `alint explain <rule-id>` remains a static, no-walk operation. Adding `--trace` runs only the
selected effective rule through the same production engine dispatch used by `alint check` and
reports:

- common gates (`level`, `when`, changed scope, Git/analysis-time availability, and
  `expect_matches`);
- configured selectors and considered/matched counts;
- extraction and cardinality counts where the kind extracts structured values;
- assertion outcome, findings, notes, and analysis failures;
- an explicit skipped or completed terminal outcome.

`--preview` adds bounded extraction rows with path, source range, structural pointer, and
provenance. Values use the existing sensitivity policy and are never revealed when their source is
sensitive. Preview output is experimental through the v2 stabilization milestone. Human and JSON
trace output is deterministic and bounded independently of repository size.

The trace recorder is absent on ordinary checks. The non-trace hot path performs no trace
allocation, value rendering, synchronization, or duplicate evaluation.

### Configuration compatibility

Every complete configuration source may declare:

```yaml
version: 1
requires:
  alint: "0.20.0"
```

The value is a minimum stable alint version, represented by exactly three unsigned decimal
components. Each source is checked immediately after guarded parsing and before its contents are
merged or its own `extends` are followed. Requirements are monotonic metadata, not mergeable
policy, and they cannot grant capabilities.

The loader retains and validates each source's `version` before dropping source-local settings.
A v1 consumer may extend only v1 sources. A future v2 consumer may extend v1 or v2 sources; a v1
consumer can never extend a v2 source. This directional rule is fixed now so Phase 3 does not need
to reinterpret old composition.

## Non-goals

v0.20 does not add:

- the Phase 2 relation IR, plan-level tracing, or `key_parity`;
- stable trace/preview JSON compatibility guarantees;
- v2 selectors or constraints;
- a plugin host or plugin test capability;
- arbitrary SemVer ranges in `requires.alint`;
- remote scenario fixtures, remote policy-test includes, or tests that execute in the caller's
  working tree;
- a second multi-case YAML wrapper around the existing scenario document.

## Work items and dependencies

| ID | Deliverable | Depends on | Completion evidence |
|---|---|---|---|
| P1 | Versioned policy-test Rust model, JSON Schema, confinement validator, and diagnostic model | none | schema golden tests; adversarial path/symlink corpus |
| P2 | Published `alint-testkit` runner with fixture/config root separation and deterministic environment | P1 | library integration test executes a packaged fixture without the CLI |
| P3 | `alint test` discovery, human/JSON output, snapshots, and exit mapping | P1, P2 | installed-binary smoke in a fresh downstream repository |
| P4 | Per-source `version` and `requires.alint` validation | none | local, bundled, remote, nested, and drop-in composition matrix |
| P5 | Engine trace model, recorder, shared instrumentation, and completeness audit | none | every canonical kind has pass/fail trace coverage; normal check has zero trace allocations |
| P6 | `explain --trace`, bounded `--preview`, human/JSON output, and schemas | P5 | selected-rule trace/check parity plus redaction and determinism e2e tests |
| P7 | Bundled-ruleset test ownership and existing-corpus migration | P2, P3, P6 | every shipped bundle owns tests; all 578 current scenarios run through the public parser |
| P8 | User docs, release notes, packaging, and generated artifacts | P3, P4, P6, P7 | preflight, package smoke, docs export, and generated checks pass |

P1, P4, and the trace model portion of P5 can proceed independently. P2 precedes the CLI so the
command cannot become a private runner implementation. P5 precedes trace rendering so human and
JSON output project one typed event model. P7 is a migration, not a second implementation.

## Cross-cutting invariants

1. **Production-path parity.** A test step and a traced rule use the same config loader, registry,
   walk, engine dispatch, failure classification, redaction, and fix pipeline as the corresponding
   production command. Test-only reimplementation of rule semantics is forbidden.
2. **Fail loud.** Invalid scenarios, unavailable requested capabilities, incomplete traces,
   analysis failures, intentional preview omission, and unsupported config versions remain
   distinguishable. Preview omission never changes the evaluated assertion and is never presented
   as complete evidence.
3. **Isolation.** With process/keyring capabilities disabled (the default), policy tests do not
   mutate the source package or caller repository except an explicitly requested snapshot file.
   Git hooks and host Git configuration are disabled. Remote config fetches are rejected before
   cache lookup or network access. `--allow-spawn` is explicitly full native-code authority, not
   an OS sandbox guarantee.
4. **Determinism.** Discovery, scenario results, trace stages, paths, related locations, and
   previews have canonical ordering. JSON carries no wall-clock timestamps. Fixture time comes
   from the scenario or its isolated Git history.
5. **Confidentiality.** The W5 sensitivity contract applies before either human or machine output.
   Snapshot updates consume already-redacted records. Terminal text is control-character
   sanitized.
6. **Bounded work.** Preview retention is `O(configured preview limit + trace stages)`, not
   `O(repository values)`. Ordinary checks do not pay trace costs. Existing parser, walk, related
   location, fix-size, and output bounds remain in force.
7. **One versioned source of truth.** YAML test documents are governed by `test_version`; config
   documents remain governed by `version`. The two numbers are independent and each has one
   generated schema.
8. **Positive output coverage.** Tests assert representative content in every human and JSON
   projection, including failures and redaction. Snapshot existence or successful serialization is
   not sufficient.

## Release acceptance

v0.20 is complete only when all of the following are observed:

- A packaged `alint` binary runs `alint test` in a fresh downstream fixture containing inline and
  directory-backed repositories, a local config chain, Git history, check/fix/check steps, a
  sensitive value, and a deliberately failing expectation. The pass and failure exit classes and
  human/JSON details are verified.
- The same smoke proves the runner makes no HTTP request, ignores host secret environment values,
  blocks process and keyring access by default, and accepts each only under the two-key grant.
- The 578-document in-repository corpus parses and executes through the public v1 scenario model.
  Migration may add required version fields and explicit expectations, but may not retain a second
  parser or assertion path.
- Every shipped bundled ruleset has owned pass and fail tests. Catalog-wide tests cover bundled
  filtering/composition and every requirement a bundle actually declares, without duplicating
  tautological cases per bundle. A gate compares the bundle catalog with the owned-test catalog.
- For every canonical kind, a documented pass and fail scenario yields a complete trace. The
  traced selected rule's findings, notes, analysis failures, and relevant skip semantics equal a
  one-rule production check over the same index.
- Preview tests plant secret canaries in dotenv, properties, explicit `sensitive: true` input,
  sensitive process output, config interpolation, and fix evidence; no canary reaches human output,
  JSON, snapshots, or assertion diagnostics.
- Repeated and deliberately shuffled parallel engine runs produce byte-identical trace JSON after
  canonical serialization. A large-repository smoke demonstrates bounded retained preview state.
- `version`/`requires` tests cover root, local extends, bundled extends, remote extends without a
  network dependency, nested configs, drop-ins, mixed source versions, malformed triplets, and an
  older binary requirement. Diagnostics identify the exact source and required/running versions.
- Existing output suites remain unchanged outside specified additive fields and W5 hardening:
  environment-derived dynamic evidence is redacted across check/fix/LSP/test/trace, while static
  explain also redacts explicitly sensitive option/message/condition evidence. Bench and allocation
  gates show no trace recorder allocation or value formatting on ordinary checks.
- Config and policy-test JSON Schemas, CLI help/manpages/completions, facts/discovery data where
  applicable, docs export, changelog, packaging metadata, and the public roadmap regenerate
  cleanly. Full workspace, cross-platform, shell, advisory, and preflight checks pass.

## Implementation handoff

Start with P1 and P4. P1 freezes the serialized test vocabulary before public Rust APIs expose it;
P4 removes the current source-version blind spot before v0.20 rulesets use a minimum requirement.
Then land P2/P3 as one vertical command slice and P5/P6 as one trace slice. Migrate the corpus and
bundle ownership only after both public contracts are executable, so the migration itself is the
end-to-end proof rather than hand-edited documentation.
