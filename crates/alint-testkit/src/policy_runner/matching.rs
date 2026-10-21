//! Exact assertion matching for policy-test observations.

use std::collections::HashMap;

use alint_core::AnalysisFailureKind;
use regex::Regex;

use super::{
    Mismatch, MismatchChannel, ObservedAnalysisFailure, ObservedNote, ObservedRelatedLocation,
    ObservedViolation, StepObservation,
};
use crate::policy_test::{
    AnalysisFailureMatcher, LevelMatcher, MessageMatcher, NoteMatcher, RelatedLocationMatcher,
    StepExpectation, ViolationMatcher,
};

pub(super) fn match_step(
    index: usize,
    expected: &StepExpectation,
    actual: &StepObservation,
) -> Vec<Mismatch> {
    let message_regexes = compile_message_regexes(expected);
    let mut mismatches = Vec::new();
    if expected.exit != actual.exit {
        mismatch(
            &mut mismatches,
            index,
            MismatchChannel::Exit,
            format!("expected {:?}, got {:?}", expected.exit, actual.exit),
        );
    }
    match_observation_lists(index, expected, actual, &message_regexes, &mut mismatches);
    exact_list(
        expected.applied.as_deref(),
        &actual.applied,
        index,
        MismatchChannel::Applied,
        &mut mismatches,
    );
    exact_list(
        expected.skipped.as_deref(),
        &actual.skipped,
        index,
        MismatchChannel::Skipped,
        &mut mismatches,
    );
    exact_list(
        expected.suggested.as_deref(),
        &actual.suggested,
        index,
        MismatchChannel::Suggested,
        &mut mismatches,
    );
    exact_list(
        expected.unfixable.as_deref(),
        &actual.unfixable,
        index,
        MismatchChannel::Unfixable,
        &mut mismatches,
    );
    if let Some(expected) = expected.non_convergent
        && expected != actual.non_convergent
    {
        mismatch(
            &mut mismatches,
            index,
            MismatchChannel::NonConvergent,
            format!("expected {expected}, got {}", actual.non_convergent),
        );
    }
    if let Some(expected) = &expected.diff
        && actual.diff.as_deref() != Some(expected)
    {
        mismatch(
            &mut mismatches,
            index,
            MismatchChannel::Diff,
            "canonical diff differs".to_string(),
        );
    }
    match_configuration_error(
        expected
            .configuration_error
            .as_ref()
            .map(|matcher| &matcher.message),
        actual.configuration_error.as_deref(),
        &message_regexes,
        index,
        &mut mismatches,
    );
    mismatches
}

fn match_observation_lists(
    index: usize,
    expected: &StepExpectation,
    actual: &StepObservation,
    message_regexes: &HashMap<&str, Regex>,
    mismatches: &mut Vec<Mismatch>,
) {
    if let Some(expected) = &expected.violations {
        match_counted(
            expected,
            &actual.violations,
            |matcher| matcher.count,
            |matcher, observation| violation_matches(matcher, observation, message_regexes),
            index,
            MismatchChannel::Violations,
            mismatches,
        );
    }
    if let Some(expected) = &expected.analysis_failures {
        match_counted(
            expected,
            &actual.analysis_failures,
            |matcher| matcher.count,
            |matcher, observation| analysis_failure_matches(matcher, observation, message_regexes),
            index,
            MismatchChannel::AnalysisFailures,
            mismatches,
        );
    }
    if let Some(expected) = &expected.notes {
        match_counted(
            expected,
            &actual.notes,
            |matcher| matcher.count,
            |matcher, observation| note_matches(matcher, observation, message_regexes),
            index,
            MismatchChannel::Notes,
            mismatches,
        );
    }
}

