//! `markdown_required_headings` — require a complete ordered Markdown heading
//! template with `*` as a zero-or-more wildcard.

use std::path::Path;
use std::sync::Arc;

use alint_core::{
    AnalysisFailureKind, Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope,
    SourceRange, Violation, eval_per_file,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Exact rendered headings (`# Text` through `###### Text`) in document
    /// order. `*` matches zero or more complete headings.
    #[schemars(length(min = 1))]
    headings: Vec<String>,
}

crate::options_schema_for!(Options);

#[derive(Debug, Clone, PartialEq, Eq)]
enum RequiredHeading {
    Any,
    Exact { level: u8, text: String },
}

#[derive(Debug)]
pub struct MarkdownRequiredHeadingsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    headings: Vec<RequiredHeading>,
}

impl Rule for MarkdownRequiredHeadingsRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for MarkdownRequiredHeadingsRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(vec![
                Violation::new("Markdown file is not valid UTF-8")
                    .with_path(Arc::<Path>::from(path))
                    .as_analysis_failure(AnalysisFailureKind::ResourceParse),
            ]);
        };
        let headings = crate::markdown::headings(text);
        if heading_sequence_matches(&self.headings, &headings) {
            return Ok(Vec::new());
        }

        let message = self.message.clone().unwrap_or_else(|| {
            "Markdown heading sequence does not match the required template".into()
        });
        let mut violation = Violation::new(message)
            .with_path(Arc::<Path>::from(path))
            .with_baseline_key("required-headings");
        if let Some(first) = headings.first()
            && let Some(range) = SourceRange::from_byte_range(text, first.span.clone())
        {
            violation = violation.with_range(range);
        }
        Ok(vec![violation])
    }
}

fn heading_sequence_matches(
    required: &[RequiredHeading],
    actual: &[crate::markdown::MarkdownHeading],
) -> bool {
    // Standard glob matching with one backtracking cursor. `*` is the only
    // wildcard, so this is linear-space and avoids a pattern × document matrix.
    let mut required_index = 0usize;
    let mut actual_index = 0usize;
    let mut wildcard_index = None;
    let mut wildcard_match = 0usize;

    while actual_index < actual.len() {
        let exact_matches = matches!(
            required.get(required_index),
            Some(RequiredHeading::Exact { level, text })
                if actual[actual_index].level == *level && actual[actual_index].text == *text
        );
        if exact_matches {
            required_index += 1;
            actual_index += 1;
        } else if matches!(required.get(required_index), Some(RequiredHeading::Any)) {
            wildcard_index = Some(required_index);
            required_index += 1;
            wildcard_match = actual_index;
        } else if let Some(wildcard) = wildcard_index {
            wildcard_match += 1;
            actual_index = wildcard_match;
            required_index = wildcard + 1;
        } else {
            return false;
        }
    }

    while matches!(required.get(required_index), Some(RequiredHeading::Any)) {
        required_index += 1;
    }
    required_index == required.len()
}

fn parse_required_headings(
    values: Vec<String>,
) -> std::result::Result<Vec<RequiredHeading>, String> {
    if values.is_empty() {
        return Err("`headings` must contain at least one entry".into());
    }
    let mut parsed = Vec::with_capacity(values.len());
    for (index, value) in values.into_iter().enumerate() {
        if value == "*" {
            if matches!(parsed.last(), Some(RequiredHeading::Any)) {
                return Err(format!(
                    "`headings` entry {} is a redundant consecutive `*` wildcard",
                    index + 1
                ));
            }
            parsed.push(RequiredHeading::Any);
            continue;
        }
        let hashes = value.bytes().take_while(|byte| *byte == b'#').count();
        if !(1..=6).contains(&hashes)
            || value.as_bytes().get(hashes) != Some(&b' ')
            || value.len() == hashes + 1
        {
            return Err(format!(
                "`headings` entry {} must be `*` or `# Text` through `###### Text`",
                index + 1
            ));
        }
        parsed.push(RequiredHeading::Exact {
            level: u8::try_from(hashes).expect("heading level range checked above"),
            text: value[hashes + 1..].to_string(),
        });
    }
    Ok(parsed)
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
    formats: std::borrow::Cow::Borrowed(&[]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: true,
    trace: alint_core::TraceMeta::SELECTION,
    nested_rules: false,
};

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    if spec.paths.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "markdown_required_headings requires a `paths` field",
        ));
    }
    let options: Options = spec
        .deserialize_options()
        .map_err(|error| Error::rule_config(&spec.id, format!("invalid options: {error}")))?;
    let headings = parse_required_headings(options.headings)
        .map_err(|error| Error::rule_config(&spec.id, error))?;
    Ok(Box::new(MarkdownRequiredHeadingsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        headings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    #[test]
    fn wildcard_matches_zero_or_more_complete_headings() {
        let required =
            parse_required_headings(vec!["# ADR".into(), "*".into(), "## Decision".into()])
                .unwrap();
        let zero = crate::markdown::headings("# ADR\n## Decision\n");
        let many = crate::markdown::headings("# ADR\n## Status\n### Detail\n## Decision\n");
        assert!(heading_sequence_matches(&required, &zero));
        assert!(heading_sequence_matches(&required, &many));
        let wrong = crate::markdown::headings("# ADR\n## Decision\n## Extra\n");
        assert!(!heading_sequence_matches(&required, &wrong));
        let ambiguous_required = parse_required_headings(vec![
            "# ADR".into(),
            "*".into(),
            "## Decision".into(),
            "## Decision".into(),
        ])
        .unwrap();
        let ambiguous = crate::markdown::headings("# ADR\n## Decision\n## Decision\n## Decision\n");
        assert!(heading_sequence_matches(&ambiguous_required, &ambiguous));
    }

    #[test]
    fn rendered_heading_text_and_levels_are_exact() {
        let rule = build(&spec_yaml(
            "id: headings\nkind: markdown_required_headings\npaths: '*.md'\nheadings: ['# A title', '## Decision']\nlevel: error\n",
        ))
        .unwrap();
        let (tmp, index) = tempdir_with_files(&[
            ("ok.md", b"# A *title*\n## Decision\n"),
            ("bad.md", b"# A title\n### Decision\n"),
        ]);
        let violations = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].path.as_deref(), Some(Path::new("bad.md")));
    }

    #[test]
    fn invalid_templates_fail_at_load() {
        for headings in ["[]", "['Decision']", "['*', '*']"] {
            let spec = spec_yaml(&format!(
                "id: h\nkind: markdown_required_headings\npaths: '*.md'\nheadings: {headings}\nlevel: error\n"
            ));
            assert!(build(&spec).is_err(), "accepted {headings}");
        }
    }
}
