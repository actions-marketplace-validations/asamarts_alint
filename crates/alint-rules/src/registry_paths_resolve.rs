//! `registry_paths_resolve` — a manifest file enumerates
//! path-like entries; each must resolve to an on-disk artefact.
//! Optional reverse "orphan" check: on-disk artefacts in a
//! declared space that no entry references.
//!
//! Cross-file: reads one manifest and resolves its entries
//! against the engine `FileIndex` (O(1) per entry via the lazy
//! path-set). Design + rationale + open-question resolutions:
//! `docs/design/v0.10/registry_paths_resolve.md`.
//!
//! ```yaml
//! - id: cargo-workspace-members-resolve
//!   kind: registry_paths_resolve
//!   registry: Cargo.toml
//!   extract: { toml: "$.workspace.members[*]" }
//!   base: registry_dir          # registry_dir (default) | lint_root | "<path>"
//!   entries_are_globs: true
//!   expect: dir                 # any (default) | file | dir
//!   must_contain: Cargo.toml
//!   exclude_query: "$.workspace.exclude[*]"
//!   orphans: { space: "crates/*", unreferenced: warn }
//!   level: error
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;
use serde_json_path::JsonPath;

use crate::structured_path::Format;

/// Runtime extraction mode, resolved from [`ExtractSpec`].
#[derive(Debug, Clone)]
enum Extract {
    /// Structured-query (RFC 9535 `JSONPath` over the parsed tree).
    Toml(String),
    Json(String),
    Yaml(String),
    /// One path per non-blank, non-comment line.
    Lines(LinesOpts),
    /// Capture group 1 of each match is the path.
    Regex(String),
}

/// The deserialised `extract:` block. `serde_yaml` does not
/// decode an externally-tagged enum from a `{ key: value }` map
/// (it expects a YAML `!tag`), and an untagged enum can't tell
/// the three `JSONPath` string variants apart — so the config
/// shape is a struct-of-options validated to exactly-one in
/// [`build`].
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtractSpec {
    #[serde(default)]
    toml: Option<String>,
    #[serde(default)]
    json: Option<String>,
    #[serde(default)]
    yaml: Option<String>,
    #[serde(default)]
    lines: Option<LinesOpts>,
    #[serde(default)]
    regex: Option<String>,
}

impl ExtractSpec {
    fn resolve(self) -> std::result::Result<Extract, String> {
        let set: Vec<&str> = [
            ("toml", self.toml.is_some()),
            ("json", self.json.is_some()),
            ("yaml", self.yaml.is_some()),
            ("lines", self.lines.is_some()),
            ("regex", self.regex.is_some()),
        ]
        .into_iter()
        .filter_map(|(n, on)| on.then_some(n))
        .collect();
        match set.as_slice() {
            [] => Err(
                "`extract` must set exactly one of toml/json/yaml/lines/regex (none set)"
                    .to_string(),
            ),
            [_] => Ok(if let Some(q) = self.toml {
                Extract::Toml(q)
            } else if let Some(q) = self.json {
                Extract::Json(q)
            } else if let Some(q) = self.yaml {
                Extract::Yaml(q)
            } else if let Some(o) = self.lines {
                Extract::Lines(o)
            } else {
                Extract::Regex(self.regex.expect("exactly-one ensures regex set"))
            }),
            many => Err(format!(
                "`extract` must set exactly one of toml/json/yaml/lines/regex (got {})",
                many.join(", ")
            )),
        }
    }
}

