//! `cross_file_value_equals` — a value extracted from one
//! authoritative file must equal a value extracted from one or
//! more other files. The cross-file value-coherence sibling of
//! `registry_paths_resolve` (path existence); shares
//! `crate::extract`. Design + open-question resolutions:
//! `docs/design/v0.10/cross_file_value_equals.md`.
//!
//! ```yaml
//! - id: workspace-versions-coherent
//!   kind: cross_file_value_equals
//!   source:
//!     file: Cargo.toml
//!     extract: { toml: "$.workspace.package.version" }
//!   targets:                       # form (a): glob + one extract
//!     files: "crates/*/Cargo.toml"
//!     extract: { toml: "$.package.version" }
//!   # OR form (b): an explicit heterogeneous list
//!   # targets:
//!   #   - { file: rust-toolchain.toml, extract: { toml: "$.toolchain.channel" } }
//!   #   - { file: Dockerfile,          extract: { regex: "FROM rust:(\\S+)" } }
//!   normalize: none                # none (default) | trim | lower | semver-major
//!   allow_missing_target: false
//!   level: error
//! ```

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
/// (form a — one query applied per glob match, the istio
/// `value_extractor:` / pitfall-#20 shape) or a sequence of
/// `{ file, extract }` (form b — heterogeneous pins). A YAML map
/// vs a sequence are structurally distinct, so an untagged enum
/// decodes them unambiguously (unlike the externally-tagged-enum
/// trap `crate::extract` documents).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TargetsSpec {
    Glob { files: String, extract: ExtractSpec },
    List(Vec<TargetEntrySpec>),
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

#[derive(Debug)]
pub struct CrossFileValueEqualsRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    source_file: String,
    source_extract: Extract,
    targets: Targets,
    normalize: Normalize,
    allow_missing: bool,
}

impl Rule for CrossFileValueEqualsRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // Cross-file: the source and every target may live
        // anywhere in the tree; never `--changed`-scoped.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut out = Vec::new();
        let Some(source) = self.resolve_source(ctx, &mut out) else {
            return Ok(out);
        };
        let source_norm = self.normalize.apply(&source);

        match &self.targets {
            Targets::Glob { scope, extract } => {
                let mut matched = 0usize;
                for e in ctx.index.files() {
                    if !scope.matches(&e.path, ctx.index) {
                        continue;
                    }
                    matched += 1;
                    self.check_target(ctx, &e.path, extract, &source, &source_norm, &mut out);
                }
                if matched == 0 && !self.allow_missing {
                    out.push(Self::violation(
                        Path::new(&self.source_file),
                        &format!("targets glob matched no files (source value {source:?})"),
                    ));
                }
            }
            Targets::List(list) => {
                for (file, extract) in list {
                    self.check_target(
                        ctx,
                        Path::new(file),
                        extract,
                        &source,
                        &source_norm,
                        &mut out,
                    );
                }
            }
        }
        Ok(out)
    }
}

impl CrossFileValueEqualsRule {
    /// Read + extract the single authoritative source value.
    /// `None` (with a violation pushed) when it can't be resolved.
    fn resolve_source(&self, ctx: &Context<'_>, out: &mut Vec<Violation>) -> Option<String> {
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
        match literal.as_slice() {
            [one] => Some(one.clone()),
            [] => {
                out.push(Self::violation(
                    src,
                    "canonical value not found (the source query matched no literal value)",
                ));
                None
            }
            _ => {
                out.push(Self::violation(
                    src,
                    "source must resolve to exactly one value (the query matched several)",
                ));
                None
            }
        }
    }

    fn check_target(
        &self,
        ctx: &Context<'_>,
        target: &Path,
        extract: &Extract,
        source: &str,
        source_norm: &str,
        out: &mut Vec<Violation>,
    ) {
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
                return;
            }
            Err(crate::io::ReadCapError::Io(_)) => {
                if !self.allow_missing {
                    out.push(Self::violation(
                        target,
                        "target file is missing or unreadable",
                    ));
                }
                return;
            }
        };
        let values = match extract_values(extract, &text) {
            Ok(v) => v,
            Err(e) => {
                out.push(Self::violation(
                    target,
                    &format!("target extract failed: {e}"),
                ));
                return;
            }
        };
        let (skipped, literal): (Vec<&String>, Vec<&String>) =
            values.iter().partition(|v| is_non_literal(v));
        for v in &skipped {
            out.push(Self::note(
                target,
                &format!("skipped non-literal target value {v:?}"),
            ));
        }
        if literal.is_empty() {
            if !self.allow_missing {
                out.push(Self::violation(
                    target,
                    "no literal value to compare (the target query matched nothing)",
                ));
            }
            return;
        }
        for value in literal {
            if self.normalize.apply(value) != source_norm {
                out.push(self.mismatch(target, source, value));
            }
        }
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