fn match_configuration_error(
    expected: Option<&MessageMatcher>,
    actual: Option<&str>,
    regexes: &HashMap<&str, Regex>,
    step: usize,
    mismatches: &mut Vec<Mismatch>,
) {
    match (expected, actual) {
        (Some(expected), Some(actual)) if message_matches(expected, actual, regexes) => {}
        (Some(_), Some(_)) => mismatch(
            mismatches,
            step,
            MismatchChannel::ConfigurationError,
            "configuration error message did not match".to_string(),
        ),
        (Some(_), None) => mismatch(
            mismatches,
            step,
            MismatchChannel::ConfigurationError,
            "expected a configuration error, but evaluation started".to_string(),
        ),
        (None, _) => {}
    }
}

fn exact_list(
    expected: Option<&[String]>,
    actual: &[String],
    step: usize,
    channel: MismatchChannel,
    mismatches: &mut Vec<Mismatch>,
) {
    let Some(expected) = expected else { return };
    let mut expected = expected.to_vec();
    let mut actual = actual.to_vec();
    expected.sort();
    actual.sort();
    if expected != actual {
        mismatch(
            mismatches,
            step,
            channel,
            format!("expected {expected:?}, got {actual:?}"),
        );
    }
}

struct AugmentState {
    seen: Vec<bool>,
    actual_to_expected: Vec<Option<usize>>,
    predecessor_expected_by_actual: Vec<Option<usize>>,
    parent_actual_by_expected: Vec<Option<usize>>,
    stack: Vec<(usize, usize)>,
}

impl AugmentState {
    fn new(len: usize) -> Self {
        Self {
            seen: vec![false; len],
            actual_to_expected: vec![None; len],
            predecessor_expected_by_actual: vec![None; len],
            parent_actual_by_expected: vec![None; len],
            stack: Vec::with_capacity(len),
        }
    }

    fn start(&mut self, expected_index: usize) {
        self.seen.fill(false);
        self.predecessor_expected_by_actual.fill(None);
        self.parent_actual_by_expected.fill(None);
        self.stack.clear();
        self.stack.push((expected_index, 0));
    }
}

fn augment_match<E, A>(
    expected_index: usize,
    expected: &[&E],
    actual: &[A],
    matches: &impl Fn(&E, &A) -> bool,
    state: &mut AugmentState,
) -> bool {
    state.start(expected_index);
    while let Some((current_expected, next_actual)) = state.stack.last_mut() {
        if *next_actual == actual.len() {
            state.stack.pop();
            continue;
        }

        let current_expected = *current_expected;
        let actual_index = *next_actual;
        *next_actual += 1;
        if state.seen[actual_index] || !matches(expected[current_expected], &actual[actual_index]) {
            continue;
        }

        state.seen[actual_index] = true;
        state.predecessor_expected_by_actual[actual_index] = Some(current_expected);
        if let Some(previous_expected) = state.actual_to_expected[actual_index] {
            state.parent_actual_by_expected[previous_expected] = Some(actual_index);
            state.stack.push((previous_expected, 0));
            continue;
        }

        // Flip the complete augmenting path without recursive calls. Policy-test
        // documents are width-unbounded, so a recursive matcher could overflow
        // the process stack on a valid document with many overlapping matchers.
        let mut matched_actual = actual_index;
        loop {
            let matched_expected = state.predecessor_expected_by_actual[matched_actual]
                .expect("every augmenting-path actual has a predecessor");
            state.actual_to_expected[matched_actual] = Some(matched_expected);
            let Some(parent_actual) = state.parent_actual_by_expected[matched_expected] else {
                return true;
            };
            matched_actual = parent_actual;
        }
    }
    false
}

