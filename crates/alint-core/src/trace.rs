//! Typed execution-trace records and the trace-only recorder.
//!
//! The wire format remains experimental. These types are public only so the
//! `alint` binary can project them; they are not part of the stable core API.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{Error, Result, RuleResult};

#[cfg(test)]
thread_local! {
    static RECORDERS_CREATED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A trace stage class. Common gates and the terminal outcome are engine-owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStageClass {
    Gate,
    Selection,
    Extraction,
    Cardinality,
    Assertion,
    Nested,
    Outcome,
}

/// The status of one trace stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    Satisfied,
    Violated,
    Skipped,
    AnalysisFailure,
    NotEvaluated,
}

/// Stable machine-readable reason for a trace stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceReason {
    LevelEnabled,
    LevelOff,
    WhenTrue,
    WhenFalse,
    WhenEvaluationFailed,
    ChangedScopeMatched,
    ChangedScopeEmpty,
    GitAvailable,
    GitUnavailable,
    AnalysisTimeAvailable,
    AnalysisTimeUnavailable,
    ManifestAvailable,
    ManifestUnavailable,
    ExpectMatchesSatisfied,
    ExpectMatchesViolated,
    ExpectMatchesFailed,
    SelectorMatched,
    SelectorNoMatch,
    ParseFailed,
    ExtractionSucceeded,
    CardinalitySatisfied,
    CardinalityMismatch,
    AssertionSatisfied,
    AssertionViolated,
    AssertionFailed,
    NestedSatisfied,
    NestedViolated,
    NestedFailed,
    OutcomeSatisfied,
    OutcomeViolated,
    OutcomeSkipped,
    OutcomeAnalysisFailure,
}

/// Checked aggregate counters carried by a stage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct TraceCounts {
    pub considered: u64,
    pub matched: u64,
    pub inputs: u64,
    pub values: u64,
    pub violations: u64,
    pub notes: u64,
    pub analysis_failures: u64,
    pub children: u64,
    pub satisfied_children: u64,
    pub violated_children: u64,
    pub failed_children: u64,
    pub skipped_children: u64,
}

impl TraceCounts {
    /// Add one counter while treating overflow as an engine invariant failure.
    pub fn checked_increment(counter: &mut u64, label: &str) -> Result<()> {
        *counter = counter
            .checked_add(1)
            .ok_or_else(|| Error::internal(format!("trace counter `{label}` overflowed")))?;
        Ok(())
    }
}

/// One work item's aggregate extraction/cardinality observation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TraceExtractionObservation {
    pub inputs: u64,
    pub values: u64,
    pub parse_failures: u64,
    pub cardinality_checks: u64,
    pub cardinality_mismatches: u64,
}

/// Half-open source coordinates retained by a preview row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct TraceSourceRange {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

/// Provenance of one extracted value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceProvenance {
    Literal,
    Structured,
    Capture,
    Derived,
    Command,
    Unknown,
}

/// Producer-supplied sensitivity. Unknown provenance is always redacted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceSensitivity {
    Public,
    Sensitive,
    Unknown,
}

/// Whether a preview path is repository structure or derived evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TracePathOrigin {
    RepositorySource,
    DerivedPublic,
    DerivedSensitive,
    DerivedUnknown,
}

/// One bounded extraction-preview row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TracePreviewRow {
    pub path: PathBuf,
    pub range: Option<TraceSourceRange>,
    pub pointer: Option<String>,
    pub occurrence: u64,
    pub provenance: TraceProvenance,
    pub value_type: String,
    pub display_value: String,
    pub redacted: bool,
    pub value_truncated: bool,
}

impl TracePreviewRow {
    const MAX_DISPLAY_BYTES: usize = 256;

