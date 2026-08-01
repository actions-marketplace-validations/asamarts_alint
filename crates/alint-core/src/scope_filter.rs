//! `ScopeFilter` — per-file rule gate that scopes rule
//! application to files whose ancestor directories contain a
//! named manifest. The "closest-ancestor manifest" pattern, in
//! practical terms.
//!
//! Composes with the existing per-file `paths:` glob and the
//! tree-level `when:` gate as an AND. A file matches a rule
//! iff every gate it has accepts the file.
//!
//! ## Why
//!
//! Bundled ecosystem rulesets (`rust@v1`, `node@v1`, …) need
//! to scope per-file rules to only files inside a package of
//! the matching ecosystem. A `**/*.rs` glob alone is too
//! broad: in a polyglot monorepo, `services/web/scripts/
//! migrate.rs` shouldn't be governed by Rust hygiene rules
//! just because it has the `.rs` extension. With
//! `scope_filter: { has_ancestor: Cargo.toml }`, the rule
//! only fires on files that have a `Cargo.toml` somewhere in
//! their ancestor chain — i.e., files inside an actual Rust
//! package.
//!
//! ## Semantics
//!
//! For a file at `crates/api/src/main.rs`, `has_ancestor:
//! Cargo.toml` walks the ancestor chain `crates/api/src/`,
//! `crates/api/`, `crates/`, root, and returns true on the
//! first match. The walk includes the file's own directory:
//! `crates/api/Cargo.toml` itself matches because
//! `crates/api/` (the file's parent) contains a `Cargo.toml`.
//!
//! See `docs/design/v0.9/scope-filter.md` for full design,
//! pinned decisions, and the bundled-ruleset migration plan.
//!
//! ## Performance
//!
//! Each `has_ancestor` check walks `Path::parent()` upward
//! and consults [`FileIndex::contains_file`] (the v0.9.5
//! path-index) at each step. Both operations are O(1)
//! hashlookups; per-file overhead is `O(depth × M)` where
//! `M` is the number of names in the `has_ancestor` list.
//! Typical: 5 levels × 1 manifest = 150 ns / file. At 1M
//! files × 5 rules with `scope_filter`, total overhead is
//! ~750 ms — and that's before the file-read savings the
//! filter unlocks.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer};

use crate::error::{Error, Result};
use crate::walker::FileIndex;

/// Per-file rule gate. Today's only primitive is
/// `has_ancestor`; the type is an enum-shape struct so future
/// primitives (`closest_ancestor_with_content`, etc.) can land
/// without breaking the public surface.
///
/// Build with [`ScopeFilter::from_spec`] to get the
/// build-time validation (rejects globs, separators, empty
/// lists). Direct construction is allowed for tests via
/// [`ScopeFilter::has_ancestor_unchecked`].
#[derive(Debug, Clone)]
pub struct ScopeFilter {
    has_ancestor: Vec<PathBuf>,
    /// `changed_since: <ref>` — when set, the file must also be in the
    /// `<ref>...HEAD` diff (resolved once per run, cached on the
    /// [`FileIndex`]). AND-composes with `has_ancestor`. Empty
    /// `has_ancestor` + `Some` `changed_since` is a diff-only filter.
    changed_since: Option<String>,
}

/// YAML-level shape of `scope_filter:`. Deserialised by
/// [`RuleSpec`](crate::config::RuleSpec) and validated into
/// the runtime [`ScopeFilter`] via
/// [`ScopeFilter::from_spec`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeFilterSpec {
    /// Single literal filename or non-empty list of literal
    /// filenames. Each must be a basename (no path separator,
    /// no glob metacharacters). Optional since v0.11 — a
    /// `scope_filter:` with only `changed_since:` is valid.
    #[serde(default, deserialize_with = "deserialize_opt_string_or_list")]
    pub has_ancestor: Option<Vec<String>>,
    /// `changed_since: <git-ref>` — narrow the rule to files in the
    /// `<ref>...HEAD` diff. Accepts the `{{env.X}}` interpolation
    /// (resolved at config load). At least one of `has_ancestor:` /
    /// `changed_since:` must be present.
    #[serde(default)]
    pub changed_since: Option<String>,
}

