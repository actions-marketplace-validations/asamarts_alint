//! `cross_file` — a value (or set of values) extracted from one
//! authoritative `source` file must hold a `relation:` to the
//! values extracted from one or more `targets`. The unified
//! cross-file value-relation kind (architecture-synthesis
//! primitive A): one kind, a `relation:` knob, over the shared
//! `crate::extract` + `normalize`. Design + open questions:
//! `docs/design/v0.12/cross_file.md`.
//!
//! `cross_file_value_equals` (v0.10) is a registered **alias** for
//! this kind with `relation` defaulting to `equals`; every existing
//! config is byte-compatible.
//!
//! ```yaml
//! - id: workspace-versions-coherent
//!   kind: cross_file
//!   source:
//!     file: Cargo.toml
//!     extract: { toml: "$.workspace.package.version" }
//!   targets:                       # form (a): glob + one extract
//!     files: "crates/*/Cargo.toml"
//!     extract: { toml: "$.package.version" }
//!   relation: equals               # equals (default) | subset | superset | set_equals
//!   normalize: none                # none (default) | trim | lower | semver-major
//!   allow_missing_target: false
//!   level: error
//!
//! # Set membership — every catalog reference must resolve to a key.
//! - id: pnpm-catalog-refs-resolve
//!   kind: cross_file
//!   source:  { file: pnpm-workspace.yaml, extract: { yaml: "$.catalog.*" } }
//!   targets: { files: "packages/**/package.json", extract: { regex: 'catalog:(\S+)' } }
//!   relation: subset               # source refs ⊆ target's keys
//! ```

use std::collections::BTreeSet;
use std::path::Path;

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

use crate::extract::{Extract, ExtractSpec, extract_values, is_non_literal};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceSpec {
    file: String,
    extract: ExtractSpec,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetEntrySpec {
    file: String,
    extract: ExtractSpec,
}

/// `targets:` is either a `{ files: <glob>, extract: … }` map
/// (form a — one query applied per glob match) or a sequence of
/// `{ file, extract }` (form b — heterogeneous pins). A YAML map
/// vs a sequence are structurally distinct, so an untagged enum
/// decodes them unambiguously.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TargetsSpec {
    Glob { files: String, extract: ExtractSpec },
    List(Vec<TargetEntrySpec>),
}

/// The relation the source value(s) must hold to each target's
/// value(s). `equals` is the 1:1 scalar case (the released
/// `cross_file_value_equals`); the set relations compare the
/// source's extracted set `S` to each target's extracted set `T`.
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Relation {
    /// Source extracts exactly one value `v`; every target value
    /// must equal `v` (after normalize).
    #[default]
    Equals,
    /// `S ⊆ T` — every source value appears in the target
    /// (singleton `S` = membership).
    Subset,
    /// `S ⊇ T` — every target value appears in the source.
    Superset,
    /// `S == T` — the sets match exactly.
    SetEquals,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Normalize {
    #[default]
    None,
    Trim,
    Lower,
    /// Compare only the leading `MAJOR` token (the dotnet/runtime
    /// SDK-band shape: same feature band, not exact patch).
    SemverMajor,
}

impl Normalize {
    fn apply(self, v: &str) -> String {
        match self {
            Self::None => v.to_string(),
            Self::Trim => v.trim().to_string(),
            Self::Lower => v.trim().to_lowercase(),
            Self::SemverMajor => v
                .trim()
                .split('.')
                .next()
                .unwrap_or("")
                .trim_start_matches(|c: char| !c.is_ascii_digit())
                .to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    source: SourceSpec,
    targets: TargetsSpec,
    #[serde(default)]
    relation: Relation,
    #[serde(default)]
    normalize: Normalize,
    #[serde(default)]
    allow_missing_target: bool,
}

/// Resolved target shape.
#[derive(Debug)]
enum Targets {
    Glob { scope: Scope, extract: Extract },
    List(Vec<(String, Extract)>),
}

/// Per-target callback for `each_target`: receives the target's
/// path, its raw literal values, and the violation sink.
type TargetFn<'a> = dyn FnMut(&Path, &[String], &mut Vec<Violation>) + 'a;

#[derive(Debug)]
pub struct CrossFileRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    source_file: String,
    source_extract: Extract,
    targets: Targets,
    relation: Relation,
    normalize: Normalize,
    allow_missing: bool,
}

impl Rule for CrossFileRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // Cross-file: the source and every target may live
        // anywhere in the tree; never `--changed`-scoped.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut out = Vec::new();
        let Some(source_values) = self.source_values(ctx, &mut out) else {
            return Ok(out);
        };

        match self.relation {
            Relation::Equals => self.check_equals(ctx, &source_values, &mut out),
            Relation::Subset | Relation::Superset | Relation::SetEquals => {
                let source_set: BTreeSet<String> = source_values
                    .iter()
                    .map(|v| self.normalize.apply(v))
                    .collect();
                self.check_set(ctx, &source_set, &mut out);
            }
        }
        Ok(out)
    }
}