    /// Build a row after applying producer-boundary redaction and display bounds.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        path: &Path,
        path_origin: TracePathOrigin,
        range: Option<TraceSourceRange>,
        pointer: Option<String>,
        occurrence: u64,
        provenance: TraceProvenance,
        sensitivity: TraceSensitivity,
        value_type: impl Into<String>,
        value: &str,
    ) -> Self {
        let value_redacted = !matches!(sensitivity, TraceSensitivity::Public)
            || matches!(provenance, TraceProvenance::Unknown);
        let path_redacted = matches!(
            path_origin,
            TracePathOrigin::DerivedSensitive | TracePathOrigin::DerivedUnknown
        );
        let (display_value, value_truncated) = if value_redacted {
            ("<redacted>".to_string(), false)
        } else {
            truncate_utf8(value, Self::MAX_DISPLAY_BYTES)
        };
        Self {
            path: if path_redacted {
                PathBuf::from("<redacted>")
            } else {
                normalize_path(path)
            },
            range: if path_redacted { None } else { range },
            pointer: if value_redacted || path_redacted {
                None
            } else {
                pointer
            },
            occurrence,
            provenance,
            value_type: value_type.into(),
            display_value,
            redacted: value_redacted || path_redacted,
            value_truncated,
        }
    }

    fn compare_order(&self, other: &Self) -> Ordering {
        self.path
            .cmp(&other.path)
            .then_with(|| self.range.cmp(&other.range))
            .then_with(|| self.pointer.cmp(&other.pointer))
            .then_with(|| self.occurrence.cmp(&other.occurrence))
            .then_with(|| self.provenance.cmp(&other.provenance))
            .then_with(|| self.value_type.cmp(&other.value_type))
            .then_with(|| self.display_value.cmp(&other.display_value))
            .then_with(|| self.redacted.cmp(&other.redacted))
            .then_with(|| self.value_truncated.cmp(&other.value_truncated))
    }
}

impl Ord for TracePreviewRow {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare_order(other)
    }
}

impl PartialOrd for TracePreviewRow {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Exact preview accounting plus the lexicographically first retained rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TracePreview {
    pub total: u64,
    pub shown: u64,
    pub omitted: u64,
    pub redacted: u64,
    pub value_truncated: u64,
    pub rows: Vec<TracePreviewRow>,
}

/// Bounded deterministic row collector. Memory is `O(limit)`.
#[derive(Debug)]
pub struct TracePreviewCollector {
    limit: usize,
    total: u64,
    redacted: u64,
    value_truncated: u64,
    rows: BinaryHeap<TracePreviewRow>,
}

impl TracePreviewCollector {
    pub fn new(limit: usize) -> Result<Self> {
        if !(1..=100).contains(&limit) {
            return Err(Error::internal(format!(
                "trace preview limit {limit} is outside 1..=100"
            )));
        }
        Ok(Self {
            limit,
            total: 0,
            redacted: 0,
            value_truncated: 0,
            rows: BinaryHeap::with_capacity(limit),
        })
    }

    pub fn push(&mut self, row: TracePreviewRow) -> Result<()> {
        TraceCounts::checked_increment(&mut self.total, "preview.total")?;
        if row.redacted {
            TraceCounts::checked_increment(&mut self.redacted, "preview.redacted")?;
        }
        if row.value_truncated {
            TraceCounts::checked_increment(&mut self.value_truncated, "preview.value_truncated")?;
        }
        if self.rows.len() < self.limit {
            self.rows.push(row);
        } else if self.rows.peek().is_some_and(|largest| row < *largest) {
            let _ = self.rows.pop();
            self.rows.push(row);
        }
        Ok(())
    }

    pub fn finish(self) -> TracePreview {
        let mut rows = self.rows.into_vec();
        rows.sort();
        let shown = u64::try_from(rows.len()).expect("preview limit fits u64");
        TracePreview {
            total: self.total,
            shown,
            omitted: self.total - shown,
            redacted: self.redacted,
            value_truncated: self.value_truncated,
            rows,
        }
    }
}

/// One typed trace stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraceStage {
    pub ordinal: u32,
    pub class: TraceStageClass,
    pub name: String,
    pub status: TraceStatus,
    pub reason: TraceReason,
    pub counts: TraceCounts,
    pub detail: Option<String>,
    pub preview: Option<TracePreview>,
}

impl TraceStage {
    pub fn new(
        class: TraceStageClass,
        name: impl Into<String>,
        status: TraceStatus,
        reason: TraceReason,
    ) -> Self {
        Self {
            ordinal: 0,
            class,
            name: name.into(),
            status,
            reason,
            counts: TraceCounts::default(),
            detail: None,
            preview: None,
        }
    }