fn deserialize_opt_string_or_list<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged, expecting = "a string, or a list of strings")]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(s) => Ok(Some(vec![s])),
        OneOrMany::Many(v) => Ok(Some(v)),
    }
}

impl ScopeFilter {
    /// Build from the deserialised spec, validating every
    /// `has_ancestor` entry. Returns `Error::rule_config` on
    /// any of:
    ///
    /// - empty list
    /// - empty string
    /// - string contains a path separator (`/` or `\`)
    /// - string contains a glob metacharacter
    ///   (`* ? [ ] { } !`)
    pub fn from_spec(rule_id: &str, spec: ScopeFilterSpec) -> Result<Self> {
        let has_ancestor = match spec.has_ancestor {
            Some(names) => {
                if names.is_empty() {
                    return Err(Error::rule_config(
                        rule_id,
                        "scope_filter.has_ancestor must be a non-empty list",
                    ));
                }
                let mut paths = Vec::with_capacity(names.len());
                for name in names {
                    validate_manifest_name(rule_id, &name)?;
                    paths.push(PathBuf::from(name));
                }
                paths
            }
            None => Vec::new(),
        };
        if has_ancestor.is_empty() && spec.changed_since.is_none() {
            return Err(Error::rule_config(
                rule_id,
                "scope_filter must set at least one of `has_ancestor:` or `changed_since:`",
            ));
        }
        Ok(Self {
            has_ancestor,
            changed_since: spec.changed_since,
        })
    }

    /// Direct construction without validation. Tests only.
    #[doc(hidden)]
    pub fn has_ancestor_unchecked(names: Vec<&str>) -> Self {
        Self {
            has_ancestor: names.into_iter().map(PathBuf::from).collect(),
            changed_since: None,
        }
    }

    /// Direct construction of a diff-only filter. Tests only.
    #[doc(hidden)]
    pub fn changed_since_unchecked(since: &str) -> Self {
        Self {
            has_ancestor: Vec::new(),
            changed_since: Some(since.to_string()),
        }
    }

    /// The configured `changed_since:` ref, if any. The engine reads
    /// this from every per-file rule to know which diffs to resolve.
    #[must_use]
    pub fn changed_since(&self) -> Option<&str> {
        self.changed_since.as_deref()
    }

    /// True iff at least one of the configured ancestor
    /// names exists as a file in some ancestor directory of
    /// `file` — including the file's own directory.
    ///
    /// Walks `Path::parent()` upward from the file, joins the
    /// candidate ancestor name to each directory, and consults
    /// `index.contains_file(...)`. First match wins; the
    /// matching ancestor's path is not exposed (this is a
    /// boolean filter).
    pub fn matches(&self, file: &Path, index: &FileIndex) -> bool {
        if !self.has_ancestor.is_empty() && !self.ancestor_matches(file, index) {
            return false;
        }
        if let Some(since) = &self.changed_since {
            // The diff set is resolved once per run and cached on the
            // index; a missing entry (ref the engine didn't resolve, or
            // a no-git repo) matches nothing — the documented silent
            // no-op.
            let in_diff = index
                .changed_paths(since)
                .is_some_and(|set| set.contains(file));
            if !in_diff {
                return false;
            }
        }
        true
    }

    /// The `has_ancestor` walk, factored out so [`matches`](Self::matches)
    /// can AND it with `changed_since`.
    fn ancestor_matches(&self, file: &Path, index: &FileIndex) -> bool {
        let mut cur = file.parent();
        loop {
            let dir = cur.unwrap_or_else(|| Path::new(""));
            for name in &self.has_ancestor {
                let candidate = dir.join(name);
                if index.contains_file(&candidate) {
                    return true;
                }
            }
            match cur {
                Some(p) if p.as_os_str().is_empty() => return false,
                Some(p) => cur = p.parent(),
                None => return false,
            }
        }
    }

    /// The configured ancestor names, for diagnostics and
    /// audits (e.g.
    /// `coverage_audit_scope_filter.rs`).
    pub fn has_ancestor_names(&self) -> &[PathBuf] {
        &self.has_ancestor
    }
}

