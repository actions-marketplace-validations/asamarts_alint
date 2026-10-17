//! `markdown_no_duplicate_headings` — reject headings whose generated base
//! anchors collide.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use alint_core::{
    AnalysisFailureKind, Context, Error, Level, PerFileRule, RelatedLocation, Result, Rule,
    RuleSpec, Scope, SourceRange, Violation, eval_per_file,
};

#[derive(Debug)]
pub struct MarkdownNoDuplicateHeadingsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for MarkdownNoDuplicateHeadingsRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for MarkdownNoDuplicateHeadingsRule {
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
        let mut first_by_anchor: HashMap<String, usize> = HashMap::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut violations = Vec::new();

        for (index, heading) in headings.iter().enumerate() {
            let anchor = crate::markdown::heading_base_anchor(heading);
            let Some(&first_index) = first_by_anchor.get(&anchor) else {
                first_by_anchor.insert(anchor.clone(), index);
                counts.insert(anchor, 1);
                continue;
            };
            let occurrence = counts.entry(anchor.clone()).or_insert(1);
            *occurrence += 1;
            let message = self.message.clone().unwrap_or_else(|| {
                format!("Markdown heading generates duplicate anchor `#{anchor}`")
            });
            let mut violation = Violation::new(message)
                .with_path(Arc::<Path>::from(path))
                .with_baseline_key(format!("{anchor}\0{}", *occurrence));
            if let Some(range) = SourceRange::from_byte_range(text, heading.span.clone()) {
                violation = violation.with_range(range);
            }
            let first = &headings[first_index];
            let mut related = RelatedLocation::new(path, "first heading with this anchor");
            if let Some(range) = SourceRange::from_byte_range(text, first.span.clone()) {
                related = related.with_range(range);
            }
            violations.push(violation.with_related_location(related));
        }
        Ok(violations)
    }
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
    nested_rules: false,
};

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    if spec.paths.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "markdown_no_duplicate_headings requires a `paths` field",
        ));
    }
    Ok(Box::new(MarkdownNoDuplicateHeadingsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
    }))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    #[test]
    fn reports_case_and_punctuation_anchor_collisions_with_first_location() {
        let rule = super::build(&spec_yaml(
            "id: duplicates\nkind: markdown_no_duplicate_headings\npaths: '*.md'\nlevel: error\n",
        ))
        .unwrap();
        let text =
            b"# One\n## Hello, World!\n## HELLO WORLD\n## Distinct {#custom}\n## Other {#custom}\n";
        let (tmp, index) = tempdir_with_files(&[("a.md", text)]);
        let violations = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0].line, Some(3));
        assert_eq!(
            violations[0].related_locations[0]
                .range
                .as_ref()
                .unwrap()
                .start_line(),
            2
        );
        assert_eq!(violations[1].line, Some(5));
        assert_eq!(violations[1].path.as_deref(), Some(Path::new("a.md")));
    }

    #[test]
    fn repeated_raw_text_with_distinct_explicit_ids_passes() {
        let rule = super::build(&spec_yaml(
            "id: duplicates\nkind: markdown_no_duplicate_headings\npaths: '*.md'\nlevel: error\n",
        ))
        .unwrap();
        let (tmp, index) =
            tempdir_with_files(&[("a.md", b"## Same {#first}\n## Same {#second}\n")]);
        assert!(rule.evaluate(&ctx(tmp.path(), &index)).unwrap().is_empty());
    }
}
