# Design doc: evidence redaction and the leak gate

Status: Implemented in v0.19 development (2026-10-16).
Decisions: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md) (DEC-3), [ADR-0012](../../adr/0012-output-completeness-testing.md)
Demand evidence: [`general-linter.md` section 4.12](../general-linter.md#412-current-fail-open-and-leak-sites)

## 1. Problem

The string-only extraction layer feeds repository-controlled values into relation messages, note bodies, baseline discriminators, and fixer summaries. A value read from a dotenv or Java properties file can therefore be copied into terminal output, any machine report, SARIF replacement text, or the advisory `message` in `.alint-baseline`. Those formats commonly hold credentials and tokens. alint is not a secret scanner, but it must not become a secret exfiltration path.

Hashing a raw low-entropy value is not sufficient. An attacker who can read a committed baseline can enumerate likely values and compare unsalted fingerprints. Redacted values must therefore be absent from both serialized text and fingerprint input, not merely hidden behind another deterministic hash of the value.

## 2. Surface area

Every rule accepts one new common field:

```yaml
- id: private-policy
  kind: structured_path_equals
  paths: private.json
  path: $.token
  equals: expected
  sensitive: true
  level: error
```

`sensitive` is optional and defaults to `false`. It is also accepted in nested `require:` rules. It is safe in local, inherited, remote, and nested policy because it can only remove report detail; it grants no read, execution, or remediation capability.

Two extraction formats imply sensitivity without this option:

- `extract: { dotenv: ... }`
- `extract: { properties: ... }`

The rule model gains internal sensitivity metadata. Public JSON and agent report schemas do not expose that metadata. Redacted evidence is rendered as `<redacted:XXXXXXXXXXXX>`, where the twelve lowercase hexadecimal digits are a SHA-256 digest of a domain-separated, non-secret structural locator such as rule id, source role, source path, query, and finding category. The digest never includes the evidence bytes. It is a stable correlation hint, not a password hash.

## 3. Semantics

### Default extraction redaction

The shared `Extract` value identifies whether its format is secret-bearing. `cross_file`, `registry_paths_resolve`, `file_graph`, and manifest-backed scope preserve that fact through evaluation.

When a dotenv/properties value contributes to a finding:

- fixed explanatory text and source/configured paths remain visible;
- each rendered value, normalized derivative, or value-derived path in the message is replaced by the locator marker;
- a parse failure from the secret-bearing input keeps its typed failure kind and source location but replaces parser-controlled detail with the marker;
- informational skipped-value notes follow the same rule;
- custom `message:` text is retained when it does not interpolate evidence, but the finding is still marked sensitive for fingerprint and remediation handling.

The finding's primary repository location is not evidence and remains navigable. A configured source/target path therefore remains visible even if its spelling happens to equal an extracted value.

### Explicit `sensitive: true`

The engine applies explicit sensitivity after nested wrapping and result assembly. Because arbitrary existing kinds flatten fixed prose and dynamic data into one string, selectively identifying evidence would be unsafe. The complete message of every violation, note, and analysis failure from that rule is replaced with `sensitive evidence <redacted:...>`. Fix status detail is replaced in the same way. Rule id, severity, policy URL, and primary location remain visible.

Nested sensitivity applies to that nested rule's findings before the parent adopts them. A sensitive parent applies the stronger whole-message rule to all findings it owns.

### Baselines and fingerprints

A sensitive violation never contributes raw evidence through `message`, `baseline_key`, or offending-line bytes:

- default-sensitive producers replace value-bearing baseline keys with a structural key containing only non-secret role/path/query/category fields;
- explicit sensitivity uses a constant structural discriminator within `(rule id, primary path)`;
- the fingerprint implementation ignores line content and message for every sensitive violation;
- multiple sensitive findings that consequently share an identity collapse through the baseline's existing `count`, so they remain suppressible without revealing values;
- `.alint-baseline` stores only the already-redacted advisory message.

This excludes low-entropy values from fingerprint input rather than attempting to protect them with a public salt. Non-sensitive findings retain byte-identical v1 fingerprints.

### Fixes and outputs

All eight check formats, human/JSON fix reports, baseline files, and LSP diagnostics consume the already-redacted finding message. Formatters do not implement their own redaction policy.

Actual `alint fix` execution may read and write the original value: remediation cannot propagate a value it never receives. Reporting remains safe:

- fix outcome detail for a sensitive finding is replaced before serialization;
- check-side `proposed_edits` are not attached to sensitive findings, and composed edits for a
  file with any sensitive finding are omitted, because SARIF replacement text can contain
  unchanged secret bytes;
- LSP diagnostics remain visible, but sensitive findings do not offer code actions carrying
  replacement bytes;
- fixability, applicability, pass/fail, and whether `alint fix` applies the fix do not change.

Redaction happens before baseline generation and before any public report is returned. A caller using `alint-core` therefore cannot accidentally serialize the raw flattened message from an engine result.

## 4. False-positive surface

The failure mode to avoid is hiding ordinary diagnostics that contain no secret. Mitigations:

- implicit redaction is format-based, not filename-based: only dotenv/properties extract adapters opt in;
- JSON, YAML, TOML, XML, INI, HCL, regex, lines, and whole-file extraction remain visible unless `sensitive: true` is set;
- configured paths and fixed prose stay visible under implicit redaction;
- explicit sensitivity deliberately prefers over-redaction because the user opted into the security boundary;
- redaction does not alter comparison, selection, fix execution, severity, or exit status;
- markers derive only from stable structural context, so they neither reveal equality between guessed secret values nor vary with process time;
- empty strings are never searched/replaced, avoiding accidental insertion between every character.

A marker may repeat for several values selected by one structural locator. That loss of diagnostic distinction is intentional; the baseline `count` preserves multiplicity without publishing evidence.

## 5. Implementation notes

`alint-core` owns marker generation, sensitivity metadata, whole-message redaction, sensitive fingerprint behavior, and proposed-edit exclusion. `Extract::redacts_by_default()` is the single format classification; consumers must not maintain separate format lists.

`alint-rules` applies implicit redaction where extracted values become diagnostics or baseline keys. Secret-bearing parse errors are converted to stable value-free text at the consumer boundary. `alint-dsl` and the schema generator add the common `sensitive` field to top-level and nested rules. No new dependency is required; `sha2` is already used by canonical fingerprints.

The hot success path adds one enum comparison per relevant extraction. Digesting and string replacement occur only when a finding is emitted. Non-sensitive rules allocate nothing new. Constitution invariants 1 (determinism), 7 (bounded analysis), 11 (repository text cannot inject output), and 12 (positive output completeness) apply.

## 6. Tests

Required gates:

- unit tests pin the marker syntax, prove the digest input excludes the raw value, and prove sensitive fingerprints never read message, baseline-key evidence, or offending-line bytes;
- schema/runtime tests accept `sensitive: true` on every canonical kind and recursively nested rules and reject non-boolean values;
- focused rule tests cover source, target, set, skipped-note, parse-failure, registry-entry, and graph-edge paths for both dotenv and properties extraction;
- fix tests prove real propagation can still occur while human/JSON status detail, SARIF proposed
  edits (including a non-sensitive sibling edit on the same file), and LSP code actions contain no
  raw value;
- a scenario plants unique sentinels in `.env`, `.properties`, and an explicitly sensitive rule,
  then asserts none appears in human, JSON, SARIF, GitHub, GitLab, Markdown, JUnit, agent,
  human/JSON fix output, LSP diagnostics, or `.alint-baseline`;
- the ADR-0012 positive-completeness gate proves each format carries the marker rather than silently dropping the finding;
- existing non-sensitive fingerprint pins remain unchanged.

The leak gate scans complete raw output bytes, not parsed message fields, so a leak through a fingerprint input, proposed edit, status detail, escaping variant, or baseline advisory field still fails.

## 7. Open questions

All decisions are resolved for W5.

- **Digest raw values or structural locators?** Structural locators. A raw-value digest preserves an offline dictionary oracle for low-entropy credentials.
- **Salt sensitive fingerprints?** No. Sensitive values are excluded from fingerprint input; the existing count model handles same-path collisions without a new baseline schema or salt lifecycle.
- **What does `sensitive: true` redact?** The complete flattened message and fix detail. Existing kinds do not retain trustworthy evidence spans, so partial redaction would be fail-open.
- **Do sensitive fixes still run?** Yes. Execution semantics are unchanged, but public proposed edits and outcome detail are suppressed/redacted.
- **Are primary paths hidden?** No. The primary source location remains navigable. Secondary
  locations are omitted from sensitive findings because relation rules may derive those paths
  directly from extracted values and the finding model cannot distinguish those values from
  independently known repository locations.