    #[must_use]
    pub fn with_counts(mut self, counts: TraceCounts) -> Self {
        self.counts = counts;
        self
    }
}
/// Trace-only producer boundary carried by an evaluation [`crate::Context`].
///
/// Implementations must redact evidence before retaining it. The engine uses a
/// mutex-backed implementation only for selected-rule tracing; ordinary checks
/// carry `None` and never construct or call a sink.
pub trait TraceSink: Sync + std::fmt::Debug {
    fn record(&self, stage: TraceStage);

    fn record_extraction(&self, observation: TraceExtractionObservation);
}

/// Static trace inventory attached to every canonical rule kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraceMeta {
    pub allowed_stages: Cow<'static, [TraceStageClass]>,
    pub required_stages: Cow<'static, [TraceStageClass]>,
    pub emits_preview_rows: bool,
    pub nested_evaluation: bool,
}

const ASSERTION_STAGES: &[TraceStageClass] = &[TraceStageClass::Assertion];
const SELECTION_STAGES: &[TraceStageClass] =
    &[TraceStageClass::Selection, TraceStageClass::Assertion];
const EXTRACTION_STAGES: &[TraceStageClass] = &[
    TraceStageClass::Selection,
    TraceStageClass::Extraction,
    TraceStageClass::Cardinality,
    TraceStageClass::Assertion,
];
const NESTED_STAGES: &[TraceStageClass] = &[
    TraceStageClass::Selection,
    TraceStageClass::Nested,
    TraceStageClass::Assertion,
];

impl TraceMeta {
    pub const ASSERTION: Self = Self {
        allowed_stages: Cow::Borrowed(ASSERTION_STAGES),
        required_stages: Cow::Borrowed(ASSERTION_STAGES),
        emits_preview_rows: false,
        nested_evaluation: false,
    };
    pub const SELECTION: Self = Self {
        allowed_stages: Cow::Borrowed(SELECTION_STAGES),
        required_stages: Cow::Borrowed(SELECTION_STAGES),
        emits_preview_rows: false,
        nested_evaluation: false,
    };
    /// Extraction/cardinality events are option-dependent; selection and the
    /// final policy assertion remain the unconditional required stages.
    pub const EXTRACTION: Self = Self {
        allowed_stages: Cow::Borrowed(EXTRACTION_STAGES),
        required_stages: Cow::Borrowed(SELECTION_STAGES),
        emits_preview_rows: true,
        nested_evaluation: false,
    };
    pub const NESTED: Self = Self {
        allowed_stages: Cow::Borrowed(NESTED_STAGES),
        required_stages: Cow::Borrowed(NESTED_STAGES),
        emits_preview_rows: false,
        nested_evaluation: true,
    };

    /// Validate one canonical kind's trace inventory before evaluation.
    pub fn validate_for_kind(&self, kind: &str) -> Result<()> {
        let mut allowed = BTreeSet::new();
        for class in self.allowed_stages.iter() {
            if matches!(class, TraceStageClass::Gate | TraceStageClass::Outcome) {
                return Err(Error::internal(format!(
                    "rule kind `{kind}` declares engine-owned trace stage `{class:?}`"
                )));
            }
            if !allowed.insert(*class) {
                return Err(Error::internal(format!(
                    "rule kind `{kind}` declares duplicate trace stage `{class:?}`"
                )));
            }
        }
        let mut required = BTreeSet::new();
        for class in self.required_stages.iter() {
            if !required.insert(*class) {
                return Err(Error::internal(format!(
                    "rule kind `{kind}` requires duplicate trace stage `{class:?}`"
                )));
            }
            if !allowed.contains(class) {
                return Err(Error::internal(format!(
                    "rule kind `{kind}` requires undeclared trace stage `{class:?}`"
                )));
            }
        }
        if !required.contains(&TraceStageClass::Assertion) {
            return Err(Error::internal(format!(
                "rule kind `{kind}` must require the policy assertion trace stage"
            )));
        }
        if self.emits_preview_rows && !allowed.contains(&TraceStageClass::Extraction) {
            return Err(Error::internal(format!(
                "rule kind `{kind}` can emit preview rows without declaring extraction"
            )));
        }
        if self.nested_evaluation != required.contains(&TraceStageClass::Nested) {
            return Err(Error::internal(format!(
                "rule kind `{kind}` nested trace metadata is inconsistent"
            )));
        }
        Ok(())
    }
}

