//! Stable aggregate report model for `alint test`.

use schemars::{JsonSchema, schema_for};
use serde::Serialize;

use crate::policy_runner::{Mismatch, PolicyTestRun, StepObservation, TestStatus};
use crate::policy_test::{PolicyTest, TestStep, ValidationDiagnostic};

pub const POLICY_TEST_REPORT_SCHEMA_VERSION: u32 = 1;
pub const POLICY_TEST_REPORT_KIND: &str = "policy-test-report";

/// Stable JSON report emitted by `alint test --format json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyTestReport {
    pub schema_version: u32,
    pub kind: &'static str,
    pub tests: Vec<PolicyTestRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<PolicyTestIssue>,
    pub summary: PolicyTestSummary,
}

impl PolicyTestReport {
    #[must_use]
    pub fn new(tests: Vec<PolicyTestRecord>) -> Self {
        Self::with_issues(tests.len(), tests, Vec::new())
    }

    #[must_use]
    pub fn with_issues(
        selected: usize,
        tests: Vec<PolicyTestRecord>,
        issues: Vec<PolicyTestIssue>,
    ) -> Self {
        let mut summary = PolicyTestSummary {
            selected,
            passed: 0,
            failed: 0,
            invalid: 0,
            skipped: 0,
        };
        for test in &tests {
            match test.status {
                PolicyTestStatus::Passed => summary.passed += 1,
                PolicyTestStatus::Failed => summary.failed += 1,
                PolicyTestStatus::Invalid => summary.invalid += 1,
                PolicyTestStatus::Skipped => summary.skipped += 1,
            }
        }
        Self {
            schema_version: POLICY_TEST_REPORT_SCHEMA_VERSION,
            kind: POLICY_TEST_REPORT_KIND,
            tests,
            issues,
            summary,
        }
    }
}

/// One selected policy test and its deterministic outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyTestRecord {
    pub source: String,
    pub name: String,
    pub tags: Vec<String>,
    pub status: PolicyTestStatus,
    pub steps: Vec<PolicyTestStepReport>,
    pub mismatches: Vec<Mismatch>,
    pub issues: Vec<PolicyTestIssue>,
}

impl PolicyTestRecord {
    #[must_use]
    pub fn executed(source: String, test: &PolicyTest, run: &PolicyTestRun) -> Self {
        let status = match run.status {
            TestStatus::Passed => PolicyTestStatus::Passed,
            TestStatus::Failed => PolicyTestStatus::Failed,
            TestStatus::Skipped => PolicyTestStatus::Skipped,
        };
        let steps = test
            .when
            .iter()
            .copied()
            .zip(run.observations.iter().cloned())
            .enumerate()
            .map(|(index, (operation, observation))| PolicyTestStepReport {
                index,
                operation,
                observation,
            })
            .collect();
        Self {
            source,
            name: test.name.clone(),
            tags: test.tags.clone(),
            status,
            steps,
            mismatches: run.mismatches.clone(),
            issues: Vec::new(),
        }
    }

    #[must_use]
    pub fn invalid(
        source: String,
        test: Option<&PolicyTest>,
        issues: Vec<PolicyTestIssue>,
    ) -> Self {
        Self {
            source,
            name: test.map_or_else(|| "<invalid>".to_string(), |test| test.name.clone()),
            tags: test.map_or_else(Vec::new, |test| test.tags.clone()),
            status: PolicyTestStatus::Invalid,
            steps: Vec::new(),
            mismatches: Vec::new(),
            issues,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyTestStatus {
    Passed,
    Failed,
    Invalid,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyTestStepReport {
    pub index: usize,
    pub operation: TestStep,
    pub observation: StepObservation,
}

/// Definition, discovery, or runner-preflight problem for one selected test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyTestIssue {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub message: String,
}

impl PolicyTestIssue {
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            field: None,
            message: message.into(),
        }
    }

    pub fn from_diagnostic(diagnostic: &ValidationDiagnostic) -> Self {
        Self {
            code: diagnostic.code.as_str().to_string(),
            field: Some(diagnostic.field.clone()),
            message: diagnostic.message.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyTestSummary {
    pub selected: usize,
    pub passed: usize,
    pub failed: usize,
    pub invalid: usize,
    pub skipped: usize,
}

#[must_use]
pub fn policy_test_report_schema() -> schemars::Schema {
    let generated = schema_for!(PolicyTestReport);
    let mut schema =
        serde_json::to_value(generated).expect("schemars schema must serialize to JSON");
    schema["$schema"] =
        serde_json::Value::String("https://json-schema.org/draft/2020-12/schema".to_string());
    schema["$id"] = serde_json::Value::String(
        "https://alint.org/schemas/v1/policy-test-report.json".to_string(),
    );
    schema["title"] = serde_json::Value::String("alint policy-test report v1".to_string());
    schema["properties"]["schema_version"] = serde_json::json!({"type": "integer", "const": 1});
    schema["properties"]["kind"] =
        serde_json::json!({"type": "string", "const": POLICY_TEST_REPORT_KIND});
    schemars::Schema::try_from(schema).expect("generated report remains a JSON Schema object")
}

pub fn policy_test_report_schema_json() -> Result<String, serde_json::Error> {
    let mut rendered = serde_json::to_string_pretty(&policy_test_report_schema())?;
    rendered.push('\n');
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts_every_status() {
        let record = |status| PolicyTestRecord {
            source: "case.yml".to_string(),
            name: "case".to_string(),
            tags: Vec::new(),
            status,
            steps: Vec::new(),
            mismatches: Vec::new(),
            issues: Vec::new(),
        };
        let report = PolicyTestReport::new(vec![
            record(PolicyTestStatus::Passed),
            record(PolicyTestStatus::Failed),
            record(PolicyTestStatus::Invalid),
            record(PolicyTestStatus::Skipped),
        ]);
        assert_eq!(report.summary.selected, 4);
        assert_eq!(report.summary.passed, 1);
        assert_eq!(report.summary.failed, 1);
        assert_eq!(report.summary.invalid, 1);
        assert_eq!(report.summary.skipped, 1);
    }

    #[test]
    fn serialized_report_validates_against_its_schema() {
        let report = PolicyTestReport::new(Vec::new());
        let instance = serde_json::to_value(report).unwrap();
        let schema = policy_test_report_schema();
        let validator =
            jsonschema::validator_for(schema.as_value()).expect("report schema compiles");
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|error| error.to_string())
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
    }
}