pub(super) fn match_counted<E: std::fmt::Debug, A: std::fmt::Debug>(
    expected: &[E],
    actual: &[A],
    count: impl Fn(&E) -> usize,
    matches: impl Fn(&E, &A) -> bool,
    step: usize,
    channel: MismatchChannel,
    mismatches: &mut Vec<Mismatch>,
) {
    let Some(expected_count) = expected
        .iter()
        .try_fold(0usize, |total, matcher| total.checked_add(count(matcher)))
    else {
        mismatch(
            mismatches,
            step,
            channel,
            format!(
                "exact one-to-one match failed; expected count exceeds the platform limit, got {} actual item(s)",
                actual.len()
            ),
        );
        return;
    };
    if expected_count != actual.len() {
        mismatch(
            mismatches,
            step,
            channel,
            format!(
                "exact one-to-one match failed; expected count {expected_count}, got {} actual item(s)",
                actual.len()
            ),
        );
        return;
    }
    let expanded: Vec<&E> = expected
        .iter()
        .flat_map(|matcher| std::iter::repeat_n(matcher, count(matcher)))
        .collect();
    let mut state = AugmentState::new(actual.len());
    let mut matched_expected = vec![false; expanded.len()];
    for (expected_index, matched) in matched_expected.iter_mut().enumerate() {
        *matched = augment_match(expected_index, &expanded, actual, &matches, &mut state);
    }
    if matched_expected.iter().all(|matched| *matched)
        && state.actual_to_expected.iter().all(Option::is_some)
    {
        return;
    }
    let unmatched_expected: Vec<_> = expanded
        .iter()
        .zip(&matched_expected)
        .filter_map(|(item, matched)| (!matched).then_some(format!("{item:?}")))
        .collect();
    let unmatched_actual: Vec<_> = actual
        .iter()
        .zip(&state.actual_to_expected)
        .filter_map(|(item, matched)| matched.is_none().then_some(format!("{item:?}")))
        .collect();
    mismatch(
        mismatches,
        step,
        channel,
        format!(
            "exact one-to-one match failed; unmatched expectations={unmatched_expected:?}; unmatched actual={unmatched_actual:?}"
        ),
    );
}

fn compile_message_regexes(expected: &StepExpectation) -> HashMap<&str, Regex> {
    fn insert<'a>(regexes: &mut HashMap<&'a str, Regex>, matcher: Option<&'a MessageMatcher>) {
        let Some(source) = matcher.and_then(|matcher| matcher.regex.as_deref()) else {
            return;
        };
        regexes.entry(source).or_insert_with(|| {
            Regex::new(source).expect("policy-test validation compiled this regex")
        });
    }

    let mut regexes = HashMap::new();
    for matcher in expected.violations.iter().flatten() {
        insert(&mut regexes, matcher.message.as_ref());
    }
    for matcher in expected.analysis_failures.iter().flatten() {
        insert(&mut regexes, matcher.message.as_ref());
    }
    for matcher in expected.notes.iter().flatten() {
        insert(&mut regexes, matcher.message.as_ref());
    }
    insert(
        &mut regexes,
        expected
            .configuration_error
            .as_ref()
            .map(|error| &error.message),
    );
    regexes
}

pub(super) fn violation_matches(
    expected: &ViolationMatcher,
    actual: &ObservedViolation,
    regexes: &HashMap<&str, Regex>,
) -> bool {
    expected.rule == actual.rule
        && expected
            .level
            .is_none_or(|value| level_matches(value, &actual.level))
        && optional_eq(expected.path.as_ref(), actual.path.as_ref())
        && expected.line.is_none_or(|value| actual.line == Some(value))
        && expected
            .column
            .is_none_or(|value| actual.column == Some(value))
        && expected
            .end_line
            .is_none_or(|value| actual.end_line == Some(value))
        && expected
            .end_column
            .is_none_or(|value| actual.end_column == Some(value))
        && expected
            .message
            .as_ref()
            .is_none_or(|value| message_matches(value, &actual.message, regexes))
        && expected.related.as_ref().is_none_or(|locations| {
            locations.len() == actual.related.len()
                && locations
                    .iter()
                    .zip(&actual.related)
                    .all(|(left, right)| related_matches(left, right))
        })
}

