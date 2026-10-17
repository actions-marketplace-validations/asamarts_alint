//! `spdx_identifier_valid` — validate SPDX license identifier tags against the
//! bundled SPDX License List and strict expression grammar.

use std::path::Path;
use std::sync::Arc;

use alint_core::{
    Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, SourceRange, Violation,
    eval_per_file,
};

const MARKER: &str = "SPDX-License-Identifier:";

#[derive(Debug)]
pub struct SpdxIdentifierValidRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for SpdxIdentifierValidRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for SpdxIdentifierValidRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        if crate::io::looks_binary(bytes) {
            return Ok(Vec::new());
        }
        let source_is_utf8 = std::str::from_utf8(bytes).is_ok();
        let text = String::from_utf8_lossy(bytes);
        let mut violations = Vec::new();
        for tag in tags(&text) {
            let problem = match spdx::Expression::parse(tag.expression) {
                Ok(expression) => deprecated_term(&expression)
                    .map(|term| format!("SPDX expression uses deprecated identifier `{term}`")),
                Err(error) => Some(format!("invalid SPDX license expression: {error}")),
            };
            let Some(problem) = problem else {
                continue;
            };
            let message = self.message.clone().unwrap_or(problem);
            let mut violation = Violation::new(message)
                .with_path(Arc::<Path>::from(path))
                .with_baseline_key(format!("spdx\0{}", tag.expression));
            if source_is_utf8 {
                if let Some(range) = SourceRange::from_byte_range(&text, tag.range) {
                    violation = violation.with_range(range);
                }
            } else {
                // Lossy decoding changes byte offsets, so preserve the reliable
                // line/column instead of publishing an inaccurate byte range.
                violation = violation.with_location(tag.line, tag.column);
            }
            violations.push(violation);
        }
        Ok(violations)
    }
}

fn deprecated_term(expression: &spdx::Expression) -> Option<&'static str> {
    for requirement in expression.requirements() {
        if let Some(id) = requirement.req.license.id()
            && id.is_deprecated()
        {
            return Some(id.name);
        }
        if let Some(id) = requirement
            .req
            .addition
            .as_ref()
            .and_then(spdx::AdditionItem::id)
            && id.is_deprecated()
        {
            return Some(id.name);
        }
    }
    None
}

#[derive(Debug, PartialEq, Eq)]
struct Tag<'a> {
    expression: &'a str,
    range: std::ops::Range<usize>,
    line: usize,
    column: usize,
}

