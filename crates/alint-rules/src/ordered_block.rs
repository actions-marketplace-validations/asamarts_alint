//! `ordered_block` — the lines between a `start` / `end` marker
//! pair must stay sorted (optionally unique) under a configurable
//! comparator. The generic form of the per-project `keep-sorted`
//! / `keep_sorted` scripts (protobuf `failure_lists` is the
//! highest-stakes source). Per-file rule (the `PerFileRule` fast
//! path), not cross-file. Design + open-question resolutions:
//! `docs/design/v0.10/ordered_block.md`.
//!
//! ```yaml
//! - id: keep-sorted
//!   kind: ordered_block
//!   paths: ["**/.gitignore", "CODEOWNERS"]
//!   start: "# keep-sorted start"   # matched on the trimmed line
//!   end: "# keep-sorted end"
//!   comparator: lexical            # lexical (default) | lexical-ci | numeric
//!   unique: false                  # also forbid duplicate entries
//!   level: warning
//! ```

use std::cmp::Ordering;
use std::path::Path;

use alint_core::{
    Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope, Violation, eval_per_file,
};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Comparator {
    /// Rust `str` `Ord` — byte-wise over the UTF-8.
    #[default]
    Lexical,
    /// ASCII-case-insensitive lexical.
    LexicalCi,
    /// Leading-integer order; entries without a leading integer
    /// fall back to `lexical` so a mixed block degrades
    /// predictably rather than panicking.
    Numeric,
}

impl Comparator {
    fn order(self, a: &str, b: &str) -> Ordering {
        match self {
            Self::Lexical => a.cmp(b),
            Self::LexicalCi => a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()),
            Self::Numeric => match (leading_int(a), leading_int(b)) {
                (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.cmp(b)),
                _ => a.cmp(b),
            },
        }
    }
}

/// The leading (optionally negative) integer of `s`, or `None`
/// when it doesn't start with one.
fn leading_int(s: &str) -> Option<i64> {
    let s = s.trim_start();
    let b = s.as_bytes();
    let neg = b.first() == Some(&b'-');
    let digits_start = usize::from(neg);
    let digits_end = b[digits_start..]
        .iter()
        .position(|c| !c.is_ascii_digit())
        .map_or(b.len(), |p| digits_start + p);
    if digits_end == digits_start {
        return None;
    }
    s[..digits_end].parse::<i64>().ok()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    start: String,
    end: String,
    #[serde(default)]
    comparator: Comparator,
    #[serde(default)]
    unique: bool,
}

#[derive(Debug)]
pub struct OrderedBlockRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    start: String,
    end: String,
    comparator: Comparator,
    unique: bool,
}

/// In-flight block state while scanning a file.
struct Block {
    start_line: usize,
    prev: Option<String>,
    /// One violation per block: once set, further entries are
    /// skipped until the `end` marker (keeps output actionable).
    reported: bool,
}

impl Rule for OrderedBlockRule {
    alint_core::rule_common_impl!();

    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for OrderedBlockRule {
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
            // Non-UTF-8 is degenerate for a line-sorted region.
            return Ok(Vec::new());
        };
        let mut violations = Vec::new();
        let mut block: Option<Block> = None;

        for (i, raw) in text.lines().enumerate() {
            let line_no = i + 1;
            let trimmed = raw.trim();

            let Some(b) = block.as_mut() else {
                if trimmed == self.start {
                    block = Some(Block {
                        start_line: line_no,
                        prev: None,
                        reported: false,
                    });
                }
                continue;
            };

            if trimmed == self.end {
                block = None;
                continue;
            }
            // Blank lines inside a block are not entries.
            if trimmed.is_empty() || b.reported {
                continue;
            }

            let entry = trimmed.to_string();
            if let Some(prev) = &b.prev {
                let ord = self.comparator.order(&entry, prev);
                if ord == Ordering::Less {
                    violations.push(self.violation(
                        path,
                        line_no,
                        b.start_line,
                        &format!("{entry:?} is out of order (it comes after {prev:?})"),
                    ));
                    b.reported = true;
                } else if self.unique && ord == Ordering::Equal {
                    violations.push(self.violation(
                        path,
                        line_no,
                        b.start_line,
                        &format!("{entry:?} is a duplicate entry"),
                    ));
                    b.reported = true;
                }
            }
            b.prev = Some(entry);
        }

        if let Some(b) = block {
            violations.push(self.violation(
                path,
                b.start_line,
                b.start_line,
                &format!(
                    "unclosed ordered_block — no {:?} line after the start",
                    self.end
                ),
            ));
        }
        Ok(violations)
    }
}