fn analysis_failure_matches(
    expected: &AnalysisFailureMatcher,
    actual: &ObservedAnalysisFailure,
    regexes: &HashMap<&str, Regex>,
) -> bool {
    expected.rule == actual.rule
        && expected
            .kind
            .is_none_or(|value| value_name(value) == actual.kind)
        && optional_eq(expected.path.as_ref(), actual.path.as_ref())
        && expected.line.is_none_or(|value| actual.line == Some(value))
        && expected
            .column
            .is_none_or(|value| actual.column == Some(value))
        && expected
            .end_line
            .is_none_or(|value| actual.end_line == Some(value))
        && expected
            .end_column
            .is_none_or(|value| actual.end_column == Some(value))
        && expected
            .message
            .as_ref()
            .is_none_or(|value| message_matches(value, &actual.message, regexes))
}

fn note_matches(
    expected: &NoteMatcher,
    actual: &ObservedNote,
    regexes: &HashMap<&str, Regex>,
) -> bool {
    expected
        .kind
        .as_ref()
        .is_none_or(|value| value == &actual.kind)
        && optional_eq(expected.path.as_ref(), actual.path.as_ref())
        && expected.line.is_none_or(|value| actual.line == Some(value))
        && expected
            .column
            .is_none_or(|value| actual.column == Some(value))
        && expected
            .end_line
            .is_none_or(|value| actual.end_line == Some(value))
        && expected
            .end_column
            .is_none_or(|value| actual.end_column == Some(value))
        && expected
            .message
            .as_ref()
            .is_none_or(|value| message_matches(value, &actual.message, regexes))
}

fn related_matches(expected: &RelatedLocationMatcher, actual: &ObservedRelatedLocation) -> bool {
    expected
        .label
        .as_ref()
        .is_none_or(|value| value == &actual.label)
        && expected.path == actual.path
        && expected.line.is_none_or(|value| actual.line == Some(value))
        && expected
            .column
            .is_none_or(|value| actual.column == Some(value))
        && expected
            .end_line
            .is_none_or(|value| actual.end_line == Some(value))
        && expected
            .end_column
            .is_none_or(|value| actual.end_column == Some(value))
}

fn message_matches(
    expected: &MessageMatcher,
    actual: &str,
    regexes: &HashMap<&str, Regex>,
) -> bool {
    if let Some(exact) = &expected.exact {
        return exact == actual;
    }
    expected
        .regex
        .as_deref()
        .and_then(|source| regexes.get(source))
        .is_some_and(|regex| regex.is_match(actual))
}

fn level_matches(expected: LevelMatcher, actual: &str) -> bool {
    matches!(
        (expected, actual),
        (LevelMatcher::Error, "error")
            | (LevelMatcher::Warning, "warning")
            | (LevelMatcher::Info, "info")
            | (LevelMatcher::Off, "off")
    )
}

fn value_name(value: crate::policy_test::AnalysisFailureKindMatcher) -> &'static str {
    match value {
        crate::policy_test::AnalysisFailureKindMatcher::Evaluation => {
            AnalysisFailureKind::Evaluation.as_str()
        }
        crate::policy_test::AnalysisFailureKindMatcher::Condition => {
            AnalysisFailureKind::Condition.as_str()
        }
        crate::policy_test::AnalysisFailureKindMatcher::ResourceRead => {
            AnalysisFailureKind::ResourceRead.as_str()
        }
        crate::policy_test::AnalysisFailureKindMatcher::ResourceParse => {
            AnalysisFailureKind::ResourceParse.as_str()
        }
        crate::policy_test::AnalysisFailureKindMatcher::NestedRuleBuild => {
            AnalysisFailureKind::NestedRuleBuild.as_str()
        }
    }
}

fn optional_eq<T: PartialEq>(expected: Option<&T>, actual: Option<&T>) -> bool {
    expected.is_none_or(|value| actual == Some(value))
}

fn mismatch(mismatches: &mut Vec<Mismatch>, step: usize, channel: MismatchChannel, detail: String) {
    mismatches.push(Mismatch {
        step: Some(step),
        channel,
        detail,
    });
}