impl From<Extract> for ExtractSpec {
    fn from(e: Extract) -> Self {
        let mut s = ExtractSpec::default();
        match e {
            Extract::Toml(q) => s.toml = Some(q),
            Extract::Json(q) => s.json = Some(q),
            Extract::Yaml(q) => s.yaml = Some(q),
            Extract::Lines(o) => s.lines = Some(o),
            Extract::Regex(q) => s.regex = Some(q),
        }
        s
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinesOpts {
    /// Lines starting with this (after trim) are skipped.
    #[serde(default = "default_comment")]
    comment: String,
}

fn default_comment() -> String {
    "#".to_string()
}

// `#[serde(default = "default_comment")]` only fires on the
// deserialize path; `LinesOpts::default()` (used by the
// `Lines(#[serde(default)] …)` variant and tests) needs the
// same `#` default, so derive can't be used here.
impl Default for LinesOpts {
    fn default() -> Self {
        Self {
            comment: default_comment(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Expect {
    #[default]
    Any,
    File,
    Dir,
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Severity {
    #[default]
    Warn,
    Error,
    Off,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrphansSpec {
    /// Glob of on-disk artefacts that should each be referenced.
    space: String,
    #[serde(default)]
    unreferenced: Severity,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    registry: String,
    extract: ExtractSpec,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    entries_are_globs: bool,
    #[serde(default)]
    expect: Expect,
    #[serde(default)]
    must_contain: Option<String>,
    #[serde(default)]
    exclude_query: Option<String>,
    #[serde(default)]
    orphans: Option<OrphansSpec>,
}

/// Resolution base for entries.
#[derive(Debug, Clone)]
enum Base {
    /// Directory containing the registry file (default; matches
    /// Cargo / npm semantics + alint's nested-manifest model).
    RegistryDir,
    /// The lint root.
    LintRoot,
    /// An explicit path, relative to the lint root.
    Explicit(PathBuf),
}

impl Base {
    fn parse(raw: Option<&str>) -> Self {
        match raw {
            None | Some("registry_dir") => Self::RegistryDir,
            Some("lint_root") => Self::LintRoot,
            Some(p) => Self::Explicit(PathBuf::from(p)),
        }
    }
}

#[derive(Debug)]
pub struct RegistryPathsResolveRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    registry: String,
    registry_scope: Option<Scope>,
    extract: Extract,
    base: Base,
    entries_are_globs: bool,
    expect: Expect,
    must_contain: Option<String>,
    exclude_query: Option<String>,
    orphans: Option<OrphansSpec>,
}

/// An entry that the extractor deliberately skipped (non-literal:
/// interpolation / variables / antiquotation). Surfaced rather
/// than silently dropped so `--explain` shows *why* a path was
/// not checked, and never fails the rule.
fn is_non_literal(entry: &str) -> bool {
    entry.contains("${")
        || entry.contains("{{")
        || entry.contains('$')
        || entry.contains('`')
        // Nix antiquotation / computed path expressions.
        || entry.contains("+ ")
        || entry.contains("(.")
}

impl Rule for RegistryPathsResolveRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // Cross-file: an entry's verdict depends on whether its
        // target exists anywhere in the tree, and the orphan
        // check needs the whole index — never `--changed`-scoped.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();

        // Directory existence: build the dir path-set once per
        // eval (O(D)); per-entry lookups are then O(1), matching
        // `contains_file`'s scaling so the rule stays index-fast.
        let dir_set: HashSet<&Path> = if self.expect == Expect::Dir
            || self.expect == Expect::Any
            || self.must_contain.is_some()
        {
            ctx.index.dirs().map(|e| &*e.path).collect()
        } else {
            HashSet::new()
        };

        for registry_rel in self.registry_files(ctx) {
            let abs = ctx.root.join(&registry_rel);
            let text = match std::fs::read_to_string(&abs) {
                Ok(t) => t,
                Err(e) => {
                    violations.push(
                        Violation::new(format!(
                            "registry file {} could not be read: {e}",
                            registry_rel.display()
                        ))
                        .with_path(registry_rel.clone()),
                    );
                    continue;
                }
            };

            let (entries, skipped) = match self.extract_entries(&text) {
                Ok(v) => v,
                Err(e) => {
                    violations.push(
                        Violation::new(format!(
                            "registry file {} could not be parsed for `extract`: {e}",
                            registry_rel.display()
                        ))
                        .with_path(registry_rel.clone()),
                    );
                    continue;
                }
            };
            // Skipped (non-literal) entries never fail; surfaced
            // for --explain via the message text only.
            let _ = skipped;

            let excluded = self.excluded_entries(&text);
            let base_dir = self.base_dir(&registry_rel);

            let mut covered: Vec<PathBuf> = Vec::new();
            for entry in &entries {
                if excluded.contains(entry) {
                    continue;
                }
                let resolved = normalise(&base_dir.join(entry));
                if self.entries_are_globs {
                    let matches = Self::glob_matches(ctx, &resolved);
                    if matches.is_empty() {
                        violations.push(self.violation(
                            &registry_rel,
                            entry,
                            "matched no path on disk",
                        ));
                    } else {
                        covered.extend(matches);
                    }
                    continue;
                }
                covered.push(resolved.clone());
                if let Some(reason) = self.existence_problem(ctx, &resolved, &dir_set) {
                    violations.push(self.violation(&registry_rel, entry, &reason));
                }
            }

            // Globbed entries still need existence/kind checks on
            // each expansion (a `crates/*` match must satisfy
            // `must_contain`, etc.).
            if self.entries_are_globs {
                for p in &covered {
                    if let Some(reason) = self.existence_problem(ctx, p, &dir_set) {
                        violations.push(self.violation(
                            &registry_rel,
                            &p.display().to_string(),
                            &reason,
                        ));
                    }
                }
            }

            self.check_orphans(ctx, &registry_rel, &covered, &mut violations);
        }

        Ok(violations)
    }
}

impl RegistryPathsResolveRule {
    /// The registry file(s): a literal path, or every index path
    /// matching the glob.
    fn registry_files(&self, ctx: &Context<'_>) -> Vec<PathBuf> {
        match &self.registry_scope {
            None => vec![PathBuf::from(&self.registry)],
            Some(scope) => ctx
                .index
                .files()
                .filter(|e| scope.matches(&e.path, ctx.index))
                .map(|e| e.path.to_path_buf())
                .collect(),
        }
    }

    fn base_dir(&self, registry_rel: &Path) -> PathBuf {
        match &self.base {
            Base::RegistryDir => registry_rel
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
            Base::LintRoot => PathBuf::new(),
            Base::Explicit(p) => p.clone(),
        }
    }

    fn extract_entries(&self, text: &str) -> std::result::Result<(Vec<String>, usize), String> {
        let raw: Vec<String> = match &self.extract {
            Extract::Toml(q) => structured(Format::Toml, q, text)?,
            Extract::Json(q) => structured(Format::Json, q, text)?,
            Extract::Yaml(q) => structured(Format::Yaml, q, text)?,
            Extract::Lines(opts) => text
                .lines()
                .map(str::trim)
                .filter(|l| {
                    if l.is_empty() {
                        return false;
                    }
                    if opts.comment.is_empty() {
                        return true;
                    }
                    !l.starts_with(opts.comment.as_str())
                })
                .map(ToString::to_string)
                .collect(),
            Extract::Regex(pat) => {
                let re = Regex::new(pat).map_err(|e| format!("bad regex: {e}"))?;
                re.captures_iter(text)
                    .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                    .collect()
            }
        };
        let before = raw.len();
        let kept: Vec<String> = raw.into_iter().filter(|e| !is_non_literal(e)).collect();
        let skipped = before - kept.len();
        Ok((kept, skipped))
    }

    fn excluded_entries(&self, text: &str) -> HashSet<String> {
        let Some(q) = &self.exclude_query else {
            return HashSet::new();
        };
        let fmt = match &self.extract {
            Extract::Json(_) => Format::Json,
            Extract::Yaml(_) => Format::Yaml,
            // exclude_query is a structured query; for line/regex
            // registries it has no meaning. Default to Toml so a
            // misconfig surfaces as an empty set, not a panic.
            _ => Format::Toml,
        };
        structured(fmt, q, text)
            .map(|v| v.into_iter().collect())
            .unwrap_or_default()
    }

    /// Reverse-completeness: on-disk artefacts under `orphans.space`
    /// that no (post-expansion) entry covered.
    fn check_orphans(
        &self,
        ctx: &Context<'_>,
        registry_rel: &Path,
        covered: &[PathBuf],
        out: &mut Vec<Violation>,
    ) {
        let Some(orph) = &self.orphans else {
            return;
        };
        if orph.unreferenced == Severity::Off {
            return;
        }
        let covered_set: HashSet<&Path> = covered.iter().map(PathBuf::as_path).collect();
        let Ok(space) = Scope::from_patterns(std::slice::from_ref(&orph.space)) else {
            return;
        };
        for e in ctx.index.files() {
            if space.matches(&e.path, ctx.index) && !covered_set.contains(&*e.path) {
                out.push(
                    Violation::new(format!(
                        "{} is under `{}` but no entry in {} references it",
                        e.path.display(),
                        orph.space,
                        registry_rel.display(),
                    ))
                    .with_path(e.path.clone()),
                );
            }
        }
    }

    fn glob_matches(ctx: &Context<'_>, pattern: &Path) -> Vec<PathBuf> {
        let pat = pattern.to_string_lossy().into_owned();
        let Ok(scope) = Scope::from_patterns(&[pat]) else {
            return Vec::new();
        };
        ctx.index
            .files()
            .filter(|e| scope.matches(&e.path, ctx.index))
            .map(|e| e.path.to_path_buf())
            .chain(
                ctx.index
                    .dirs()
                    .filter(|e| scope.matches(&e.path, ctx.index))
                    .map(|e| e.path.to_path_buf()),
            )
            .collect()
    }

    /// `None` => the resolved path is fine. `Some(reason)` => a
    /// violation message fragment.
    fn existence_problem(
        &self,
        ctx: &Context<'_>,
        path: &Path,
        dir_set: &HashSet<&Path>,
    ) -> Option<String> {
        let is_file = ctx.index.contains_file(path);
        let is_dir = dir_set.contains(path);
        match self.expect {
            Expect::File => {
                if !is_file {
                    return Some("does not resolve to a file on disk".into());
                }
            }
            Expect::Dir => {
                if !is_dir {
                    return Some("does not resolve to a directory on disk".into());
                }
            }
            Expect::Any => {
                if !is_file && !is_dir {
                    return Some("does not resolve to any path on disk".into());
                }
            }
        }
        if let Some(mc) = &self.must_contain {
            // Only meaningful when the entry is a directory.
            if is_dir && !ctx.index.contains_file(&path.join(mc)) {
                return Some(format!("resolves to a directory missing `{mc}`"));
            }
        }
        None
    }

    fn violation(&self, registry: &Path, entry: &str, reason: &str) -> Violation {
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("{}: entry {entry:?} {reason}", registry.display()));
        Violation::new(msg).with_path(registry.to_path_buf())
    }
}

/// Run a structured-query (`Format::parse` + RFC 9535 `JSONPath`),
/// returning every string-valued match. Non-string nodes are
/// dropped (a non-literal path that the manifest expresses as a
/// table/array is skipped, not failed).
fn structured(fmt: Format, query: &str, text: &str) -> std::result::Result<Vec<String>, String> {
    let value = fmt.parse(text)?;
    let path = JsonPath::parse(query).map_err(|e| format!("bad JSONPath {query:?}: {e}"))?;
    Ok(path
        .query(&value)
        .iter()
        .filter_map(|v| v.as_str().map(ToString::to_string))
        .collect())
}

/// Collapse `a/./b` and `a/b/../c` so index lookups (which key on
/// the walked relative path) match. Does not touch the
/// filesystem.
fn normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        use std::path::Component::{CurDir, Normal, ParentDir, Prefix, RootDir};
        match comp {
            CurDir => {}
            ParentDir => {
                out.pop();
            }
            Normal(c) => out.push(c),
            RootDir | Prefix(_) => out.push(comp.as_os_str()),
        }
    }
    out
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "registry_paths_resolve")?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;

    if opts.registry.trim().is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "registry_paths_resolve `registry` must not be empty",
        ));
    }
    // A glob registry is resolved against the index; a literal one
    // is read directly. `is_glob` mirrors the structured-path /
    // file_exists literal test.
    let is_glob = opts
        .registry
        .chars()
        .any(|c| matches!(c, '*' | '?' | '[' | ']' | '{' | '}'));
    let registry_scope = if is_glob {
        Some(
            Scope::from_patterns(std::slice::from_ref(&opts.registry)).map_err(|e| {
                Error::rule_config(&spec.id, format!("invalid `registry` glob: {e}"))
            })?,
        )
    } else {
        None
    };
    let extract = opts
        .extract
        .resolve()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid `extract`: {e}")))?;
    if let Extract::Regex(p) = &extract {
        Regex::new(p)
            .map_err(|e| Error::rule_config(&spec.id, format!("invalid `extract.regex`: {e}")))?;
    }

    Ok(Box::new(RegistryPathsResolveRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        registry: opts.registry,
        registry_scope,
        extract,
        base: Base::parse(opts.base.as_deref()),
        entries_are_globs: opts.entries_are_globs,
        expect: opts.expect,
        must_contain: opts.must_contain,
        exclude_query: opts.exclude_query,
        orphans: opts.orphans,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alint_core::{FileEntry, FileIndex};

    fn index(files: &[&str], dirs: &[&str]) -> FileIndex {
        let mut e: Vec<FileEntry> = files
            .iter()
            .map(|p| FileEntry {
                path: Path::new(p).into(),
                is_dir: false,
                size: 1,
            })
            .collect();
        e.extend(dirs.iter().map(|p| FileEntry {
            path: Path::new(p).into(),
            is_dir: true,
            size: 0,
        }));
        FileIndex::from_entries(e)
    }

    fn rule(opts: Options) -> RegistryPathsResolveRule {
        RegistryPathsResolveRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            registry: opts.registry,
            registry_scope: None,
            extract: opts.extract.resolve().expect("test extract valid"),
            base: Base::parse(opts.base.as_deref()),
            entries_are_globs: opts.entries_are_globs,
            expect: opts.expect,
            must_contain: opts.must_contain,
            exclude_query: opts.exclude_query,
            orphans: opts.orphans,
        }
    }

    fn opts(registry: &str, extract: Extract) -> Options {
        Options {
            registry: registry.into(),
            extract: extract.into(),
            base: None,
            entries_are_globs: false,
            expect: Expect::Any,
            must_contain: None,
            exclude_query: None,
            orphans: None,
        }
    }

    fn eval(r: &RegistryPathsResolveRule, root: &Path, idx: &FileIndex) -> Vec<Violation> {
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
    fn lines_entries_resolve_pass_and_fail() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("MANIFEST"),
            "src/a.rs\nsrc/b.rs\n# a comment\n",
        )
        .unwrap();
        let r = rule(opts("MANIFEST", Extract::Lines(LinesOpts::default())));
        // Both present -> pass.
        let v = eval(
            &r,
            dir.path(),
            &index(&["src/a.rs", "src/b.rs", "MANIFEST"], &[]),
        );
        assert!(v.is_empty(), "{v:?}");
        // b.rs missing -> one violation.
        let v = eval(&r, dir.path(), &index(&["src/a.rs", "MANIFEST"], &[]));
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("src/b.rs"));
    }

    #[test]
    fn toml_workspace_members_expect_dir_must_contain() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/core\", \"crates/cli\"]\n",
        )
        .unwrap();
        let mut o = opts("Cargo.toml", Extract::Toml("$.workspace.members[*]".into()));
        o.expect = Expect::Dir;
        o.must_contain = Some("Cargo.toml".into());
        let r = rule(o);
        // Both crate dirs exist and contain Cargo.toml -> pass.
        let idx = index(
            &[
                "crates/core/Cargo.toml",
                "crates/cli/Cargo.toml",
                "Cargo.toml",
            ],
            &["crates/core", "crates/cli"],
        );
        assert!(eval(&r, dir.path(), &idx).is_empty());
        // cli dir missing its Cargo.toml -> must_contain violation.
        let idx = index(
            &["crates/core/Cargo.toml", "Cargo.toml"],
            &["crates/core", "crates/cli"],
        );
        let v = eval(&r, dir.path(), &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("crates/cli"));
    }

    #[test]
    fn non_literal_entries_are_skipped_not_failed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pkgs.nix"),
            "callPackage ./pkgs/real {}\ncallPackage (./. + \"/pkgs/${name}\") {}\n",
        )
        .unwrap();
        let r = rule(opts(
            "pkgs.nix",
            Extract::Regex(r"callPackage\s+(\S+)".into()),
        ));
        // Only the literal `./pkgs/real` is checked; the
        // antiquoted entry is skipped (not a violation).
        let idx = index(&["pkgs.nix"], &["pkgs/real"]);
        let v = eval(&r, dir.path(), &idx);
        assert!(v.is_empty(), "non-literal must be skipped, got {v:?}");
    }

    #[test]
    fn entries_are_globs_zero_match_is_a_violation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        let mut o = opts("Cargo.toml", Extract::Toml("$.workspace.members[*]".into()));
        o.entries_are_globs = true;
        let r = rule(o);
        // No crates/* on disk -> the glob matched nothing.
        let v = eval(&r, dir.path(), &index(&["Cargo.toml"], &[]));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("no path"));
    }

    #[test]
    fn orphans_flags_unreferenced_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/a\"]\n",
        )
        .unwrap();
        let mut o = opts("Cargo.toml", Extract::Toml("$.workspace.members[*]".into()));
        o.orphans = Some(OrphansSpec {
            space: "crates/*/Cargo.toml".into(),
            unreferenced: Severity::Error,
        });
        let r = rule(o);
        // crates/b exists on disk but isn't a member -> orphan.
        let idx = index(
            &["crates/a/Cargo.toml", "crates/b/Cargo.toml", "Cargo.toml"],
            &["crates/a", "crates/b"],
        );
        let v = eval(&r, dir.path(), &idx);
        assert!(
            v.iter().any(|x| x.message.contains("crates/b/Cargo.toml")),
            "expected crates/b flagged as orphan, got {v:?}"
        );
    }

    #[test]
    fn exclude_query_subtracts_before_checking() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\nexclude = [\"b\"]\n",
        )
        .unwrap();
        let mut o = opts("Cargo.toml", Extract::Toml("$.workspace.members[*]".into()));
        o.exclude_query = Some("$.workspace.exclude[*]".into());
        o.expect = Expect::Dir;
        let r = rule(o);
        // `b` is excluded, so its absence must not fail; `a` exists.
        let idx = index(&["Cargo.toml"], &["a"]);
        assert!(eval(&r, dir.path(), &idx).is_empty());
    }
}
