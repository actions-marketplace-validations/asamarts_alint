//! `git_commit_subject_matches` — each commit's subject line (the
//! first line of its message) must match a regex.
//!
//! The subject-grammar member of the commit-validation family
//! (`git_commit_signed_off`, `git_commit_no_fixup`, …): enforces a
//! prefix + shape convention like `pkg/path: lowercase summary`
//! (go / Gerrit), `subsystem: description` (node), or
//! conventional-commit types. Unlike `git_commit_message`'s
//! `pattern:` (which matches the whole subject + body), `matches:`
//! is anchored to the **subject alone**, so `^…$` describes the
//! first line exactly. For a subject-length cap use
//! `git_commit_message`'s `subject_max_length:`.
//!
//! Shares the family shape (the `commit_range` module): `since:`
//! unset checks HEAD only; `since:` set checks `<since>..HEAD`,
//! oldest-first, merge commits excluded unless `include_merges:`.
//! Silent outside a git repo / with no commits; a bad `since:` ref
//! hard-fails with a shallow-clone hint. `since:`'s `{{env.X}}`
//! interpolation is resolved at config load by `alint-dsl`.
//!
//! Check-only — alint can't rewrite the user's commit history.

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Violation};
use regex::Regex;
use serde::Deserialize;

use crate::commit_range::{collect_commits, format_commit_violation};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Regex the commit subject (first line of the message) must
    /// match.
    matches: String,
    /// Base ref for range mode. Unset → HEAD only. The canonical
    /// `{{env.X}}` interpolation is resolved at config load.
    #[serde(default)]
    since: Option<String>,
    /// Include merge commits when checking a range. No effect
    /// without `since:`.
    #[serde(default)]
    include_merges: bool,
}

#[derive(Debug)]
pub struct GitCommitSubjectMatchesRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message_override: Option<String>,
    matches: Regex,
    since_raw: Option<String>,
    include_merges: bool,
}

impl Rule for GitCommitSubjectMatchesRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        let commits = collect_commits(
            ctx,
            self.since_raw.as_deref(),
            self.include_merges,
            &self.id,
        )?;
        for commit in &commits {
            let subject = commit.message.split('\n').next().unwrap_or("");
            if !self.matches.is_match(subject) {
                let msg = self.message_override.clone().unwrap_or_else(|| {
                    format_commit_violation(
                        commit,
                        &format!("subject does not match `{}`", self.matches.as_str()),
                    )
                });
                violations.push(Violation::new(msg));
            }
        }
        Ok(violations)
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "git_commit_subject_matches has no fix op",
        ));
    }
    if opts.include_merges && opts.since.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "`include_merges: true` has no effect without `since:`. Either remove it \
             or set `since:` to enable range mode.",
        ));
    }
    let matches = Regex::new(&opts.matches).map_err(|e| {
        Error::rule_config(
            &spec.id,
            format!("invalid `matches:` regex `{}`: {e}", opts.matches),
        )
    })?;

    Ok(Box::new(GitCommitSubjectMatchesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message_override: spec.message.clone(),
        matches,
        since_raw: opts.since,
        include_merges: opts.include_merges,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(toml: &str) -> RuleSpec {
        let mut full = String::from(
            "id = \"subject-grammar\"\nkind = \"git_commit_subject_matches\"\nlevel = \"error\"\n",
        );
        full.push_str(toml);
        toml::from_str(&full).unwrap()
    }

    #[test]
    fn build_accepts_minimal_and_rejects_fix() {
        assert!(build(&spec("matches = \"^[a-z]+: \"\n")).is_ok());
        assert!(
            build(&spec(
                "matches = \"^x\"\nfix = { file_create = { content = \"x\" } }\n"
            ))
            .is_err()
        );
    }

    #[test]
    fn build_requires_matches() {
        // `matches:` is the one required field.
        assert!(build(&spec("")).is_err());
    }

    #[test]
    fn build_rejects_invalid_regex() {
        let err = build(&spec("matches = \"(unclosed\"\n")).unwrap_err();
        assert!(err.to_string().contains("regex"), "{err}");
    }

    #[test]
    fn build_rejects_include_merges_without_since() {
        let err = build(&spec("matches = \"^x\"\ninclude_merges = true\n")).unwrap_err();
        assert!(err.to_string().contains("include_merges"), "{err}");
    }

    #[test]
    fn subject_regex_is_anchored_to_the_first_line() {
        // A conventional grammar matches the subject but not a body
        // line that happens to look like a subject.
        let re = Regex::new(r"^[a-z0-9_/.-]+: [a-z].{0,70}$").unwrap();
        assert!(re.is_match("pkg/net: add a thing"));
        assert!(!re.is_match("WIP: not lowercase enough? Capitalised"));
        // The subject is split on the first newline, so a valid
        // subject with a body still matches against the first line.
        let subject = "feat: ok".split('\n').next().unwrap();
        assert!(re.is_match(subject));
    }
}
