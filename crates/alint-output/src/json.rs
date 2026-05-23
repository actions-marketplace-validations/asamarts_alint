use std::io::Write;
use std::path::Path;

use alint_core::{FixReport, FixStatus, Level, Report};
use serde::Serialize;

#[derive(Serialize)]
struct JsonReport<'a> {
    schema_version: u32,
    summary: Summary,
    results: Vec<JsonResult<'a>>,
}

#[derive(Serialize)]
struct Summary {
    failing_rules: usize,
    passing_rules: usize,
    total_violations: usize,
    has_errors: bool,
    has_warnings: bool,
}

#[derive(Serialize)]
struct JsonResult<'a> {
    id: &'a str,
    level: Level,
    passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_url: Option<&'a str>,
    /// Whether the rule declares a fixer. Useful for downstream
    /// tools that want to decide whether suggesting `alint fix`
    /// makes sense for this rule.
    fixable: bool,
    violations: Vec<JsonViolation<'a>>,
    /// Informational notes (non-violation findings). Omitted entirely
    /// when empty, so results without notes are byte-identical to
    /// pre-v0.11 output.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notes: Vec<JsonViolation<'a>>,
}

#[derive(Serialize)]
struct JsonViolation<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<&'a Path>,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
}

pub fn write_json(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    let summary = Summary {
        failing_rules: report.failing_rules(),
        passing_rules: report.passing_rules(),
        total_violations: report.total_violations(),
        has_errors: report.has_errors(),
        has_warnings: report.has_warnings(),
    };
    let results: Vec<JsonResult<'_>> = report
        .results
        .iter()
        .map(|r| JsonResult {
            id: r.rule_id.as_ref(),
            level: r.level,
            passed: r.passed(),
            policy_url: r.policy_url.as_deref(),
            fixable: r.is_fixable,
            violations: r
                .violations
                .iter()
                .map(|v| JsonViolation {
                    path: v.path.as_deref(),
                    message: v.message.as_ref(),
                    line: v.line,
                    column: v.column,
                })
                .collect(),
            notes: r
                .notes
                .iter()
                .map(|v| JsonViolation {
                    path: v.path.as_deref(),
                    message: v.message.as_ref(),
                    line: v.line,
                    column: v.column,
                })
                .collect(),
        })
        .collect();
    let out = JsonReport {
        schema_version: 1,
        summary,
        results,
    };
    serde_json::to_writer_pretty(&mut *w, &out)?;
    writeln!(w)?;
    Ok(())
}

#[derive(Serialize)]
struct JsonFixReport<'a> {
    schema_version: u32,
    summary: FixSummary,
    results: Vec<JsonFixRuleResult<'a>>,
}

#[derive(Serialize)]
struct FixSummary {
    applied: usize,
    skipped: usize,
    unfixable: usize,
}

#[derive(Serialize)]
struct JsonFixRuleResult<'a> {
    id: &'a str,
    level: Level,
    items: Vec<JsonFixItem<'a>>,
}

#[derive(Serialize)]
struct JsonFixItem<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<&'a Path>,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a str>,
}

pub fn write_fix_json(report: &FixReport, w: &mut dyn Write) -> std::io::Result<()> {
    let results: Vec<JsonFixRuleResult<'_>> = report
        .results
        .iter()
        .map(|r| JsonFixRuleResult {
            id: r.rule_id.as_ref(),
            level: r.level,
            items: r
                .items
                .iter()
                .map(|it| {
                    let (status, detail) = match &it.status {
                        FixStatus::Applied(s) => ("applied", Some(s.as_str())),
                        FixStatus::Skipped(s) => ("skipped", Some(s.as_str())),
                        FixStatus::Unfixable => ("unfixable", None),
                    };
                    JsonFixItem {
                        path: it.violation.path.as_deref(),
                        message: it.violation.message.as_ref(),
                        line: it.violation.line,
                        column: it.violation.column,
                        status,
                        detail,
                    }
                })
                .collect(),
        })
        .collect();
    let out = JsonFixReport {
        schema_version: 1,
        summary: FixSummary {
            applied: report.applied(),
            skipped: report.skipped(),
            unfixable: report.unfixable(),
        },
        results,
    };
    serde_json::to_writer_pretty(&mut *w, &out)?;
    writeln!(w)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{RuleResult, Violation};

    fn render(report: &Report) -> String {
        let mut buf = Vec::new();
        write_json(report, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn notes_array_present_and_excluded_from_pass_fail() {
        let r = RuleResult::new(
            "reg".into(),
            Level::Warning,
            None,
            vec![Violation::new("skipped non-literal entry \"${X}\"").as_note()],
            false,
        );
        let out = render(&Report { results: vec![r] });
        assert!(
            out.contains("\"notes\""),
            "notes array should appear: {out}"
        );
        assert!(out.contains("skipped non-literal"), "{out}");
        // A note never counts as a violation.
        assert!(out.contains("\"passed\": true"), "{out}");
        assert!(out.contains("\"total_violations\": 0"), "{out}");
    }

    #[test]
    fn notes_omitted_when_empty() {
        let r = RuleResult::new(
            "v".into(),
            Level::Error,
            None,
            vec![Violation::new("real")],
            false,
        );
        let out = render(&Report { results: vec![r] });
        assert!(
            !out.contains("\"notes\""),
            "empty notes must be omitted: {out}"
        );
    }
}
