---
status: accepted
date: 2026-10-18
decision-makers: asamarts
---

# 0022. Policy authoring loop contracts

## Status

Accepted (2026-10-18). This records the Phase 1b outcomes for ADR-0019 decision 6 and
questions Q7, Q11, and Q14.

## Context

ADR-0019 schedules a public policy-test runner, author-facing diagnostics, and configuration
compatibility metadata for v0.20, but deliberately leaves their exact contracts to the Phase 1b
design. Those contracts cross existing architectural boundaries:

- making the internal scenario harness public creates a versioned input and output format plus a
  host-capability boundary;
- evaluating `explain` changes ADR-0009's static-only command unless static description and
  repository-backed diagnostics remain distinct;
- validating only the final merged config loses each extended source's language version and cannot
  safely compose a future `version: 2` source;
- reusable policy sources need a minimum binary version without gaining trust or capabilities.

These choices must be fixed before public Rust types, schemas, or config composition behavior make
them accidental API.

## Decision

We will ship the v0.20 authoring loop under three coordinated contracts:

1. **Policy tests.** `alint test` promotes the existing one-scenario-per-YAML-document testkit
   model. Every document declares its independent `test_version`; the existing `alint-testkit`
   crate becomes the published runner and remains the single parser and execution path. Repository
   fixtures execute in temporary repositories, while fixture/config/snapshot package paths remain
   confined to the scenario package. Remote config extends are disabled. Process and host-keyring
   access each require both a document declaration and an explicit CLI grant; the process grant is
   full native-code authority, not a sandbox.
2. **Rule trace.** Plain `alint explain <rule-id>` remains a static, no-walk operation. The explicit
   `--trace` mode evaluates only the selected effective rule through production engine dispatch and
   projects typed, completeness-audited stages. `--preview` adds deterministic bounded extraction
   rows after producer-side redaction. Static explain JSON remains stable and distinct; trace JSON
   uses a separate experimental discriminator and schema until the `version: 2` stabilization gate.
3. **Config compatibility.** Every complete config source validates its own language version before
   merge. A v1 source may consume only v1 sources; a future v2 source may consume v1 or v2 sources,
   and this rule applies at every immediate composition edge. Complete extended sources must state
   `version`; `.alint.d` fragments may inherit their root's version. The optional closed
   `requires: { alint: "MAJOR.MINOR.PATCH" }` mapping is a source-local minimum stable release,
   checked before that source is merged or its supported child extends are followed. It cannot
   grant capabilities and has no bypass.

The implementation-level vocabulary, confinement rules, ordering, redaction, schemas, diagnostics,
and acceptance tests are normative in [`docs/design/v0.20/`](../design/v0.20/README.md).

## Consequences

- Ruleset authors get one repeatable path from fixture assertion to production-parity diagnosis and
  an enforceable minimum binary version.
- Public policy-test and trace schemas are separate from config `version`; changing one does not
  reinterpret the others.
- Existing root configs continue to work without `requires`, but a reusable complete extended file
  that omitted `version` must add it during the pre-1.0 migration.
- Trace cannot silently become a second evaluator, and ordinary checks do not pay trace allocation
  or rendering costs.
- A committed policy test cannot activate host code or keyring access by itself. An explicitly
  granted spawning test can exercise the invoking user's full authority, which users and CI must
  treat accordingly.
- `requires.alint` rejects too-old binaries rather than attempting best-effort evaluation. Older
  binaries remain fail-closed because the v1 schema rejects the unknown field.

## Considered Options

- **Add a new multi-case test wrapper or facade crate:** rejected because it would fork the mature
  scenario parser and runner.
- **Make all explain calls evaluate or add trace fields to stable explain JSON:** rejected because
  static explanation must stay cheap and compatible, while trace is still evolving.
- **Run every rule and filter the report for trace:** rejected because unrelated expensive,
  spawning, or failing rules would execute.
- **Validate only the final merged config version:** rejected because source-local language and
  requirement metadata are discarded during composition.
- **Accept SemVer ranges or an ignore-requirements flag:** rejected because ranges add unnecessary
  interpretation and bypassing an explicit minimum can produce false policy results.

## More Information

- [`v0.20/README.md`](../design/v0.20/README.md): scope, work items, invariants, and release gates.
- [`policy-tests.md`](../design/v0.20/policy-tests.md): public scenario and runner contract.
- [`rule-trace.md`](../design/v0.20/rule-trace.md): selected-rule trace and preview contract.
- [`config-requirements.md`](../design/v0.20/config-requirements.md): per-source compatibility
  contract.
- [ADR-0019](0019-general-linter-program-and-plugin-architecture.md): parent program decision.
- [ADR-0002](0002-config-language-is-yaml.md): YAML composition model extended here.
- [ADR-0004](0004-extends-trust-boundary-and-path-confinement.md): trust/confinement boundary extended
  to public policy tests.
- [ADR-0009](0009-rule-discovery-cli-config-vs-catalog.md): static explain behavior retained here.
