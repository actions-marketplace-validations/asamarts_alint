# Design doc: fail-loud analysis failures

Status: Implemented (2026-10-16).
Decisions: [ADR-0012](../../adr/0012-output-completeness-testing.md),
[ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md) (DEC-2)
Demand evidence: [`general-linter.md` section 4.12](../general-linter.md#412-current-fail-open-and-leak-sites)

## 1. Problem

A rule evaluation error is currently converted to an ordinary `Violation` at the rule's configured
level. A warning- or info-level rule can therefore fail to inspect repository content while `alint
check` exits 0, and `alint baseline` can permanently suppress the resulting pathless, message-keyed
"rule error". The same semantic failure has several other representations: `when` errors, nested
rule build/evaluation errors, unreadable resources, and document parse failures are ordinary
violations or are silently omitted by some control-flow paths.

This violates DEC-2. A failure caused by repository content or repository-controlled evaluation
input is a finding that always counts as an error, is never baselinable, and remains distinct from a
policy violation. Configuration errors still fail before evaluation with exit 2; engine defects and
other internal errors still use exit 3.

## 2. Surface area

No YAML or rule-kind option changes.

`alint-core` gains a structural `AnalysisFailure` record and `AnalysisFailureKind`. Every
`RuleResult` and `FixRuleResult` carries an `analysis_failures` collection separate from policy
violations, informational notes, and fix items. The stable kinds are:

- `evaluation`: a rule or per-file evaluator returned `Err`;
- `condition`: a runtime `when` expression could not be evaluated;
- `resource_read`: an in-scope resource could not be read after discovery;
- `resource_parse`: bytes were read, but the configured document parser could not produce a value;
- `nested_rule_build`: a repository-derived nested rule instance could not be built.

Each record carries an optional repository-relative path, message, and optional line/column. Its
rule id and policy URL come from the owning result. The kind is data; no consumer identifies a
failure by matching message text.

The check JSON report adds `summary.total_analysis_failures` and a per-rule
`analysis_failures` array. The agent report adds the same count and a flat `analysis_failures`
array. Their committed v1 schemas change atomically with the serializers. These are additive W2
fields; DEC-4's W6 decision still controls the one-release-ahead relaxation for the richer finding
location fields.

The eight check formats project failures as follows:

| Format | Projection |
|---|---|
| Human | An error-styled `analysis failure` finding, grouped by resource path. |
| JSON | Full per-rule structured records with `kind`, path, message, line, and column. |
| SARIF | An error result with `kind: fail`, the failure class in result properties, and `executionSuccessful: false`. |
| GitHub | An `error` annotation; path and location are retained when present. |
| GitLab | A Code Quality issue at error severity with a failure-class-prefixed description. |
| Markdown | An error row labeled `analysis failure`; path and location are retained. |
| JUnit | An errored testcase, not a policy-failure testcase. |
| Agent | A non-remediation analysis-failure record; it never receives a fix command or proposed edit. |

LSP maps every failure to an Error diagnostic regardless of the rule's configured level. A
pathless failure is anchored to the config file, matching existing repository-level diagnostics.
The diagnostic message includes the stable failure class.

## 3. Semantics

A result passes only when both `violations` and `analysis_failures` are empty. Notes remain
non-failing. `Report::has_errors()` is true when any analysis failure exists, including one owned by
a warning, info, or off rule. `has_warnings()` continues to describe policy violations only.
Summary violation counts remain policy-violation counts; analysis failures have their own count.

The engine converts every `Err` it currently wraps as `Violation::new("rule error: ...")`, every
runtime `when` error, and each equivalent single-file or nested path into the structural collection.
File-major evaluation attaches the current repository path. A rule-major error is repository-level
unless the error producer supplied a resource path. Nested failures retain the parent rule id and
selected resource path; they do not masquerade as violations of the synthesized child id.

An unreadable discovered file is `resource_read`. A malformed document encountered by a configured
structured evaluator is `resource_parse`: invalid syntax prevented the configured assertion from
running. By contrast, a parser or validator that successfully produces a value and determines that
it violates the configured policy emits an ordinary violation. Bounded analysis that deliberately
stops at a documented cap remains the W4 `analysis_truncated` note unless that rule's existing
contract explicitly treats the cap as a policy violation.

A failed external command is classified by contract. A nonzero status used by a `command` or
`command_idempotent` rule as its asserted policy result remains a violation. Failure to spawn, wait,
or finish before the configured timeout is an analysis failure because no policy verdict was
obtained.

Baseline application and generation inspect only `violations`; they copy analysis failures through
unchanged. `alint baseline` refuses to write or update a baseline when the run contains any analysis
failure. An existing baseline can suppress ordinary violations from the same rule but cannot remove
or downgrade its failures.

Fix evaluation partitions failures before baseline classification and fixer dispatch. A failure is
never offered to a fixer. It is rendered in fix reports and forces exit 1, including under
`--fix-only`, `--dry-run`, `--diff`, warning/info rule levels, and `--baseline`. Non-convergence keeps
its existing higher-priority exit 2.

W3 still owns manifest-derived scope parse failures, Git/repository unavailability in Git-aware
kinds, and non-string extraction outcomes. W2 provides the structural destination and output
contract those paths will use; it does not silently implement W3's policy decisions.

## 4. False-positive surface

The principal risk is reclassifying a legitimate policy failure as an execution failure and making
it non-baselinable. The boundary is whether evaluation obtained the configured verdict:

- expected command nonzero statuses and schema-validation mismatches remain violations;
- spawn, read, parse, and runtime-expression failures become analysis failures;
- load-time invalid options and static nested-rule errors remain configuration errors (exit 2);
- a repository-derived nested instance that becomes invalid only after token substitution is an
  analysis failure.

A file removed between walk and read keeps the existing race semantics: `NotFound` is ignored.
Other read errors are failures. Repository-controlled paths and messages pass through each
format's existing terminal, workflow-command, Markdown, XML, and JSON escaping.

Failures never carry baseline keys, baseline state, fixability, proposed edits, or fix statuses.
This prevents an accidental later code path from making them suppressible or remediable.

## 5. Implementation notes

`alint-core::rule` owns `AnalysisFailure`, `AnalysisFailureKind`, producer constructors, and result
partitioning. The existing rule return type remains `Result<Vec<Violation>>` for this release;
`Violation` is the internal producer carrier and gains a structural disposition so rules that need
to continue after one failed resource can emit a failure without aborting the remaining scan.
`RuleResult::new` consumes that carrier and moves failures into their dedicated collection before
fixability and baseline logic. No message-prefix sniffing is permitted.

The per-file file-major bucket carries violations and failures without a second file scan. Empty-
failure runs retain the existing hot path and deterministic ordering. Output formatters add only a
linear pass over failure vectors. No dependency is added.

Constitution invariant 1 applies: result and failure order follows rule order, then existing
resource traversal order, and is byte-identical across runs. ADR-0012 applies: every check format
has a positive sentinel gate, not merely a non-empty-output test.

## 6. Tests

- Core tests cover all five kinds, result partitioning, failure-only pass/fail counts, warning/info
  escalation, deterministic ordering, and file-major/resource-path preservation.
- Regression tests cover every former engine "rule error" wrapper: rule-major, file-major,
  single-file/LSP, `expect_matches`, fix, and nested evaluation/build/condition paths.
- A warning-level structured rule over malformed input exits 1 and exposes
  `kind: resource_parse`; a valid mismatching document remains an ordinary warning violation.
- Baseline tests prove generation refuses a failure, application preserves it, existing entries do
  not suppress it, and no failure is fingerprinted.
- Fix tests prove no fixer runs for a failure and all fix modes exit 1 except existing
  non-convergence exit 2 precedence.
- JSON, agent, SARIF, GitLab, and JUnit documents validate against their published formats/schemas.
- LSP tests prove Error severity and config-file anchoring for a pathless failure.
- An exhaustive ADR-0012 gate matches every `Format` variant and proves the sentinel rule id,
  failure kind, resource, and reason reach the format-specific projection.
- CLI scenarios cover human and all machine formats, warning-level exit behavior, baseline refusal,
  and deterministic repeated output.

No benchmark threshold is needed. The changed path adds no work to successful evaluations and one
linear projection pass only when failures exist; the standard release benchmark smoke and
cumulative performance gate still run.

## 7. Open questions

All decisions are resolved.

- **2026-10-16:** Failures live in a dedicated collection on each rule result, not in the violation
  vector with a severity override.
- **2026-10-16:** The structural classes are evaluation, condition, resource read, resource parse,
  and nested-rule build; renderers never classify by English text.
- **2026-10-16:** Malformed configured input is a non-baselinable analysis failure; a successfully
  evaluated policy mismatch remains a violation.
- **2026-10-16:** Fix reports carry the same failure records; `--fix-only` cannot hide incomplete
  analysis.
- **2026-10-16:** JSON and agent v1 gain additive structural fields with atomic schema updates;
  W6 still owns the DEC-4 general schema relaxation and related-location fields.