/// Complete traced execution of one effective configured rule.
#[derive(Debug, Clone)]
pub struct RuleTrace {
    pub rule_id: String,
    pub kind: String,
    pub stages: Vec<TraceStage>,
    pub outcome: TraceStatus,
    pub result: Option<RuleResult>,
}

/// Trace-only stage recorder. Ordinary checks never construct one.
#[derive(Debug)]
pub(crate) struct TraceRecorder {
    meta: TraceMeta,
    stages: Vec<TraceStage>,
    next_ordinal: u32,
}

impl TraceRecorder {
    pub(crate) fn new(meta: TraceMeta) -> Self {
        #[cfg(test)]
        RECORDERS_CREATED.with(|count| count.set(count.get() + 1));
        Self {
            meta,
            stages: Vec::new(),
            next_ordinal: 0,
        }
    }

    pub(crate) fn record(&mut self, mut stage: TraceStage) -> Result<()> {
        stage.ordinal = self.next_ordinal;
        self.next_ordinal = self
            .next_ordinal
            .checked_add(1)
            .ok_or_else(|| Error::internal("trace stage ordinal overflowed"))?;
        self.stages.push(stage);
        Ok(())
    }

    pub(crate) fn finish(
        mut self,
        rule_id: String,
        kind: String,
        result: Option<RuleResult>,
    ) -> Result<RuleTrace> {
        self.meta.validate_for_kind(&kind)?;
        if self
            .stages
            .iter()
            .any(|stage| stage.class == TraceStageClass::Outcome)
        {
            return Err(Error::internal(format!(
                "rule kind `{kind}` emitted the engine-owned terminal trace stage"
            )));
        }
        let assertion_completed = self.stages.iter().any(|stage| {
            stage.class == TraceStageClass::Assertion
                && matches!(stage.status, TraceStatus::Satisfied | TraceStatus::Violated)
        });
        let observed: BTreeSet<TraceStageClass> = self
            .stages
            .iter()
            .filter_map(|stage| {
                (!matches!(
                    stage.class,
                    TraceStageClass::Gate | TraceStageClass::Outcome
                ))
                .then_some(stage.class)
            })
            .collect();
        for stage in &observed {
            if !self.meta.allowed_stages.contains(stage) {
                return Err(Error::internal(format!(
                    "rule kind `{kind}` emitted undeclared trace stage `{stage:?}`"
                )));
            }
        }
        if assertion_completed {
            for required in self.meta.required_stages.iter() {
                if !observed.contains(required) {
                    return Err(Error::internal(format!(
                        "rule kind `{kind}` reached assertion without required trace stage `{required:?}`"
                    )));
                }
            }
        }
        let outcome = match result.as_ref() {
            Some(result) if !result.analysis_failures.is_empty() => TraceStatus::AnalysisFailure,
            Some(result) if !result.violations.is_empty() => TraceStatus::Violated,
            Some(_) => TraceStatus::Satisfied,
            None if self
                .stages
                .iter()
                .any(|stage| stage.status == TraceStatus::AnalysisFailure) =>
            {
                TraceStatus::AnalysisFailure
            }
            None if self
                .stages
                .iter()
                .any(|stage| stage.status == TraceStatus::Skipped) =>
            {
                TraceStatus::Skipped
            }
            None => {
                return Err(Error::internal(format!(
                    "rule kind `{kind}` produced neither a result nor a represented skip"
                )));
            }
        };
        let reason = match outcome {
            TraceStatus::Satisfied => TraceReason::OutcomeSatisfied,
            TraceStatus::Violated => TraceReason::OutcomeViolated,
            TraceStatus::Skipped => TraceReason::OutcomeSkipped,
            TraceStatus::AnalysisFailure => TraceReason::OutcomeAnalysisFailure,
            TraceStatus::NotEvaluated => {
                return Err(Error::internal(
                    "terminal trace outcome cannot be not_evaluated",
                ));
            }
        };
        let counts = result
            .as_ref()
            .map_or_else(TraceCounts::default, |result| TraceCounts {
                violations: u64::try_from(result.violations.len()).expect("length fits u64"),
                notes: u64::try_from(result.notes.len()).expect("length fits u64"),
                analysis_failures: u64::try_from(result.analysis_failures.len())
                    .expect("length fits u64"),
                ..TraceCounts::default()
            });
        self.record(
            TraceStage::new(TraceStageClass::Outcome, "outcome", outcome, reason)
                .with_counts(counts),
        )?;
        Ok(RuleTrace {
            rule_id,
            kind,
            stages: self.stages,
            outcome,
            result,
        })
    }
}