impl CrossFileRule {
    /// Read + extract the source file's literal values (raw, not
    /// normalised — callers normalise as the relation needs).
    /// `None` (with a violation pushed) when the source can't be
    /// read or parsed.
    fn source_values(&self, ctx: &Context<'_>, out: &mut Vec<Violation>) -> Option<Vec<String>> {
        let src = Path::new(&self.source_file);
        let text = match read_rel(ctx, src) {
            Ok(t) => t,
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                out.push(Self::violation(
                    src,
                    &format!("source file is too large to analyze ({n} bytes; 256 MiB cap)"),
                ));
                return None;
            }
            Err(crate::io::ReadCapError::Io(e)) => {
                out.push(Self::violation(
                    src,
                    &format!("source file is unreadable: {e}"),
                ));
                return None;
            }
        };
        let values = match extract_values(&self.source_extract, &text) {
            Ok(v) => v,
            Err(e) => {
                out.push(Self::violation(src, &format!("source extract failed: {e}")));
                return None;
            }
        };
        let (skipped, literal): (Vec<String>, Vec<String>) =
            values.into_iter().partition(|v| is_non_literal(v));
        for v in &skipped {
            out.push(Self::note(
                src,
                &format!("skipped non-literal source value {v:?}"),
            ));
        }
        Some(literal)
    }

    /// `relation: equals` — the released `cross_file_value_equals`
    /// behaviour: the source must resolve to exactly one value, and
    /// every target value must equal it after normalize.
    fn check_equals(&self, ctx: &Context<'_>, source_values: &[String], out: &mut Vec<Violation>) {
        let source = match source_values {
            [one] => one.clone(),
            [] => {
                out.push(Self::violation(
                    Path::new(&self.source_file),
                    "canonical value not found (the source query matched no literal value)",
                ));
                return;
            }
            _ => {
                out.push(Self::violation(
                    Path::new(&self.source_file),
                    "source must resolve to exactly one value (the query matched several); \
                     use a set relation (subset/superset/set_equals) for multi-value sources",
                ));
                return;
            }
        };
        let source_norm = self.normalize.apply(&source);
        self.each_target(ctx, out, &mut |target, values, out| {
            if values.is_empty() {
                if !self.allow_missing {
                    out.push(Self::violation(
                        target,
                        "no literal value to compare (the target query matched nothing)",
                    ));
                }
                return;
            }
            for value in values {
                if self.normalize.apply(value) != source_norm {
                    out.push(self.mismatch(target, &source, value));
                }
            }
        });
    }

    /// The set relations — compare the source set `S` to each
    /// target's extracted (normalised) set `T`.
    fn check_set(
        &self,
        ctx: &Context<'_>,
        source_set: &BTreeSet<String>,
        out: &mut Vec<Violation>,
    ) {
        self.each_target(ctx, out, &mut |target, values, out| {
            let target_set: BTreeSet<String> =
                values.iter().map(|v| self.normalize.apply(v)).collect();
            if let Some(v) = self.set_violation(target, source_set, &target_set) {
                out.push(v);
            }
        });
    }

    fn set_violation(
        &self,
        target: &Path,
        source: &BTreeSet<String>,
        actual: &BTreeSet<String>,
    ) -> Option<Violation> {
        let missing: BTreeSet<&String> = source.difference(actual).collect();
        let extra: BTreeSet<&String> = actual.difference(source).collect();
        let reason = match self.relation {
            Relation::Subset if !missing.is_empty() => Some(format!(
                "is missing value(s) required by {}: {}",
                self.source_file,
                render(&missing)
            )),
            Relation::Superset if !extra.is_empty() => Some(format!(
                "has value(s) not present in {}: {}",
                self.source_file,
                render(&extra)
            )),
            Relation::SetEquals if !missing.is_empty() || !extra.is_empty() => Some(format!(
                "set differs from {} (missing: {}; extra: {})",
                self.source_file,
                render(&missing),
                render(&extra),
            )),
            _ => None,
        }?;
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("{} {reason}", target.display()));
        Some(Violation::new(msg).with_path(target.to_path_buf()))
    }

    /// Iterate the targets (glob expansion or explicit list),
    /// calling `f(target_path, raw_literal_values, out)` for each
    /// readable target. Read/extract errors and a zero-match glob
    /// are reported here, so `f` sees only resolvable targets.
    fn each_target(&self, ctx: &Context<'_>, out: &mut Vec<Violation>, f: &mut TargetFn<'_>) {
        match &self.targets {
            Targets::Glob { scope, extract } => {
                let mut matched = 0usize;
                for e in ctx.index.files() {
                    if !scope.matches(&e.path, ctx.index) {
                        continue;
                    }
                    matched += 1;
                    if let Some(values) = self.target_values(ctx, &e.path, extract, out) {
                        f(&e.path, &values, out);
                    }
                }
                if matched == 0 && !self.allow_missing {
                    out.push(Self::violation(
                        Path::new(&self.source_file),
                        "targets glob matched no files",
                    ));
                }
            }
            Targets::List(list) => {
                for (file, extract) in list {
                    let target = Path::new(file);
                    if let Some(values) = self.target_values(ctx, target, extract, out) {
                        f(target, &values, out);
                    }
                }
            }
        }
    }

    /// Read + extract one target's raw literal values. `None`
    /// (with a violation pushed, unless `allow_missing` for the
    /// missing-file case) when the target can't be read or parsed.
    fn target_values(
        &self,
        ctx: &Context<'_>,
        target: &Path,
        extract: &Extract,
        out: &mut Vec<Violation>,
    ) -> Option<Vec<String>> {
        let text = match read_rel(ctx, target) {
            Ok(t) => t,
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                // A too-large target is always a violation — never
                // suppressed by `allow_missing` (it is present,
                // just unanalysable).
                out.push(Self::violation(
                    target,
                    &format!("target file is too large to analyze ({n} bytes; 256 MiB cap)"),
                ));
                return None;
            }
            Err(crate::io::ReadCapError::Io(_)) => {
                if !self.allow_missing {
                    out.push(Self::violation(
                        target,
                        "target file is missing or unreadable",
                    ));
                }
                return None;
            }
        };
        let values = match extract_values(extract, &text) {
            Ok(v) => v,
            Err(e) => {
                out.push(Self::violation(
                    target,
                    &format!("target extract failed: {e}"),
                ));
                return None;
            }
        };
        let (skipped, literal): (Vec<String>, Vec<String>) =
            values.into_iter().partition(|v| is_non_literal(v));
        for v in &skipped {
            out.push(Self::note(
                target,
                &format!("skipped non-literal target value {v:?}"),
            ));
        }
        Some(literal)
    }

    fn violation(path: &Path, reason: &str) -> Violation {
        Violation::new(format!("{}: {reason}", path.display())).with_path(path.to_path_buf())
    }

    /// An informational note (non-violation finding) — e.g. a
    /// non-literal value the rule skipped rather than compared.
    fn note(path: &Path, reason: &str) -> Violation {
        Self::violation(path, reason).as_note()
    }

    fn mismatch(&self, target: &Path, source: &str, target_value: &str) -> Violation {
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} value {target_value:?} != {} value {source:?}",
                target.display(),
                self.source_file,
            )
        });
        Violation::new(msg).with_path(target.to_path_buf())
    }
}

