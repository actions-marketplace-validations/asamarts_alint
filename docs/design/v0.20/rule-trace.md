# Design doc: rule trace and evidence preview

Status: Implemented (2026-10-18); P5 engine model/recorder and P6 CLI projection complete.
Decisions: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md)
questions Q7/Q14 and [ADR-0022](../../adr/0022-policy-authoring-loop-contracts.md).
Demand evidence:
[`../general-linter.md` section 5.8](../general-linter.md#58-policy-authoring-testing-and-debugging)
and Phase 1b.

## 1. Problem

`alint explain <rule-id>` describes the effective static rule but does not evaluate it. It cannot
answer why a configured rule stayed silent, which files a selector admitted, how many values an
extractor produced, which cardinality gate failed, or where an assertion got its evidence. Authors
must infer engine behavior from config and final findings. This is particularly weak for a false
negative: there is no finding to inspect.

A useful trace must not become a second evaluator. It must execute the selected effective rule
through production dispatch, expose enough typed stages to explain pass/fail/skip behavior, preserve
analysis failures, redact sensitive evidence, remain deterministic under parallel evaluation, and
retain bounded state on large repositories. It must also avoid taxing ordinary `check`: no trace
allocation, formatting, lock, or duplicate extraction may occur when tracing is off.

## 2. Surface area

### 2.1 Command

The existing command gains these rule-local options:

```text
alint explain <RULE_ID>
  [--trace]
  [--preview]
  [--preview-limit N]
  [--changed]
  [--base REF]
```

Global `--format human|json`, `--as-of`, `--config`, `--no-gitignore`, color, width, and docs options
keep their existing meanings where applicable. `--base` implies `--changed`. `--preview` requires
`--trace`; `--preview-limit` requires `--preview`, defaults to 20, and accepts 1 through 100.
Unsupported output formats remain errors.

Without `--trace`, the command keeps the existing static load/no-walk path: it resolves only
analysis time needed for the description and does not evaluate or spawn a configured rule. Output
is byte-for-byte unchanged for non-sensitive configs. v0.20's shared explain payload additionally
redacts environment-derived and explicitly sensitive option/message/condition evidence in both
plain and trace modes; that security correction is the only permitted plain-explain output change.

With `--trace`, the command walks the configured repository and evaluates only the selected
effective rule. The help text states that this has the same read/process behavior as checking that
rule; in particular a trusted local `command` rule and configured custom facts can execute because
trace follows the production check path. No fixer is invoked and no fix is applied. Remote
composition retains the production spawn and trust gates.

Trace is diagnostic rather than enforcement. Exit 0 means the selected rule reached a represented
terminal outcome, including satisfied, violated, skipped, or an ordinary typed analysis failure.
Invalid arguments, config, or rule id use exit 2; runner/engine/invariant failures use exit 3. There
is no exit 1 for a policy violation because `explain` does not decide repository compliance.

### 2.2 Static rule plus execution trace

Trace output contains the existing static rule description followed by one execution record. The
record is a typed projection of these stage classes:

| Stage | Required content |
|---|---|
| `gate` | level, `when`, changed scope, Git availability, analysis-time availability, capability gates, and `expect_matches` when applicable |
| `selection` | configured selector name, normalized expression/summary, considered count, matched count, and skip/failure state |
| `extraction` | extractor/source name, input count, extracted row/value count, non-string/unsupported/parse outcomes, and optional bounded preview |
| `cardinality` | assertion name, expected relation/bound, actual count(s), and satisfied/violated/not-evaluated state |
| `assertion` | policy predicate, satisfied/violated/not-evaluated state, finding count, note count, and analysis-failure count |
| `nested` | aggregate child-instance counts/statuses plus bounded child details for a nested-rule kind |
| `outcome` | exactly one terminal status and the selected rule's complete result counts |

Common gates and terminal outcome are engine-owned. Kind-specific selection, extraction,
cardinality, assertion, and nested stages are emitted through shared trace helpers in the same code
which computes the production result. A trace is invalid if its kind's declared required stages do
not appear, if it has no terminal outcome, or if a stage claims a completed assertion after an
unrepresented analysis failure.

Typed reason codes accompany human detail, for example `level_off`, `when_false`,
`changed_scope_empty`, `git_unavailable`, `selector_no_match`, `parse_failed`,
`cardinality_mismatch`, and `assertion_satisfied`. Free-form detail is supplementary and sanitized;
machine consumers do not need to parse it. Reason codes remain part of the experimental trace
contract and may change with its schema before v2 stabilization.

An off rule has `gate(level)=skipped` and `outcome=skipped`. A false `when` has an evaluated gate and
the same explicit terminal skip. A `when` evaluation error has `gate(when)=analysis_failure` and a
terminal `analysis_failure`; it does not masquerade as false. An empty selector is a completed
selection with zero matches, followed by the production `expect_matches` assertion where enabled.
Unavailable Git or analysis time follows the existing W2/W3 typed failure contract.

### 2.3 Trace metadata in the capability catalog

`KindMeta` gains mandatory trace metadata for every canonical kind. It declares the allowed
kind-owned stage classes and the subset required whenever policy assertion is reached, whether it
can emit extracted rows, and whether it can create nested child evaluations. Aliases resolve the
canonical record exactly, as they do for every other capability.

The metadata is the audit inventory, not a second execution table. It does not contain selector
implementations, assertion logic, or strings copied from rule code. Shared engine stages are
implicit. Conditional kind stages are allowed by metadata and exercised by option-matrix tests;
unconditional stages are marked required. Generated facts and `alint rules show` expose this
trace-stage inventory so docs, plugin contracts, and completeness tests do not grow a hand-maintained
list.

At trace completion the recorder compares observed stage classes with the canonical metadata.
An undeclared stage or required stage missing after assertion was reached is an engine invariant
failure with the kind and stage, not an apparently complete sparse trace. A terminal common-gate
skip exempts kind-owned stages because evaluation never reached them. Ordinary checks do not run
this audit.

### 2.4 Evidence preview

`--trace` reports aggregate counts without retaining per-value or per-child details. Adding
`--preview` permits bounded structured-extraction and nested-child rows such as:

```text
extract version declarations: 9 inputs, 9 values
  Cargo.toml:3:11  /package/version  string  "0.20.0"  (toml)
  package.json:2:14 /version         string  "0.20.0"  (json)
  ... 7 omitted
```

A preview row can carry:

- repository-relative source path and optional half-open source range;
- structural pointer using JSON Pointer escaping, or a stable selector-local row id when no
  structural path exists;
- source/provenance class (`literal`, `structured`, `capture`, `derived`, or `command`);
- value type and display value;
- `redacted` and `value_truncated` flags.

Repository source paths are retained. Paths computed from sensitive extracted evidence are replaced
with a structural redacted marker, matching the W5 related-location policy. Sensitive values are
replaced before they enter the recorder; no raw value, reversible hash, prefix, length, or equality
class is retained. Dotenv and Java-properties values are sensitive by default, and explicit
`sensitive: true` applies to every stage, nested child, failure detail, command-derived row, and
kind-specific option/message/condition evidence of the rule. Config interpolation returns
field-level sensitive provenance; evidence-bearing uses make the resulting finding/failure and
static/trace payload redacted across all consumers. Configured rule ids and repository paths remain
visible structural identity under the W5 contract and must not contain secrets.

A non-sensitive displayed value is at most 256 UTF-8 bytes, cut at a character boundary and marked
`value_truncated: true`. Human output escapes control characters; JSON carries the sanitized string.
This bound applies to the display only. The production assertion still uses the complete in-memory
value under its existing parser and input limits.

The recorder keeps the lexicographically first N rows by normalized path bytes, source range,
structural pointer, and stable occurrence ordinal. A bounded max-heap per extraction stage allows
parallel producers to find that deterministic set while retaining `O(N)` rows. Exact total,
shown, omitted, redacted, and value-truncated counts accompany every preview. Omission is always
visible and does not imply analysis truncation; a separate production note/failure continues to
represent any evaluator limit.

A nested kind records exact child totals and aggregate outcomes, but retains details for only the
same N deterministically first child instances. Relation rows remain an experimental preview
surface. v0.20 does not define the Phase 2 relation IR or present preview records as a stable
relation API.

### 2.5 JSON envelope

Plain explain JSON keeps its existing `schema_version: 1`, `kind: "rule"` envelope unchanged. Trace
uses a separate document:

```json
{
  "schema_version": 1,
  "kind": "rule-trace",
  "stability": "experimental",
  "rule": {},
  "execution": {
    "changed": false,
    "base": null,
    "preview": false,
    "preview_limit": 20
  },
  "stages": [],
  "result": {
    "status": "satisfied",
    "violations": [],
    "analysis_failures": [],
    "notes": []
  }
}
```

`rule` is the typed, redacted payload used by the existing static explain renderer, without its
outer envelope. It therefore cannot drift to a second definition of capabilities, selectors, fixes,
options, messages, redaction, or analysis time.

The trace schema is committed at `schemas/experimental/rule-trace.json`. `stability` is always
`experimental` through the v2 stabilization gate. Fields, stage detail, and reason codes may change
before that gate. An incompatible change increments `schema_version`, replaces the experimental
schema, and is called out in the changelog; pre-v2 versions need no overlap window. The top-level
`kind` prevents a consumer from confusing the document with stable static explain JSON. No trace
field is added conditionally to `kind: "rule"`.

JSON carries no wall-clock time, duration, thread id, absolute checkout path, temporary path, or
unordered map. Dates appear only when they are configured/derived analysis inputs already shown by
static explain. All repository paths use `/`.

### 2.6 Human presentation

Human output keeps the static block first, then prints `trace:` with indented stages in execution
order. Each stage has a status word, exact aggregate counts where relevant, and concise reason.
Passing details collapse to one line; failed/skipped gates, analysis failures, and preview rows are
expanded. A terminal line distinguishes `satisfied`, `violated`, `skipped`, and
`analysis failure`.

Width wrapping uses the shared output renderer and terminal sanitization. Color is status-only and
never the sole carrier of meaning. `--no-color` and non-TTY behavior match other commands. Preview
omission/redaction markers are literal words, not dim-only annotations.

## 3. Semantics

### 3.1 Production-path parity

The engine exposes one selected-rule execution entry point used by trace. It accepts the same root,
full index, changed set/base result, facts, variables, registry, analysis time, and effective
`RuleEntry` as `Engine::run`. It invokes the same dispatch branch, per-file evaluator, common gates,
nested-rule builder, `expect_matches` postcondition, failure conversion, note collection, redaction,
and result canonicalization. It does not call a trace-specific `evaluate` implementation. Analysis
time is resolved once and the same resolution feeds both the static rule payload and evaluation.

A trace result must equal the selected rule's unbaselined production-engine result over the same
inputs after filtering the full report by rule id. Equality covers violations, ranges, related
locations, analysis failures, notes, level, policy URL, and ordering. Skip reasons are trace-only
because a production report may omit a skipped result.

A trace executes the selected configured id, not every rule of the same kind and not a catalog
example. Template, extends, nested-config, trust-demotion, common-field, and alias resolution have
already produced the effective entry. Unknown ids fail before the walk.

### 3.2 Recorder and ordering

The execution context carries an optional trace handle. `None` is the ordinary check state. The
normal path performs one predictable option check only at existing stage boundaries; checks for
high-cardinality extraction are hoisted outside per-file/value loops. It does not allocate a
recorder, construct a value display, increment trace counters, take a lock, or retain an extra row.
Shared extraction helpers emit only after obtaining the value already required by production
evaluation.

Trace mode creates per-work-item buffers for parallel file evaluation and aggregate counters for
bounded stages. It does not serialize worker execution through a global output lock. The engine
merges buffers after evaluation and sorts by:

1. configured stage ordinal and nested parent ordinal;
2. stage class and stable stage name;
3. normalized repository path bytes;
4. start/end range;
5. structural pointer;
6. stable source occurrence ordinal.

Thread scheduling and hash-map iteration therefore cannot affect output. Stage identifiers are
static or config-derived, never file/value-derived. A stage which aggregates many targets stores
checked `u64` counters plus its bounded preview heap, not one event per file/value; counter overflow
is an engine invariant failure. Config size bounds the number of distinct stage records; repository
size does not.

### 3.3 Redaction and failures

Redaction occurs at the producer boundary before trace storage. The recorder API accepts a
sensitivity/provenance token with every display candidate and exposes no convenience method which
defaults unknown provenance to public. Unknown provenance is redacted. The config loader likewise
retains which effective field fragments came from environment interpolation, including through
merge/template expansion. Evidence-bearing use applies the existing explicit-sensitive result
policy before check/fix/LSP/test/trace output; static options/conditions/messages and unstructured
config error detail remove the interpolated bytes. Structural rule ids and repository paths keep
their existing visible identity. Assertion and failure text passes through the existing result
redactor before the trace copies it.

A parser error, unavailable resource, failed command spawn/wait, failed runtime condition, or nested
build error produces the same typed `AnalysisFailure` as check and a matching trace stage. A
nonzero command exit which is the rule's policy predicate remains a violation. Preview truncation is
not an analysis failure because exact counts and the full assertion were computed; production
bounded-analysis truncation remains the existing typed note and is shown separately.

Every execution has exactly one terminal outcome. `satisfied` means no policy violation and no
analysis failure after a completed assertion. `violated` means one or more policy violations and no
analysis failure. `analysis_failure` means at least one analysis failure, regardless of ordinary
violations. `skipped` means a represented precondition prevented assertion. These statuses do not
replace the complete result arrays.

## 4. False-positive surface

A hand-built diagnostic evaluator could disagree with check, especially around changed-mode full
indexes, scope filters, nested rules, `expect_matches`, Git failures, or post-evaluation redaction.
The selected-rule production entry point and result parity test prohibit that fork.

Counts can mislead if "considered" changes meaning across kinds. Stage names and count labels come
from shared helpers; each selector declares its input population and output population. A kind
cannot emit a bare number with an ad hoc human label. Unsupported or non-string extraction is a
separate count/failure state, never silently folded into zero matches.

A bounded preview can show only compliant rows while an omitted row caused a failure. The assertion
stage reports the actual failed predicate and finding count independently, and the preview always
reports omitted totals. It is evidence sampling, not proof of completeness.

Tracing itself can alter behavior if instrumentation re-parses content, changes iteration order, or
holds borrowed values beyond the evaluator. Emit from existing values, retain only bounded owned
redacted summaries, and compare traced/non-traced production results to prevent observer effects.

## 5. Implementation notes

Introduce typed core records (`RuleTrace`, `TraceStage`, `TraceStatus`, `TraceCounts`,
`TracePreviewRow`, and terminal summary) in a focused module. They are internal Rust types while the
wire contract is experimental; do not freeze them into `alint-core`'s promised public API. The CLI
renderer and experimental schema serialize from these types.

Extend `KindMeta` with a compact `TraceMeta` and update every registration in the same change. The
facts generator and registry completeness tests derive from it. Do not add a parallel list in the
CLI or xtask.

Environment interpolation records a sparse, source/field-addressed provenance map only when an
environment value is actually read; configs with no environment interpolation allocate no
provenance state. Merge and template expansion carry the marks with the winning fragments. Rule
construction consumes them to apply result sensitivity and the shared static payload consumes them
to redact displayed fragments; builder errors redact marked bytes before formatting.

Engine-owned hooks cover level, condition, changed scope, dependency availability,
`expect_matches`, dispatch counts, and terminal outcome. Shared selector/extractor helpers cover
path globs, scope filters, structured queries, regex captures, file/dir sets, graph inputs, and
command results. Kind code adds explicit cardinality/assertion events only where the production
predicate cannot be inferred by a helper. Each hook takes stable identifiers rather than rendered
sentences.

The selected-rule API should share the internal run plan with whole-report execution. Copying one
entry into a second mini-engine is acceptable only if it preserves the exact engine setup and the
parity suite proves it; duplicating dispatch match arms is not. A filtered full run is not acceptable
because unrelated spawning, expensive, or failing rules would execute.

The experimental schema generator lives beside the stable schema generators but writes to
`schemas/experimental`. Package manifests include that file. CLI help/manpages/completions are
regenerated for the new flags.

P5/P6 are a large engine-wide instrumentation change. They add no new third-party dependency:
typed records, bounded heaps, checked counters, serialization, and schema generation use the
standard library and already-approved workspace crates. Any dependency proposed for concurrent
aggregation must first show that per-work-item buffers cannot satisfy the no-lock hot-path contract
and must pass cargo-deny review.

## 6. Tests

Permanent coverage includes:

- CLI argument validation for every flag combination, rule-id failure, formats, changed/base
  behavior, preview bounds, help/manpage/completion generation, and exit mapping;
- byte-for-byte unchanged plain human and JSON explain output for non-sensitive configs, exact
  redaction of environment-derived/explicitly sensitive option, condition, and message evidence in
  both formats, and proof that plain explain does not walk, evaluate, or spawn;
- full-report versus selected-rule trace result equality for every dispatch class, common field,
  nested kind, changed dependency, Git dependency, analysis-time dependency, fix-bearing rule,
  warning/info/off level, and `expect_matches` state;
- one documented pass and fail scenario for every canonical kind, with registry-driven assertions
  that all `TraceMeta` required stages appear and one terminal outcome exists;
- gate outcomes for off, true/false/error conditions, empty/nonempty changed sets, missing Git,
  unavailable analysis time, no selector match, parser failure, command spawn/timeout/nonzero exit,
  nested build failure, and bounded-analysis notes;
- selector/extraction/cardinality count identities, including duplicate structured keys,
  non-string values, unsupported formats, multiple source files, zero/many matches, and exact source
  ranges/pointers;
- deterministic ordering under deliberately shuffled file insertion, randomized hash seeds, and
  repeated parallel scheduling; canonical JSON must be byte-identical;
- preview limits 1, 20, and 100; exact shown/omitted/redacted/truncated counters; multibyte 256-byte
  clipping; nested child bounds; million-row synthetic extraction with retained memory proportional
  to the limit;
- planted sensitive canaries from dotenv, properties, explicit sensitivity, env interpolation,
  explicitly sensitive command output, parser errors, nested rules, evidence-derived paths,
  findings, notes, and related locations, scanned across human and JSON output;
- terminal control/bidi text, non-UTF-8 filesystem names where supported, Windows separator
  normalization, case-sensitive ordering, and width/no-color behavior;
- positive human/JSON output completeness for satisfied, violated, skipped, analysis-failure,
  redacted, omitted-preview, and value-truncated cases;
- experimental schema validation and discriminator/type tests proving `kind: "rule-trace"` cannot
  deserialize as the stable static `kind: "rule"` model;
- allocation/benchmark instrumentation proving ordinary `check` and plain `explain` allocate no
  trace state and do not format preview values; existing benchmark scenarios stay within their
  normal regression gate.

The e2e smoke builds the packaged binary, creates a fresh Git repository with a composed config,
runs static explain, pass/fail/skip traces, changed/base traces, and redacted previews in human and
JSON formats, and compares the selected result with `alint check --format json` for the same rule.
It observes that no fix modifies the repository.

## 7. Open questions

Q14 is resolved for Phase 1b: rule-level trace ships in v0.20, while plan/IR trace waits for Phase 2.
Trace and preview JSON remain explicitly experimental until the v2 stabilization gate; static
explain JSON remains stable and separate.

Q7 is resolved for Phase 1b: extraction rows may appear only through bounded experimental preview.
They are not the relation IR and do not commit Phase 2's row/join representation.

Also resolved:

- `--preview` is opt-in and requires `--trace`;
- only the selected effective rule executes;
- violations and typed analysis failures are represented terminal outcomes, not explain exit 1;
- required trace surfaces derive from `KindMeta` and are runtime-audited;
- normal checks have no trace allocations or formatting;
- value previews are deterministic, redacted at production, and bounded to 1-100 rows per stage.

Deferred: plan-level dependency graphs, timing/profiling spans, trace streaming, stable trace JSON,
plugin-provided trace stages, and the Phase 2 relation IR. Timing is deliberately absent because it
would make output nondeterministic and turn explain into a profiler.