/// Build-time guard for cross-file rule builders. Cross-file
/// rules express ancestor scoping through `for_each_dir +
/// when_iter:` instead of `scope_filter:`; the engine consults
/// the per-file dispatch path's `Scope::matches` (which folds
/// in `scope_filter` since v0.9.10), so a cross-file rule with
/// `scope_filter:` set would silently ignore the field. This
/// helper produces a clear build-time error so the
/// misconfiguration surfaces at config-load time rather than
/// as a confused-rule-doesn't-fire bug.
///
/// Usage in a cross-file rule builder:
///
/// ```ignore
/// pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
///     reject_scope_filter_on_cross_file(spec, "for_each_dir")?;
///     // …
/// }
/// ```
pub fn reject_scope_filter_on_cross_file(
    spec: &crate::config::RuleSpec,
    cross_file_kind_label: &str,
) -> Result<()> {
    if spec.scope_filter.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            format!(
                "scope_filter is supported on per-file rules only; {cross_file_kind_label} is a \
                 cross-file rule. Express ancestor scoping via `for_each_dir + when_iter:` \
                 instead — see docs/design/v0.9/scope-filter.md for the pattern."
            ),
        ));
    }
    Ok(())
}

/// Build-time guard for rules whose evaluation target is fixed
/// (a hardcoded path or a tree-level invariant), making
/// `scope_filter:` semantically meaningless. Sister helper to
/// [`reject_scope_filter_on_cross_file`]; used by rules like
/// `no_submodules` (hardcoded to `.gitmodules` at the repo
/// root) where the user-supplied filter has nothing to scope.
///
/// `reason` is the user-facing why-can't-I-use-it: it gets
/// inlined into the error message after `"...scope_filter is not
/// supported on <rule_kind>; "`. Keep it as a single sentence
/// fragment that completes that lead. Example: `"this rule is
/// hardcoded to check `.gitmodules` at the repository root"`.
///
/// Usage in a rule builder:
///
/// ```ignore
/// pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
///     reject_scope_filter_with_reason(
///         spec,
///         "no_submodules",
///         "this rule is hardcoded to check `.gitmodules` at the repository root",
///     )?;
///     // …
/// }
/// ```
pub fn reject_scope_filter_with_reason(
    spec: &crate::config::RuleSpec,
    rule_kind: &str,
    reason: &str,
) -> Result<()> {
    if spec.scope_filter.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            format!("scope_filter is not supported on {rule_kind}; {reason}"),
        ));
    }
    Ok(())
}