fn tags(text: &str) -> Vec<Tag<'_>> {
    let mut found = Vec::new();
    let mut line_start = 0usize;
    for (line_index, raw_line) in text.split_inclusive('\n').enumerate() {
        let line = raw_line
            .strip_suffix('\n')
            .unwrap_or(raw_line)
            .strip_suffix('\r')
            .unwrap_or_else(|| raw_line.strip_suffix('\n').unwrap_or(raw_line));
        let Some(marker_start) = line.find(MARKER) else {
            line_start += raw_line.len();
            continue;
        };
        let after_marker = marker_start + MARKER.len();
        let value = &line[after_marker..];
        let leading = value.len() - value.trim_start().len();
        let mut expression_end = line.len();
        while expression_end > after_marker + leading
            && line.as_bytes()[expression_end - 1].is_ascii_whitespace()
        {
            expression_end -= 1;
        }
        for closer in ["-->", "*/", "*)"] {
            if line[..expression_end].ends_with(closer) {
                expression_end -= closer.len();
                while expression_end > after_marker + leading
                    && line.as_bytes()[expression_end - 1].is_ascii_whitespace()
                {
                    expression_end -= 1;
                }
                break;
            }
        }
        let expression_start = after_marker + leading;
        let expression = &line[expression_start..expression_end];
        found.push(Tag {
            expression,
            range: line_start + expression_start..line_start + expression_end,
            line: line_index + 1,
            column: line[..expression_start].chars().count() + 1,
        });
        line_start += raw_line.len();
    }
    found
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
            "spdx_identifier_valid requires a `paths` field",
        ));
    }
    Ok(Box::new(SpdxIdentifierValidRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    #[test]
    fn strict_expressions_custom_refs_and_comment_closers_pass() {
        for text in [
            "// SPDX-License-Identifier: MIT OR Apache-2.0\n",
            "/* SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception */\n",
            "<!-- SPDX-License-Identifier: LicenseRef-Internal -->\n",
            "# SPDX-License-Identifier: DocumentRef-upstream:LicenseRef-Custom\n",
        ] {
            let expression = tags(text)[0].expression;
            assert!(spdx::Expression::parse(expression).is_ok(), "{expression}");
        }
        for expression in [
            "(MIT OR Apache-2.0) AND BSD-3-Clause",
            "MIT WITH LLVM-exception",
            "MIT and Apache-2.0",
        ] {
            assert!(spdx::Expression::parse(expression).is_ok(), "{expression}");
        }
        for expression in ["MIT/Apache-2.0", "MIT WITH Nope", "(MIT OR Apache-2.0"] {
            assert!(spdx::Expression::parse(expression).is_err(), "{expression}");
        }
    }

    #[test]
    fn unknown_malformed_and_deprecated_identifiers_fire() {
        let rule = build(&spec_yaml(
            "id: spdx\nkind: spdx_identifier_valid\npaths: '**/*'\nlevel: error\n",
        ))
        .unwrap();
        let (tmp, index) = tempdir_with_files(&[
            ("unknown.rs", b"// SPDX-License-Identifier: M1T\n"),
            ("malformed.rs", b"// SPDX-License-Identifier: MIT OR\n"),
            ("deprecated.rs", b"// SPDX-License-Identifier: GPL-2.0\n"),
            (
                "valid.rs",
                b"// SPDX-License-Identifier: MIT AND Apache-2.0\n",
            ),
        ]);
        let violations = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 3);
        assert!(violations.iter().all(|violation| violation.range.is_some()));
        assert!(
            violations
                .iter()
                .any(|violation| violation.message.contains("deprecated"))
        );
    }

    #[test]
    fn no_tag_and_binary_files_are_silent() {
        let rule = build(&spec_yaml(
            "id: spdx\nkind: spdx_identifier_valid\npaths: '**/*'\nlevel: error\n",
        ))
        .unwrap();
        let (tmp, index) = tempdir_with_files(&[
            ("plain.rs", b"fn main() {}\n"),
            ("blob", b"\0SPDX-License-Identifier: NOPE\n"),
        ]);
        assert!(rule.evaluate(&ctx(tmp.path(), &index)).unwrap().is_empty());
    }

    #[test]
    fn bundled_tables_have_current_identifiers() {
        assert_eq!(spdx::identifiers::VERSION, "3.29.0");
        assert!(spdx::license_id("MIT").is_some());
        assert!(spdx::exception_id("LLVM-exception").is_some());
    }

    #[test]
    fn bundled_tables_parse_every_current_identifier_and_exception() {
        for license in spdx::identifiers::LICENSES {
            let id = spdx::license_id(license.name)
                .unwrap_or_else(|| panic!("bundled license lookup failed: {}", license.name));
            if !id.is_deprecated() {
                assert!(
                    spdx::Expression::parse(license.name).is_ok(),
                    "current bundled license did not parse: {}",
                    license.name
                );
            }
        }
        for exception in spdx::identifiers::EXCEPTIONS {
            let id = spdx::exception_id(exception.name)
                .unwrap_or_else(|| panic!("bundled exception lookup failed: {}", exception.name));
            if !id.is_deprecated() {
                let expression = format!("MIT WITH {}", exception.name);
                assert!(
                    spdx::Expression::parse(&expression).is_ok(),
                    "current bundled exception did not parse: {}",
                    exception.name
                );
            }
        }
    }
}