impl OrderedBlockRule {
    fn violation(&self, path: &Path, line: usize, start_line: usize, desc: &str) -> Violation {
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("ordered_block (start at line {start_line}): {desc}"));
        Violation::new(msg)
            .with_path(std::sync::Arc::<Path>::from(path))
            .with_location(line, 1)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    if spec.paths.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "ordered_block requires a `paths` field (the files whose marked blocks to check)",
        ));
    }
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.start.trim().is_empty() || opts.end.trim().is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "ordered_block `start` and `end` marker lines must not be empty",
        ));
    }
    if opts.start.trim() == opts.end.trim() {
        return Err(Error::rule_config(
            &spec.id,
            "ordered_block `start` and `end` markers must differ",
        ));
    }
    Ok(Box::new(OrderedBlockRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        start: opts.start.trim().to_string(),
        end: opts.end.trim().to_string(),
        comparator: opts.comparator,
        unique: opts.unique,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(comparator: Comparator, unique: bool) -> OrderedBlockRule {
        OrderedBlockRule {
            id: "t".into(),
            level: Level::Warning,
            policy_url: None,
            message: None,
            scope: Scope::from_patterns(&["**/*".to_string()]).unwrap(),
            start: "# keep-sorted start".into(),
            end: "# keep-sorted end".into(),
            comparator,
            unique,
        }
    }

    fn eval(r: &OrderedBlockRule, text: &str) -> Vec<Violation> {
        let ctx = Context {
            root: Path::new("/"),
            index: &alint_core::FileIndex::from_entries(Vec::new()),
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        r.evaluate_file(&ctx, Path::new("f.txt"), text.as_bytes())
            .unwrap()
    }

    #[test]
    fn sorted_block_passes() {
        let t = "x\n# keep-sorted start\nalpha\nbravo\ncharlie\n# keep-sorted end\ny\n";
        assert!(eval(&rule(Comparator::Lexical, false), t).is_empty());
    }

    #[test]
    fn unsorted_block_fails_once_at_the_offending_line() {
        let t = "# keep-sorted start\nalpha\ncharlie\nbravo\ndelta\n# keep-sorted end\n";
        let v = eval(&rule(Comparator::Lexical, false), t);
        assert_eq!(v.len(), 1, "one violation per block: {v:?}");
        // `bravo` (line 4) is out of order after `charlie`.
        assert_eq!(v[0].line, Some(4));
        assert!(v[0].message.contains("bravo"));
    }

    #[test]
    fn no_markers_is_silent() {
        let t = "just\nsome\nunsorted\nlines\nz\na\n";
        assert!(eval(&rule(Comparator::Lexical, false), t).is_empty());
    }

    #[test]
    fn unique_flags_duplicate_only_when_set() {
        let t = "# keep-sorted start\nalpha\nalpha\nbravo\n# keep-sorted end\n";
        // Non-decreasing: a duplicate is fine without `unique`.
        assert!(eval(&rule(Comparator::Lexical, false), t).is_empty());
        let v = eval(&rule(Comparator::Lexical, true), t);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("duplicate"));
    }

    #[test]
    fn lexical_ci_and_numeric_comparators() {
        // Bravo < alpha lexically (uppercase), but ci-sorted.
        let ci = "# keep-sorted start\nalpha\nBravo\ncharlie\n# keep-sorted end\n";
        assert!(eval(&rule(Comparator::LexicalCi, false), ci).is_empty());
        // Numeric: "9" before "10" (lexical would flip them).
        let num = "# keep-sorted start\n2\n9\n10\n100\n# keep-sorted end\n";
        assert!(eval(&rule(Comparator::Numeric, false), num).is_empty());
        let bad = "# keep-sorted start\n10\n9\n# keep-sorted end\n";
        assert_eq!(eval(&rule(Comparator::Numeric, false), bad).len(), 1);
    }

    #[test]
    fn multiple_blocks_checked_independently() {
        let t = "# keep-sorted start\na\nb\n# keep-sorted end\nmid\n\
                 # keep-sorted start\nz\nq\n# keep-sorted end\n";
        let v = eval(&rule(Comparator::Lexical, false), t);
        assert_eq!(v.len(), 1, "only the 2nd block (z, q) is unsorted: {v:?}");
    }

    #[test]
    fn unclosed_start_is_a_violation() {
        let t = "before\n# keep-sorted start\nalpha\nbravo\n";
        let v = eval(&rule(Comparator::Lexical, false), t);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("unclosed"));
        assert_eq!(v[0].line, Some(2));
    }

    #[test]
    fn blank_lines_inside_a_block_are_ignored() {
        let t = "# keep-sorted start\nalpha\n\nbravo\n\ncharlie\n# keep-sorted end\n";
        assert!(eval(&rule(Comparator::Lexical, false), t).is_empty());
    }
}