fn validate_manifest_name(rule_id: &str, name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::rule_config(
            rule_id,
            "scope_filter.has_ancestor names must not be empty",
        ));
    }
    if name.contains('/') || name.contains('\\') {
        // Pitfall #11 in `docs/development/CONFIG-AUTHORING.md`: the
        // most common adopter mistake is reaching for `has_ancestor`
        // to scope by directory (e.g. `airflow-core/pyproject.toml`),
        // when the right answer is a `paths:` glob on the rule's
        // main scope. Surface that distinction in the error message.
        let basename = name.rsplit(['/', '\\']).next().unwrap_or(name);
        return Err(Error::rule_config(
            rule_id,
            format!(
                "scope_filter.has_ancestor name {name:?} must be a basename — no path separators.\n  \
                 hint: to match files inside a specific subtree, use `paths:` on the rule's main \
                 scope (e.g. `paths: \"airflow-core/**/*.py\"`); to match files in any subtree \
                 that has this manifest, use the basename only (e.g. `has_ancestor: {basename:?}`)."
            ),
        ));
    }
    if name
        .chars()
        .any(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '!'))
    {
        return Err(Error::rule_config(
            rule_id,
            format!(
                "scope_filter.has_ancestor name {name:?} must be a literal — no glob \
                 metacharacters allowed (use `Cargo.toml`, not `*.toml`)"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walker::{FileEntry, FileIndex};
    use std::path::Path;
    use std::sync::Arc;

    fn idx(paths: &[&str]) -> FileIndex {
        FileIndex::from_entries(
            paths
                .iter()
                .map(|p| FileEntry {
                    path: Arc::<Path>::from(Path::new(p)),
                    is_dir: false,
                    size: 0,
                })
                .collect(),
        )
    }

    fn filter(names: Vec<&str>) -> ScopeFilter {
        ScopeFilter::has_ancestor_unchecked(names)
    }

    #[test]
    fn root_manifest_matches_root_file() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["Cargo.toml", "lib.rs"]);
        assert!(f.matches(Path::new("lib.rs"), &i));
    }

    #[test]
    fn root_manifest_matches_nested_file() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["Cargo.toml", "src/lib.rs"]);
        assert!(f.matches(Path::new("src/lib.rs"), &i));
    }

    #[test]
    fn nested_manifest_matches_own_dir() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["crates/api/Cargo.toml", "crates/api/src/main.rs"]);
        // Manifest at crates/api/ — main.rs's ancestor.
        assert!(f.matches(Path::new("crates/api/src/main.rs"), &i));
    }

    #[test]
    fn manifest_at_files_own_dir_matches_the_manifest_itself() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["crates/api/Cargo.toml"]);
        // `Cargo.toml` is in the file's own dir → match.
        assert!(f.matches(Path::new("crates/api/Cargo.toml"), &i));
    }

    #[test]
    fn root_cargo_toml_matches_itself() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["Cargo.toml"]);
        assert!(f.matches(Path::new("Cargo.toml"), &i));
    }

    #[test]
    fn no_manifest_in_any_ancestor_returns_false() {
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&["src/lib.rs"]);
        assert!(!f.matches(Path::new("src/lib.rs"), &i));
    }

    #[test]
    fn sibling_manifest_does_not_match() {
        let f = filter(vec!["Cargo.toml"]);
        // Sibling has Cargo.toml, but our file is in a different subtree.
        let i = idx(&["crates/api/Cargo.toml", "services/web/src/index.ts"]);
        assert!(!f.matches(Path::new("services/web/src/index.ts"), &i));
    }

    #[test]
    fn two_name_list_matches_if_either_found() {
        let f = filter(vec!["pyproject.toml", "setup.py"]);
        let i = idx(&["app/setup.py", "app/main.py"]);
        assert!(f.matches(Path::new("app/main.py"), &i));
    }

    #[test]
    fn closest_ancestor_among_multiple() {
        // Both root and crates/api have Cargo.toml. Either match.
        let f = filter(vec!["Cargo.toml"]);
        let i = idx(&[
            "Cargo.toml",
            "crates/api/Cargo.toml",
            "crates/api/src/main.rs",
        ]);
        assert!(f.matches(Path::new("crates/api/src/main.rs"), &i));
    }

    // ── from_spec validation ──────────────────────────────────

    #[test]
    fn from_spec_rejects_empty_list() {
        let err = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: Some(vec![]),
                changed_since: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("non-empty"), "msg: {err}");
    }

    #[test]
    fn from_spec_rejects_empty_string() {
        let err = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: Some(vec![String::new()]),
                changed_since: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("not be empty"), "msg: {err}");
    }

    #[test]
    fn from_spec_rejects_path_separator() {
        let err = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: Some(vec!["foo/bar".into()]),
                changed_since: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("path separators"), "msg: {err}");
    }

    #[test]
    fn from_spec_rejects_glob_metacharacters() {
        for bad in &["*.toml", "Cargo?", "[abc]", "{a,b}", "!Cargo"] {
            let err = ScopeFilter::from_spec(
                "r",
                ScopeFilterSpec {
                    has_ancestor: Some(vec![(*bad).into()]),
                    changed_since: None,
                },
            )
            .unwrap_err();
            assert!(err.to_string().contains("glob"), "msg for {bad:?}: {err}");
        }
    }

    #[test]
    fn from_spec_accepts_canonical_manifests() {
        for good in &[
            "Cargo.toml",
            "package.json",
            "pyproject.toml",
            "setup.py",
            "go.mod",
            "pom.xml",
            "build.gradle",
            "build.gradle.kts",
        ] {
            ScopeFilter::from_spec(
                "r",
                ScopeFilterSpec {
                    has_ancestor: Some(vec![(*good).into()]),
                    changed_since: None,
                },
            )
            .unwrap_or_else(|e| panic!("{good:?} should be valid; got {e}"));
        }
    }

    // ── deserialise OneOrMany ─────────────────────────────────

    #[test]
    fn deserialize_single_string_form() {
        let yaml = "has_ancestor: Cargo.toml\n";
        let spec: ScopeFilterSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.has_ancestor, Some(vec!["Cargo.toml".to_string()]));
        assert_eq!(spec.changed_since, None);
    }

    #[test]
    fn deserialize_changed_since_only_form() {
        let yaml = "changed_since: origin/main\n";
        let spec: ScopeFilterSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.has_ancestor, None);
        assert_eq!(spec.changed_since.as_deref(), Some("origin/main"));
    }

    #[test]
    fn deserialize_list_form() {
        let yaml = "has_ancestor:\n  - pom.xml\n  - build.gradle\n";
        let spec: ScopeFilterSpec = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(
            spec.has_ancestor,
            Some(vec!["pom.xml".to_string(), "build.gradle".to_string()]),
        );
    }

    #[test]
    fn deserialize_rejects_unknown_field() {
        let yaml = "has_ancestor: Cargo.toml\nunknown: x\n";
        assert!(serde_yaml_ng::from_str::<ScopeFilterSpec>(yaml).is_err());
    }

    // ── changed_since ─────────────────────────────────────────

    fn idx_with_diff(paths: &[&str], since: &str, diff: &[&str]) -> FileIndex {
        let i = idx(paths);
        let mut map = std::collections::HashMap::new();
        map.insert(since.to_string(), diff.iter().map(PathBuf::from).collect());
        i.set_changed_paths(map);
        i
    }

    #[test]
    fn changed_since_matches_only_files_in_diff() {
        let f = ScopeFilter::changed_since_unchecked("origin/main");
        let i = idx_with_diff(&["src/a.rs", "src/b.rs"], "origin/main", &["src/a.rs"]);
        assert!(f.matches(Path::new("src/a.rs"), &i), "in-diff file matches");
        assert!(
            !f.matches(Path::new("src/b.rs"), &i),
            "out-of-diff file skipped"
        );
    }

    #[test]
    fn changed_since_with_unpopulated_cache_matches_nothing() {
        // No git / unresolved ref → empty/absent cache → silent no-op.
        let f = ScopeFilter::changed_since_unchecked("origin/main");
        let i = idx(&["src/a.rs"]);
        assert!(!f.matches(Path::new("src/a.rs"), &i));
    }

    #[test]
    fn changed_since_and_composes_with_has_ancestor() {
        // Both gates must hold. a.rs is in the diff AND under a
        // Cargo.toml; b.rs is in the diff but has no ancestor manifest.
        let f = ScopeFilter {
            has_ancestor: vec![PathBuf::from("Cargo.toml")],
            changed_since: Some("origin/main".to_string()),
        };
        let i = idx_with_diff(
            &["crates/x/Cargo.toml", "crates/x/a.rs", "loose/b.rs"],
            "origin/main",
            &["crates/x/a.rs", "loose/b.rs"],
        );
        assert!(f.matches(Path::new("crates/x/a.rs"), &i));
        assert!(
            !f.matches(Path::new("loose/b.rs"), &i),
            "no ancestor manifest"
        );
    }

    #[test]
    fn from_spec_rejects_neither_field() {
        let err = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: None,
                changed_since: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("at least one"), "msg: {err}");
    }

    #[test]
    fn from_spec_accepts_changed_since_only() {
        let f = ScopeFilter::from_spec(
            "r",
            ScopeFilterSpec {
                has_ancestor: None,
                changed_since: Some("origin/main".into()),
            },
        )
        .unwrap();
        assert_eq!(f.changed_since(), Some("origin/main"));
        assert!(f.has_ancestor_names().is_empty());
    }
}