/// Render a sorted value set for a violation message.
fn render(set: &BTreeSet<&String>) -> String {
    set.iter()
        .map(|v| format!("{v:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Read a tree-relative path as text (the index stores paths, not
/// contents, so the cross-file rules read the file themselves).
fn read_rel(ctx: &Context<'_>, rel: &Path) -> Result<String, crate::io::ReadCapError> {
    crate::io::read_capped(&ctx.root.join(rel)).map(|b| String::from_utf8_lossy(&b).into_owned())
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "cross_file")?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;

    let cfg = |msg: String| Error::rule_config(&spec.id, msg);

    if opts.source.file.trim().is_empty() {
        return Err(cfg("`source.file` must not be empty".into()));
    }
    let source_extract = opts
        .source
        .extract
        .resolve()
        .map_err(|e| cfg(format!("invalid `source.extract`: {e}")))?;

    let targets = match opts.targets {
        TargetsSpec::Glob { files, extract } => {
            if files.trim().is_empty() {
                return Err(cfg("`targets.files` must not be empty".into()));
            }
            let scope = Scope::from_patterns(std::slice::from_ref(&files))
                .map_err(|e| cfg(format!("invalid `targets.files` glob: {e}")))?;
            Targets::Glob {
                scope,
                extract: extract
                    .resolve()
                    .map_err(|e| cfg(format!("invalid `targets.extract`: {e}")))?,
            }
        }
        TargetsSpec::List(list) => {
            if list.is_empty() {
                return Err(cfg("`targets` list must not be empty".into()));
            }
            let mut resolved = Vec::with_capacity(list.len());
            for (i, t) in list.into_iter().enumerate() {
                if t.file.trim().is_empty() {
                    return Err(cfg(format!("`targets[{i}].file` must not be empty")));
                }
                let ex = t
                    .extract
                    .resolve()
                    .map_err(|e| cfg(format!("invalid `targets[{i}].extract`: {e}")))?;
                resolved.push((t.file, ex));
            }
            Targets::List(resolved)
        }
    };

    Ok(Box::new(CrossFileRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        source_file: opts.source.file,
        source_extract,
        targets,
        relation: opts.relation,
        normalize: opts.normalize,
        allow_missing: opts.allow_missing_target,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{FileEntry, FileIndex};

    fn index(files: &[&str]) -> FileIndex {
        FileIndex::from_entries(
            files
                .iter()
                .map(|p| FileEntry {
                    path: Path::new(p).into(),
                    is_dir: false,
                    size: 1,
                })
                .collect(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn rule(
        source_file: &str,
        source: Extract,
        targets: Targets,
        relation: Relation,
        normalize: Normalize,
    ) -> CrossFileRule {
        CrossFileRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: source_file.into(),
            source_extract: source,
            targets,
            relation,
            normalize,
            allow_missing: false,
        }
    }

    fn eval(r: &CrossFileRule, root: &Path, idx: &FileIndex) -> Vec<Violation> {
        let ctx = Context {
            root,
            index: idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        r.evaluate(&ctx).unwrap()
    }

    // ─── equals (the migrated cross_file_value_equals path) ──────

    #[test]
    fn equals_glob_targets_pass_and_fail_on_version_lockstep() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace.package]\nversion = \"1.4.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("crates/a")).unwrap();
        std::fs::create_dir_all(root.join("crates/b")).unwrap();
        std::fs::write(
            root.join("crates/a/Cargo.toml"),
            "[package]\nversion = \"1.4.0\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crates/b/Cargo.toml"),
            "[package]\nversion = \"1.3.0\"\n",
        )
        .unwrap();
        let idx = index(&["Cargo.toml", "crates/a/Cargo.toml", "crates/b/Cargo.toml"]);
        let r = rule(
            "Cargo.toml",
            Extract::Toml("$.workspace.package.version".into()),
            Targets::Glob {
                scope: Scope::from_patterns(&["crates/*/Cargo.toml".to_string()]).unwrap(),
                extract: Extract::Toml("$.package.version".into()),
            },
            Relation::Equals,
            Normalize::None,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "only crates/b drifts: {v:?}");
        assert!(v[0].message.contains("crates/b/Cargo.toml"));
        assert!(v[0].message.contains("1.3.0"));
    }

    #[test]
    fn equals_multi_value_source_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("m.json"), "{\"v\":[\"1\",\"2\"]}").unwrap();
        let idx = index(&["m.json"]);
        let r = rule(
            "m.json",
            Extract::Json("$.v[*]".into()),
            Targets::List(vec![("m.json".into(), Extract::Json("$.v[0]".into()))]),
            Relation::Equals,
            Normalize::None,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("exactly one value"));
    }

    #[test]
    fn equals_semver_major_normalize_allows_band() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("global.json"),
            "{\"sdk\":{\"version\":\"8.0.402\"}}",
        )
        .unwrap();
        std::fs::write(root.join("Directory.Build.props"), "8.0.100\n").unwrap();
        let idx = index(&["global.json", "Directory.Build.props"]);
        let r = rule(
            "global.json",
            Extract::Json("$.sdk.version".into()),
            Targets::List(vec![(
                "Directory.Build.props".into(),
                Extract::Lines(crate::extract::LinesOpts::default()),
            )]),
            Relation::Equals,
            Normalize::SemverMajor,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    // ─── set relations ──────────────────────────────────────────

    fn set_rule(source: Extract, targets: Targets, relation: Relation) -> CrossFileRule {
        rule("src.json", source, targets, relation, Normalize::None)
    }

    fn write_sets(root: &Path, source: &str, target: &str) {
        std::fs::write(root.join("src.json"), source).unwrap();
        std::fs::write(root.join("tgt.json"), target).unwrap();
    }

    fn set_targets() -> Targets {
        Targets::List(vec![("tgt.json".into(), Extract::Json("$.have[*]".into()))])
    }

    #[test]
    fn subset_fires_when_a_source_value_is_missing_from_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {a, b}; T = {a} -> b is missing.
        write_sets(root, "{\"need\":[\"a\",\"b\"]}", "{\"have\":[\"a\",\"c\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Json("$.need[*]".into()),
            set_targets(),
            Relation::Subset,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("missing"));
        assert!(v[0].message.contains("\"b\""));
        // `c` is extra in the target but `subset` does not care.
        assert!(!v[0].message.contains("\"c\""));
    }

    #[test]
    fn subset_silent_when_source_is_contained() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_sets(
            root,
            "{\"need\":[\"a\",\"b\"]}",
            "{\"have\":[\"a\",\"b\",\"c\"]}",
        );
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Json("$.need[*]".into()),
            set_targets(),
            Relation::Subset,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn superset_fires_on_a_target_value_not_in_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {a, b}; T = {a, z} -> z is not covered by the source.
        write_sets(root, "{\"need\":[\"a\",\"b\"]}", "{\"have\":[\"a\",\"z\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Json("$.need[*]".into()),
            set_targets(),
            Relation::Superset,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("not present"));
        assert!(v[0].message.contains("\"z\""));
    }

    #[test]
    fn set_equals_reports_both_missing_and_extra() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {a, b}; T = {a, z} -> missing b, extra z.
        write_sets(root, "{\"need\":[\"a\",\"b\"]}", "{\"have\":[\"a\",\"z\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Json("$.need[*]".into()),
            set_targets(),
            Relation::SetEquals,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("missing"));
        assert!(v[0].message.contains("\"b\""));
        assert!(v[0].message.contains("extra"));
        assert!(v[0].message.contains("\"z\""));
    }

    #[test]
    fn set_equals_silent_on_matching_sets_regardless_of_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_sets(root, "{\"need\":[\"b\",\"a\"]}", "{\"have\":[\"a\",\"b\"]}");
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Json("$.need[*]".into()),
            set_targets(),
            Relation::SetEquals,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn subset_singleton_is_a_membership_check() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // S = {needle}; member -> silent.
        write_sets(
            root,
            "{\"need\":[\"needle\"]}",
            "{\"have\":[\"hay\",\"needle\",\"straw\"]}",
        );
        let idx = index(&["src.json", "tgt.json"]);
        let r = set_rule(
            Extract::Json("$.need[*]".into()),
            set_targets(),
            Relation::Subset,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }
}