/// Read a tree-relative path as text (the index stores paths, not
/// contents, so the cross-file rules read the file themselves).
fn read_rel(ctx: &Context<'_>, rel: &Path) -> Result<String, crate::io::ReadCapError> {
    crate::io::read_capped(&ctx.root.join(rel)).map(|b| String::from_utf8_lossy(&b).into_owned())
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "cross_file_value_equals")?;
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

    Ok(Box::new(CrossFileValueEqualsRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        source_file: opts.source.file,
        source_extract,
        targets,
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

    fn rule(
        source_file: &str,
        source: Extract,
        targets: Targets,
        normalize: Normalize,
    ) -> CrossFileValueEqualsRule {
        CrossFileValueEqualsRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_file: source_file.into(),
            source_extract: source,
            targets,
            normalize,
            allow_missing: false,
        }
    }

    fn eval(r: &CrossFileValueEqualsRule, root: &Path, idx: &FileIndex) -> Vec<Violation> {
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

    #[test]
    fn glob_targets_pass_and_fail_on_version_lockstep() {
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
            Normalize::None,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "only crates/b drifts: {v:?}");
        assert!(v[0].message.contains("crates/b/Cargo.toml"));
        assert!(v[0].message.contains("1.3.0"));
    }

    #[test]
    fn explicit_list_heterogeneous_targets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.85\"\n",
        )
        .unwrap();
        std::fs::write(root.join("VERSION"), "1.85\n").unwrap();
        std::fs::write(root.join("Bad"), "1.84\n").unwrap();
        let idx = index(&["rust-toolchain.toml", "VERSION", "Bad"]);
        let r = rule(
            "rust-toolchain.toml",
            Extract::Toml("$.toolchain.channel".into()),
            Targets::List(vec![
                (
                    "VERSION".into(),
                    Extract::Lines(crate::extract::LinesOpts::default()),
                ),
                (
                    "Bad".into(),
                    Extract::Lines(crate::extract::LinesOpts::default()),
                ),
            ]),
            Normalize::Trim,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "only Bad drifts: {v:?}");
        assert!(v[0].message.contains("Bad"));
    }

    #[test]
    fn semver_major_normalize_allows_band() {
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
            Normalize::SemverMajor,
        );
        // 8.0.402 vs 8.0.100 — same major band, no violation.
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn lower_normalize_makes_case_insensitive() {
        // Design-doc normalize matrix: `lower` was untested.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("src.txt"), "ABC\n").unwrap();
        std::fs::write(root.join("tgt.txt"), "abc\n").unwrap();
        let idx = index(&["src.txt", "tgt.txt"]);
        let mk = |n| {
            rule(
                "src.txt",
                Extract::Lines(crate::extract::LinesOpts::default()),
                Targets::List(vec![(
                    "tgt.txt".into(),
                    Extract::Lines(crate::extract::LinesOpts::default()),
                )]),
                n,
            )
        };
        assert_eq!(
            eval(&mk(Normalize::None), root, &idx).len(),
            1,
            "ABC vs abc differ under None"
        );
        assert!(
            eval(&mk(Normalize::Lower), root, &idx).is_empty(),
            "lower normalize makes the compare case-insensitive"
        );
    }

    #[test]
    fn multi_value_source_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("m.json"), "{\"v\":[\"1\",\"2\"]}").unwrap();
        let idx = index(&["m.json"]);
        let r = rule(
            "m.json",
            Extract::Json("$.v[*]".into()),
            Targets::List(vec![("m.json".into(), Extract::Json("$.v[0]".into()))]),
            Normalize::None,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("exactly one value"));
    }

    #[test]
    fn non_literal_target_value_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("src.toml"), "v = \"1.0.0\"\n").unwrap();
        std::fs::write(root.join("t.toml"), "v = \"${VERSION}\"\n").unwrap();
        let idx = index(&["src.toml", "t.toml"]);
        let r = rule(
            "src.toml",
            Extract::Toml("$.v".into()),
            Targets::List(vec![("t.toml".into(), Extract::Toml("$.v".into()))]),
            Normalize::None,
        );
        // The only target value is interpolated -> skipped, not a
        // mismatch; but "no literal value" fires unless allowed.
        let mut r2 = r;
        r2.allow_missing = true;
        let v = eval(&r2, root, &idx);
        // Skipped non-literal target surfaces as a note, not a
        // violation (v0.11).
        let real: Vec<_> = v.iter().filter(|x| !x.is_note).collect();
        let notes: Vec<_> = v.iter().filter(|x| x.is_note).collect();
        assert!(
            real.is_empty(),
            "non-literal must not be a violation, got {real:?}"
        );
        assert_eq!(notes.len(), 1, "skipped target value surfaces as one note");
        assert!(
            notes[0]
                .message
                .contains("skipped non-literal target value"),
            "note: {:?}",
            notes[0].message
        );
    }
}