#[cfg(test)]
pub(crate) fn recorders_created_on_current_thread() -> u64 {
    RECORDERS_CREATED.with(std::cell::Cell::get)
}

#[cfg(windows)]
fn normalize_path(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().replace('\\', "/"))
}

#[cfg(not(windows))]
fn normalize_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

fn truncate_utf8(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str, occurrence: u64) -> TracePreviewRow {
        TracePreviewRow::new(
            Path::new(path),
            TracePathOrigin::RepositorySource,
            None,
            None,
            occurrence,
            TraceProvenance::Structured,
            TraceSensitivity::Public,
            "string",
            path,
        )
    }

    #[test]
    fn preview_retains_lexicographically_first_rows_with_exact_counts() {
        let mut collector = TracePreviewCollector::new(2).unwrap();
        collector.push(row("z", 0)).unwrap();
        collector.push(row("a", 0)).unwrap();
        collector.push(row("m", 0)).unwrap();
        let preview = collector.finish();
        assert_eq!(preview.total, 3);
        assert_eq!(preview.shown, 2);
        assert_eq!(preview.omitted, 1);
        assert_eq!(preview.rows[0].path, Path::new("a"));
        assert_eq!(preview.rows[1].path, Path::new("m"));
    }

    #[test]
    fn preview_order_is_deterministic_when_structural_keys_collide() {
        let make_row = |value: &str| {
            TracePreviewRow::new(
                Path::new("same"),
                TracePathOrigin::RepositorySource,
                None,
                Some("/same".to_string()),
                0,
                TraceProvenance::Structured,
                TraceSensitivity::Public,
                "string",
                value,
            )
        };
        for values in [["z", "a"], ["a", "z"]] {
            let mut collector = TracePreviewCollector::new(1).unwrap();
            for value in values {
                collector.push(make_row(value)).unwrap();
            }
            assert_eq!(collector.finish().rows[0].display_value, "a");
        }
    }

    #[test]
    fn preview_redacts_unknown_and_sensitive_values_before_storage() {
        for (provenance, sensitivity) in [
            (TraceProvenance::Unknown, TraceSensitivity::Public),
            (TraceProvenance::Command, TraceSensitivity::Sensitive),
            (TraceProvenance::Structured, TraceSensitivity::Unknown),
        ] {
            let row = TracePreviewRow::new(
                Path::new("a"),
                TracePathOrigin::RepositorySource,
                None,
                None,
                0,
                provenance,
                sensitivity,
                "string",
                "secret-canary",
            );
            assert!(row.redacted);
            assert_eq!(row.display_value, "<redacted>");
        }
    }

    #[test]
    fn preview_redacts_sensitive_derived_paths_and_structural_pointers() {
        let row = TracePreviewRow::new(
            Path::new("secret-canary/result.txt"),
            TracePathOrigin::DerivedSensitive,
            None,
            Some("/secret-canary".to_string()),
            0,
            TraceProvenance::Derived,
            TraceSensitivity::Sensitive,
            "string",
            "secret-canary",
        );
        assert_eq!(row.path, Path::new("<redacted>"));
        assert!(row.pointer.is_none());
        assert_eq!(row.display_value, "<redacted>");
        assert!(row.redacted);

        let path_only = TracePreviewRow::new(
            Path::new("secret-canary/result.txt"),
            TracePathOrigin::DerivedSensitive,
            Some(TraceSourceRange {
                start_line: 1,
                start_column: 2,
                end_line: 3,
                end_column: 4,
            }),
            Some("/secret-canary".to_string()),
            0,
            TraceProvenance::Derived,
            TraceSensitivity::Public,
            "string",
            "public",
        );
        assert_eq!(path_only.path, Path::new("<redacted>"));
        assert!(path_only.range.is_none());
        assert!(path_only.pointer.is_none());
        assert_eq!(path_only.display_value, "public");
        assert!(path_only.redacted);
    }

    #[test]
    fn preview_limit_is_bounded_and_accounting_includes_omitted_rows() {
        assert!(TracePreviewCollector::new(0).is_err());
        assert!(TracePreviewCollector::new(101).is_err());

        let mut collector = TracePreviewCollector::new(1).unwrap();
        collector.push(row("z", 0)).unwrap();
        collector.push(row("a", 0)).unwrap();
        let preview = collector.finish();
        assert_eq!((preview.total, preview.shown, preview.omitted), (2, 1, 1));
        assert_eq!(preview.rows[0].path, Path::new("a"));
    }

    #[test]
    fn preview_truncates_at_a_utf8_boundary() {
        let value = format!("{}é", "a".repeat(255));
        let row = TracePreviewRow::new(
            Path::new("a"),
            TracePathOrigin::RepositorySource,
            None,
            None,
            0,
            TraceProvenance::Literal,
            TraceSensitivity::Public,
            "string",
            &value,
        );
        assert_eq!(row.display_value.len(), 255);
        assert!(row.value_truncated);
    }

    #[test]
    fn recorder_rejects_missing_and_undeclared_kind_stages() {
        let mut missing = TraceRecorder::new(TraceMeta::SELECTION);
        missing
            .record(TraceStage::new(
                TraceStageClass::Assertion,
                "policy",
                TraceStatus::Satisfied,
                TraceReason::AssertionSatisfied,
            ))
            .unwrap();
        assert!(
            missing
                .finish("id".into(), "kind".into(), Some(passing_result()))
                .unwrap_err()
                .to_string()
                .contains("required trace stage")
        );

        let mut undeclared = TraceRecorder::new(TraceMeta::ASSERTION);
        undeclared
            .record(TraceStage::new(
                TraceStageClass::Selection,
                "selector",
                TraceStatus::Satisfied,
                TraceReason::SelectorMatched,
            ))
            .unwrap();
        assert!(
            undeclared
                .finish("id".into(), "kind".into(), None)
                .unwrap_err()
                .to_string()
                .contains("undeclared trace stage")
        );
    }

    #[test]
    fn recorder_rejects_producer_owned_terminal_and_invalid_inventory() {
        let mut terminal = TraceRecorder::new(TraceMeta::ASSERTION);
        terminal
            .record(TraceStage::new(
                TraceStageClass::Outcome,
                "outcome",
                TraceStatus::Satisfied,
                TraceReason::OutcomeSatisfied,
            ))
            .unwrap();
        assert!(
            terminal
                .finish("id".into(), "kind".into(), None)
                .unwrap_err()
                .to_string()
                .contains("engine-owned terminal")
        );

        let invalid = TraceMeta {
            allowed_stages: Cow::Borrowed(&[TraceStageClass::Assertion]),
            required_stages: Cow::Borrowed(&[TraceStageClass::Extraction]),
            emits_preview_rows: false,
            nested_evaluation: false,
        };
        assert!(
            invalid
                .validate_for_kind("broken")
                .unwrap_err()
                .to_string()
                .contains("requires undeclared")
        );
    }

    #[test]
    fn runtime_owned_trace_metadata_remains_supported() {
        let metadata = TraceMeta {
            allowed_stages: Cow::Owned(vec![
                TraceStageClass::Selection,
                TraceStageClass::Assertion,
            ]),
            required_stages: Cow::Owned(vec![TraceStageClass::Assertion]),
            emits_preview_rows: false,
            nested_evaluation: false,
        };
        metadata.validate_for_kind("runtime-plugin").unwrap();
    }

    #[test]
    fn common_gate_analysis_failure_is_not_misreported_as_skip() {
        let mut recorder = TraceRecorder::new(TraceMeta::ASSERTION);
        recorder
            .record(TraceStage::new(
                TraceStageClass::Gate,
                "when",
                TraceStatus::AnalysisFailure,
                TraceReason::WhenEvaluationFailed,
            ))
            .unwrap();
        let trace = recorder.finish("id".into(), "kind".into(), None).unwrap();
        assert_eq!(trace.outcome, TraceStatus::AnalysisFailure);
        assert_eq!(
            trace.stages.last().map(|stage| stage.reason),
            Some(TraceReason::OutcomeAnalysisFailure)
        );
    }

    fn passing_result() -> RuleResult {
        RuleResult::new("id".into(), crate::Level::Error, None, Vec::new(), false)
    }
}
