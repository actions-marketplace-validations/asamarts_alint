# Design doc: notes in every output format

Status: Accepted (2026-10-16).
Decisions: [ADR-0012](../../adr/0012-output-completeness-testing.md),
[ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md)
Demand evidence: [`general-linter.md` section 4.12](../general-linter.md#412-current-fail-open-and-leak-sites)

## 1. Problem

Rules can emit informational notes when analysis deliberately skips an entry rather than finding a
violation. The engine separates these notes from violations, but only JSON and the human CLI expose
them today. SARIF, GitHub, GitLab, Markdown, JUnit, and agent output silently drop them. A green CI
surface can therefore hide incomplete analysis that another output format reports.

The output-completeness contract in ADR-0012 requires a positive gate: each supported output must
prove that a note reaches the consumer. Notes remain non-violations and must not change the exit
status, pass/fail counts, baseline, fingerprints, or fixability.

## 2. Surface area

No YAML or rule-kind option changes. The check-report model gains a structural note kind:

- `informational`: the existing skipped-entry note;
- `analysis_truncated`: bounded analysis stopped before inspecting all candidate input.

`Violation::as_note()` remains the producer API for an informational note. A distinct constructor
marks truncated analysis; formatters never infer the kind from message text.

The eight check formats project notes as follows:

| Format | Projection |
|---|---|
| Human | Existing stderr count; `--show-notes` prints the full list. |
| JSON | Existing per-rule `notes` array, with an additive `kind` field. |
| SARIF | Informational notes are `level: note` results; truncation is a `toolExecutionNotifications` entry. |
| GitHub | One unlocated `notice` containing the count and full-list pointer. |
| GitLab | One valid Code Quality `info` issue containing the count and pointer. |
| Markdown | One blockquote containing the count and pointer. |
| JUnit | One suite-level `system-out` line containing the count and pointer. |
| Agent | An additive top-level note summary with the count and `details_format: "json"`. |

The fixed pointer text tells users to rerun the same check with `--format json` for the full list.
This avoids fabricating a command that omits the original path, config, `--only`, or `--changed`
arguments. `--show-notes` remains a human-output control; machine stdout remains self-contained and
byte-clean.

## 3. Semantics

A note-only rule still passes. A note-only report exits 0, retains its passing-rule count, and says
that no violations were found where the format has such a message. Summary-only formats add exactly
one note summary when at least one note exists. Empty-note reports are byte-identical to their
pre-W4 output.

JSON preserves each note's rule association, path, line, column, and message. SARIF does the same
for informational notes and forces `level: note` regardless of the owning rule's configured level.
SARIF notes carry no fixes, fingerprints, baseline state, or suppressions. Truncated notes are not
also emitted as results: each becomes one invocation notification, preserving its message and
location without presenting incomplete execution as a policy violation.

The GitLab summary is appended after real issues and bypasses the positional violation-fingerprint
grid. Its reserved fingerprint is stable across note counts, so one dashboard item is updated rather
than recreated. JUnit notes do not add tests, failures, or errors. Agent notes are not remediation
tasks and therefore do not enter `violations` or gain an `agent_instruction` or fix command.

Within each rule, SARIF order is live violations, informational notes, then baseline-suppressed
violations. Truncation notifications follow report and note order. All count-only projections use
the report's deterministic result/note order only to count, never to construct identity.

This item covers the eight `check` formats over `Report`. Fix reports and LSP diagnostics are
separate contracts. The LSP keeps its existing stderr count/full-list behavior and does not publish
notes as Hint diagnostics.

## 4. False-positive surface

Notes never assert policy failure, so their primary risk is accidental escalation. Mitigations:

- every pass/fail, exit, baseline, fingerprint, and fix path remains violation-only;
- SARIF uses `note`, GitLab uses `info`, GitHub uses `notice`, and JUnit uses `system-out` rather
  than failure-bearing records;
- note summaries contain no repository-controlled note body, limiting annotation flooding and
  terminal-control exposure;
- truncated analysis is structural, never recognized by matching English text;
- a note emitted by a fixer-backed rule is forcibly non-fixable.

SARIF and GitLab consumers may count their visible note record as an item even though alint exits 0.
That consumer-visible count change is intentional and documented in the changelog.

## 5. Implementation notes

`alint-core` owns `NoteKind`, note partitioning, and an allocation-free `Report::total_notes()`.
The existing `RuleResult::notes: Vec<Violation>` remains in place ahead of W6's richer finding
model. `alint-output` performs the per-format projections. The strict agent and check JSON Schemas
change atomically with their serializers. No new dependency is required.

Invariant 1 requires stable ordering and byte-identical empty-note output. ADR-0012 requires an
exhaustive positive format gate. Baseline application must continue cloning notes unchanged while
fingerprinting only violations.

## 6. Tests

- Core tests cover informational/truncated partitioning, note-only pass behavior,
  `total_notes()`, and non-fixability under a fixer-backed result.
- Formatter tests cover note-only and mixed reports, escaping, exact count, pointer, and unchanged
  violation/test/failure totals.
- SARIF tests distinguish note results from truncation notifications and prove neither receives a
  fix or fingerprint.
- JSON and agent note-bearing documents validate against the committed public schemas.
- An exhaustive ADR-0012 gate matches every `Format` variant and proves the sentinel note reaches
  it in the format-specific projection. The human half runs the CLI because human notes live on
  stderr after the formatter.
- Existing note-free format snapshots must remain unchanged.

No benchmark threshold is needed: count-only formats add one linear pass over result vectors, and
full-detail formats already traverse the report. The zero-note branches allocate nothing beyond
existing formatter state.

## 7. Open questions

All decisions are resolved.

- **2026-10-16:** GitLab uses one synthetic valid Code Quality issue, not a nonstandard envelope.
- **2026-10-16:** JUnit uses suite-level `system-out`, not a testcase or failure.
- **2026-10-16:** Agent exposes a structured summary, not full note bodies or remediation entries.
- **2026-10-16:** The full-list pointer names `--format json`; `--show-notes` stays human-only.
- **2026-10-16:** Truncation is a structural `NoteKind` and maps only to SARIF execution
  notifications.
