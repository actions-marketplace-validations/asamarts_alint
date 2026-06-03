//! `file_graph` — assemble the repo's *file → file* reference
//! graph from path-based edges and assert a global structural
//! property. The graph layer the 1-level cross-file kinds
//! (`registry_paths_resolve`, `import_gate`, `pair_hash`) can't
//! express. `require:` modes: `acyclic` (no dependency cycle),
//! `forbidden_edges` (layering firewall), `no_dangling` (every
//! edge resolves to an existing path), `no_orphans` (no
//! unreferenced node, save declared `roots`).
//!
//! Nodes are repo files selected by a glob; edges are extracted
//! from each node's content (`crate::extract`, regex/structured/
//! lines) and *resolved as paths* — relative to the referencing
//! file or the repo root. Bare module specifiers (no leading `.`
//! under `relative_to_file`), absolute paths, URLs, and computed/
//! interpolated references are **dropped, not mis-resolved**:
//! resolving module *names* is the package-graph non-goal.
//!
//! Pure-parse and extraction-based — it never shells out, so it
//! stays out of `SPAWNING_RULE_KINDS`. Cross-file: needs the whole
//! index (`requires_full_index`), never `--changed`-scoped.
//! Design + rationale: `docs/design/v0.12/file_dependency_graph.md`.
//!
//! ```yaml
//! # Layering — domain code must not reach into infra.
//! - id: domain-not-depend-on-infra
//!   kind: file_graph
//!   nodes: "src/**/*.ts"
//!   edges:
//!     from_content:
//!       extract: { regex: 'from\s+"(\.[^"]+)"' }
//!       resolve: relative_to_file        # | relative_to_repo_root
//!   require:
//!     forbidden_edges:
//!       - { from: "src/domain/**", to: "src/infra/**" }
//!
//! # Acyclicity — the clearest capability gap (nothing else does it).
//! - id: no-proto-import-cycles
//!   kind: file_graph
//!   nodes: "proto/**/*.proto"
//!   edges:
//!     from_content:
//!       extract: { regex: 'import\s+"([^"]+)"' }
//!       resolve: relative_to_repo_root
//!   require: acyclic
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::slice;

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use regex::Regex;
use serde::Deserialize;

use crate::extract::{Extract, ExtractSpec, extract_values, is_non_literal};

/// How a content-extracted reference string is turned into a path.
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Resolve {
    /// Join the reference onto the referencing file's directory.
    /// Only explicitly-relative refs (leading `.`) are resolved;
    /// bare specifiers (module names) are dropped.
    #[default]
    RelativeToFile,
    /// Treat the reference as a path from the repo root (the proto
    /// `import "a/b.proto"` shape).
    RelativeToRepoRoot,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FromContentSpec {
    extract: ExtractSpec,
    #[serde(default)]
    resolve: Resolve,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgesSpec {
    from_content: FromContentSpec,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ForbiddenEdgeSpec {
    from: String,
    to: String,
}

/// `require:` is either a bare string (`acyclic`) or a map
/// (`{ forbidden_edges: [...] }`). A scalar and a map are
/// structurally distinct, so an untagged enum decodes them
/// unambiguously (the proven `cross_file_value_equals` `targets:`
/// pattern — not the externally-tagged-enum-from-map trap
/// `crate::extract` documents).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RequireSpec {
    Named(NamedRequire),
    Map(RequireMap),
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum NamedRequire {
    Acyclic,
    /// Every path-shaped reference must resolve to a path on disk.
    NoDangling,
    /// No node is unreferenced (bare form — no entry-point roots).
    NoOrphans,
}

/// The map form of `require:`. A struct-of-options (validated to
/// exactly-one in `build`) rather than an externally-tagged enum,
/// so it decodes from a YAML map cleanly. The bare-string modes
/// (`acyclic`, `no_dangling`, `no_orphans` with no roots) live in
/// `NamedRequire`; the configured modes live here, and future ones
/// (`fresh: { … }`) join as additional `Option` fields.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequireMap {
    #[serde(default)]
    forbidden_edges: Option<Vec<ForbiddenEdgeSpec>>,
    #[serde(default)]
    no_orphans: Option<NoOrphansSpec>,
}

/// Options for the `no_orphans` map form: `roots` lists globs whose
/// nodes are allowed to be unreferenced (graph entry points). Bare
/// `require: no_orphans` (no roots) is the `NamedRequire` form.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoOrphansSpec {
    #[serde(default)]
    roots: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    nodes: String,
    edges: EdgesSpec,
    require: RequireSpec,
}

/// A resolved forbidden-edge prohibition: an edge whose source
/// matches `from` and whose (resolved) target matches `to` is a
/// violation. The raw globs are kept for the message.
#[derive(Debug)]
struct ForbiddenPattern {
    from: Scope,
    to: Scope,
    from_glob: String,
    to_glob: String,
}

/// The resolved structural assertion.
#[derive(Debug)]
enum Require {
    Acyclic,
    /// Every path-shaped edge must resolve to an existing path.
    NoDangling,
    /// No node is unreferenced, except those matching a `roots` glob.
    NoOrphans {
        roots: Option<Scope>,
    },
    ForbiddenEdges(Vec<ForbiddenPattern>),
}

#[derive(Debug)]
pub struct FileGraphRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    nodes: Scope,
    extract: Extract,
    resolve: Resolve,
    require: Require,
}

impl Rule for FileGraphRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // Cross-file: the graph spans the whole tree (an edge can
        // point at any node, a cycle can route through any file),
        // so the rule must see the full index, never a
        // `--changed`-scoped subset.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        // Stable node ordering → byte-identical violation output
        // (the snapshot discipline the parallel walker upholds).
        let mut nodes: Vec<PathBuf> = ctx
            .index
            .files()
            .filter(|e| self.nodes.matches(&e.path, ctx.index))
            .map(|e| e.path.to_path_buf())
            .collect();
        nodes.sort();

        Ok(match &self.require {
            Require::ForbiddenEdges(pats) => self.check_forbidden(ctx, &nodes, pats),
            Require::Acyclic => self.check_acyclic(ctx, &nodes),
            Require::NoDangling => self.check_no_dangling(ctx, &nodes),
            Require::NoOrphans { roots } => self.check_no_orphans(ctx, &nodes, roots.as_ref()),
        })
    }
}

impl FileGraphRule {
    /// One violation per (node, target) edge that any prohibition
    /// matches. A node no `from` glob selects is never even read.
    fn check_forbidden(
        &self,
        ctx: &Context<'_>,
        nodes: &[PathBuf],
        pats: &[ForbiddenPattern],
    ) -> Vec<Violation> {
        let mut out = Vec::new();
        for node in nodes {
            let applicable: Vec<&ForbiddenPattern> = pats
                .iter()
                .filter(|p| p.from.matches(node, ctx.index))
                .collect();
            if applicable.is_empty() {
                continue;
            }
            for target in self.node_targets(ctx, node, &mut out) {
                for p in &applicable {
                    if p.to.matches(&target, ctx.index) {
                        out.push(self.forbidden_violation(node, &target, p));
                    }
                }
            }
        }
        out
    }

    /// One violation per distinct dependency cycle among the
    /// nodes. Only node → node edges form the graph (an edge to a
    /// non-node file can't be part of a node cycle).
    fn check_acyclic(&self, ctx: &Context<'_>, nodes: &[PathBuf]) -> Vec<Violation> {
        let mut out = Vec::new();
        let index_of: HashMap<&Path, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, p)| (p.as_path(), i))
            .collect();

        let mut adj: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (i, node) in nodes.iter().enumerate() {
            let mut neigh: Vec<usize> = self
                .node_targets(ctx, node, &mut out)
                .iter()
                .filter_map(|t| index_of.get(t.as_path()).copied())
                .filter(|&j| j != i) // drop degenerate self-loops
                .collect();
            neigh.sort_unstable();
            neigh.dedup();
            if !neigh.is_empty() {
                adj.insert(i, neigh);
            }
        }

        for cycle in collect_cycles(&adj, nodes.len()) {
            out.push(self.cycle_violation(nodes, &cycle));
        }
        out
    }

    /// One violation per path-shaped edge whose resolved target
    /// exists nowhere in the index (as a file or a directory).
    /// References that aren't path-shaped (bare module names, URLs)
    /// are dropped, not flagged — `no_dangling` is a
    /// reference-integrity check, not a module resolver.
    fn check_no_dangling(&self, ctx: &Context<'_>, nodes: &[PathBuf]) -> Vec<Violation> {
        let mut out = Vec::new();
        let dirs: HashSet<&Path> = ctx.index.dirs().map(|e| &*e.path).collect();
        for node in nodes {
            for target in self.node_targets(ctx, node, &mut out) {
                let exists = ctx.index.contains_file(&target) || dirs.contains(target.as_path());
                if !exists {
                    out.push(self.dangling_violation(node, &target));
                }
            }
        }
        out
    }

    /// One violation per node that no *other* node references, unless
    /// it matches a `roots` glob (a declared graph entry point).
    /// Reverse-edge analysis over the node → node sub-graph.
    fn check_no_orphans(
        &self,
        ctx: &Context<'_>,
        nodes: &[PathBuf],
        roots: Option<&Scope>,
    ) -> Vec<Violation> {
        let mut out = Vec::new();
        let node_set: HashSet<&Path> = nodes.iter().map(PathBuf::as_path).collect();

        let mut referenced: HashSet<PathBuf> = HashSet::new();
        for node in nodes {
            for target in self.node_targets(ctx, node, &mut out) {
                // A self-reference can't un-orphan a node; only an
                // edge from a *different* node counts.
                if target.as_path() != node.as_path() && node_set.contains(target.as_path()) {
                    referenced.insert(target);
                }
            }
        }

        for node in nodes {
            if referenced.contains(node) || roots.is_some_and(|r| r.matches(node, ctx.index)) {
                continue;
            }
            out.push(self.orphan_violation(node));
        }
        out
    }

    /// Read one node and resolve every content reference to a path.
    /// Unreadable / unparseable nodes push a violation and yield no
    /// edges; references that don't resolve to a path are dropped.
    fn node_targets(
        &self,
        ctx: &Context<'_>,
        node: &Path,
        out: &mut Vec<Violation>,
    ) -> Vec<PathBuf> {
        let abs = ctx.root.join(node);
        let text = match crate::io::read_capped(&abs) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                out.push(Self::node_violation(
                    node,
                    &format!("is too large to analyze ({n} bytes; 256 MiB cap)"),
                ));
                return Vec::new();
            }
            Err(crate::io::ReadCapError::Io(e)) => {
                out.push(Self::node_violation(
                    node,
                    &format!("could not be read: {e}"),
                ));
                return Vec::new();
            }
        };
        let refs = match extract_values(&self.extract, &text) {
            Ok(v) => v,
            Err(e) => {
                out.push(Self::node_violation(
                    node,
                    &format!("edge extraction failed: {e}"),
                ));
                return Vec::new();
            }
        };
        refs.iter()
            .filter(|r| !is_non_literal(r))
            .filter_map(|r| resolve_ref(r, node, self.resolve))
            .collect()
    }

    fn node_violation(node: &Path, reason: &str) -> Violation {
        Violation::new(format!("file_graph node {} {reason}", node.display()))
            .with_path(node.to_path_buf())
    }

    fn forbidden_violation(&self, src: &Path, target: &Path, pat: &ForbiddenPattern) -> Violation {
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} has a forbidden dependency edge to {} (forbidden_edges: from {:?} to {:?})",
                src.display(),
                target.display(),
                pat.from_glob,
                pat.to_glob,
            )
        });
        Violation::new(msg).with_path(src.to_path_buf())
    }

    fn cycle_violation(&self, nodes: &[PathBuf], cycle: &[usize]) -> Violation {
        let mut rendered: String = cycle
            .iter()
            .map(|&i| nodes[i].display().to_string())
            .collect::<Vec<_>>()
            .join(" \u{2192} ");
        // Close the loop so the cycle reads unambiguously.
        rendered.push_str(" \u{2192} ");
        rendered.push_str(&nodes[cycle[0]].display().to_string());
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("dependency cycle ({} files): {rendered}", cycle.len()));
        Violation::new(msg).with_path(nodes[cycle[0]].clone())
    }

    fn dangling_violation(&self, src: &Path, target: &Path) -> Violation {
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} references {}, which does not resolve to any path on disk",
                src.display(),
                target.display(),
            )
        });
        Violation::new(msg).with_path(src.to_path_buf())
    }

    fn orphan_violation(&self, node: &Path) -> Violation {
        let msg = self.message.clone().unwrap_or_else(|| {
            format!(
                "{} is an orphan: no other node references it (and it is not a declared root)",
                node.display(),
            )
        });
        Violation::new(msg).with_path(node.to_path_buf())
    }
}

/// Resolve a content reference to a normalised, repo-relative path,
/// or `None` when it is not a path we should follow (a bare module
/// name, an absolute path, a URL, or one that escapes the root).
fn resolve_ref(reference: &str, from_file: &Path, mode: Resolve) -> Option<PathBuf> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }
    let joined = match mode {
        Resolve::RelativeToFile => {
            // Only explicitly-relative references are filesystem
            // paths; a bare `foo/bar` is a module specifier.
            if !reference.starts_with('.') {
                return None;
            }
            let base = from_file.parent().unwrap_or_else(|| Path::new(""));
            normalise(&base.join(reference))
        }
        Resolve::RelativeToRepoRoot => {
            if reference.starts_with('/') || reference.contains("://") {
                return None;
            }
            normalise(Path::new(reference))
        }
    };
    // Empty, or escaping the repo root via a leading `..` → drop.
    if joined.as_os_str().is_empty() || joined.components().next() == Some(Component::ParentDir) {
        return None;
    }
    Some(joined)
}

/// Collapse `a/./b` and `a/b/../c` without touching the filesystem,
/// so resolved paths key into the index (which stores walked
/// relative paths). Mirrors `registry_paths_resolve::normalise`.
fn normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    // Preserve a leading `..` so `resolve_ref` can
                    // detect (and drop) a root-escaping reference.
                    out.push("..");
                }
            }
            Component::Normal(c) => out.push(c),
            Component::RootDir | Component::Prefix(_) => out.push(comp.as_os_str()),
        }
    }
    out
}

/// Every distinct directed cycle in `adj` (node indices `0..n`),
/// each canonicalised (rotated to start at its smallest index, so
/// the same cycle always reports identically) and the whole set
/// sorted. Iterative DFS — no recursion-depth limit on deep graphs.
fn collect_cycles(adj: &BTreeMap<usize, Vec<usize>>, n: usize) -> Vec<Vec<usize>> {
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;

    let mut state = vec![WHITE; n];
    let mut cycles: BTreeSet<Vec<usize>> = BTreeSet::new();
    let empty: Vec<usize> = Vec::new();

    for start in 0..n {
        if state[start] != WHITE {
            continue;
        }
        let mut path: Vec<usize> = vec![start];
        let mut next_child: Vec<usize> = vec![0];
        state[start] = GRAY;

        while let Some(&node) = path.last() {
            let neighbors = adj.get(&node).unwrap_or(&empty);
            let child = next_child[path.len() - 1];
            if child < neighbors.len() {
                next_child[path.len() - 1] += 1;
                let next = neighbors[child];
                match state[next] {
                    WHITE => {
                        state[next] = GRAY;
                        path.push(next);
                        next_child.push(0);
                    }
                    GRAY => {
                        // Back-edge: the cycle is the path suffix
                        // from `next` to the current node.
                        if let Some(pos) = path.iter().position(|&x| x == next) {
                            cycles.insert(canonical_cycle(&path[pos..]));
                        }
                    }
                    _ => {} // BLACK: fully explored, no new cycle.
                }
            } else {
                state[node] = BLACK;
                path.pop();
                next_child.pop();
            }
        }
    }
    cycles.into_iter().collect()
}

/// Rotate a cycle so its smallest node index leads (direction
/// preserved), giving every rotation of the same cycle one
/// canonical form.
fn canonical_cycle(cycle: &[usize]) -> Vec<usize> {
    let min_pos = cycle
        .iter()
        .enumerate()
        .min_by_key(|&(_, &v)| v)
        .map_or(0, |(i, _)| i);
    let mut out = Vec::with_capacity(cycle.len());
    out.extend_from_slice(&cycle[min_pos..]);
    out.extend_from_slice(&cycle[..min_pos]);
    out
}

/// Resolve the map form of `require:` — exactly one of
/// `forbidden_edges` / `no_orphans` (the bare-string modes are
/// resolved in `build`).
fn resolve_map_require(map: RequireMap, cfg: &impl Fn(String) -> Error) -> Result<Require> {
    match (map.forbidden_edges, map.no_orphans) {
        (Some(_), Some(_)) => Err(cfg(
            "`require` map must set exactly one of `forbidden_edges` / `no_orphans`".into(),
        )),
        (None, None) => Err(cfg(
            "`require` map must set a known mode (`forbidden_edges` or `no_orphans`)".into(),
        )),
        (Some(edges), None) => {
            if edges.is_empty() {
                return Err(cfg(
                    "`require.forbidden_edges` must list at least one {from, to} pattern".into(),
                ));
            }
            let mut pats = Vec::with_capacity(edges.len());
            for (i, e) in edges.into_iter().enumerate() {
                if e.from.trim().is_empty() || e.to.trim().is_empty() {
                    return Err(cfg(format!(
                        "`require.forbidden_edges[{i}]` needs a non-empty `from` and `to`"
                    )));
                }
                let from = Scope::from_patterns(slice::from_ref(&e.from)).map_err(|err| {
                    cfg(format!("invalid `forbidden_edges[{i}].from` glob: {err}"))
                })?;
                let to = Scope::from_patterns(slice::from_ref(&e.to))
                    .map_err(|err| cfg(format!("invalid `forbidden_edges[{i}].to` glob: {err}")))?;
                pats.push(ForbiddenPattern {
                    from,
                    to,
                    from_glob: e.from,
                    to_glob: e.to,
                });
            }
            Ok(Require::ForbiddenEdges(pats))
        }
        (None, Some(spec)) => {
            if spec.roots.iter().any(|r| r.trim().is_empty()) {
                return Err(cfg(
                    "`require.no_orphans.roots` entries must not be empty".into()
                ));
            }
            let roots = if spec.roots.is_empty() {
                None
            } else {
                Some(
                    Scope::from_patterns(&spec.roots)
                        .map_err(|err| cfg(format!("invalid `no_orphans.roots` glob: {err}")))?,
                )
            };
            Ok(Require::NoOrphans { roots })
        }
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "file_graph")?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let cfg = |msg: String| Error::rule_config(&spec.id, msg);

    if opts.nodes.trim().is_empty() {
        return Err(cfg("`nodes` glob must not be empty".into()));
    }
    let nodes = Scope::from_patterns(slice::from_ref(&opts.nodes))
        .map_err(|e| cfg(format!("invalid `nodes` glob: {e}")))?;

    let extract = opts
        .edges
        .from_content
        .extract
        .resolve()
        .map_err(|e| cfg(format!("invalid `edges.from_content.extract`: {e}")))?;
    if let Extract::Regex(p) = &extract {
        Regex::new(p)
            .map_err(|e| cfg(format!("invalid `edges.from_content.extract.regex`: {e}")))?;
    }

    let require = match opts.require {
        RequireSpec::Named(NamedRequire::Acyclic) => Require::Acyclic,
        RequireSpec::Named(NamedRequire::NoDangling) => Require::NoDangling,
        RequireSpec::Named(NamedRequire::NoOrphans) => Require::NoOrphans { roots: None },
        RequireSpec::Map(map) => resolve_map_require(map, &cfg)?,
    };

    Ok(Box::new(FileGraphRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        nodes,
        extract,
        resolve: opts.edges.from_content.resolve,
        require,
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

    fn scope(pat: &str) -> Scope {
        Scope::from_patterns(slice::from_ref(&pat.to_string())).expect("valid glob")
    }

    fn forbidden(
        nodes: &str,
        regex: &str,
        resolve: Resolve,
        from: &str,
        to: &str,
    ) -> FileGraphRule {
        FileGraphRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            nodes: scope(nodes),
            extract: Extract::Regex(regex.into()),
            resolve,
            require: Require::ForbiddenEdges(vec![ForbiddenPattern {
                from: scope(from),
                to: scope(to),
                from_glob: from.into(),
                to_glob: to.into(),
            }]),
        }
    }

    fn acyclic(nodes: &str, regex: &str, resolve: Resolve) -> FileGraphRule {
        FileGraphRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            nodes: scope(nodes),
            extract: Extract::Regex(regex.into()),
            resolve,
            require: Require::Acyclic,
        }
    }

    fn eval(r: &FileGraphRule, root: &Path, idx: &FileIndex) -> Vec<Violation> {
        let ctx = Context {
            root,
            index: idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        r.evaluate(&ctx).expect("evaluate ok")
    }

    #[test]
    fn forbidden_edge_fires_on_relative_import() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/domain")).unwrap();
        std::fs::create_dir_all(root.join("src/infra")).unwrap();
        // domain reaches into infra — forbidden.
        std::fs::write(
            root.join("src/domain/order.ts"),
            "import { db } from \"../infra/db\";\n",
        )
        .unwrap();
        std::fs::write(root.join("src/infra/db.ts"), "export const db = 1;\n").unwrap();
        let idx = index(&["src/domain/order.ts", "src/infra/db.ts"]);
        let r = forbidden(
            "src/**/*.ts",
            r#"from\s+"(\.[^"]+)""#,
            Resolve::RelativeToFile,
            "src/domain/**",
            "src/infra/**",
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("src/domain/order.ts"));
        assert!(v[0].message.contains("src/infra/db"));
    }

    #[test]
    fn forbidden_edge_silent_when_layering_respected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/domain")).unwrap();
        std::fs::create_dir_all(root.join("src/infra")).unwrap();
        // infra → domain is allowed; domain imports a sibling only.
        std::fs::write(
            root.join("src/domain/order.ts"),
            "import { money } from \"./money\";\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/domain/money.ts"),
            "export const money = 1;\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/infra/db.ts"),
            "import { order } from \"../domain/order\";\n",
        )
        .unwrap();
        let idx = index(&[
            "src/domain/order.ts",
            "src/domain/money.ts",
            "src/infra/db.ts",
        ]);
        let r = forbidden(
            "src/**/*.ts",
            r#"from\s+"(\.[^"]+)""#,
            Resolve::RelativeToFile,
            "src/domain/**",
            "src/infra/**",
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn bare_specifier_is_dropped_not_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/domain")).unwrap();
        // A bare module specifier that lexically contains "infra"
        // must NOT be treated as a path edge into src/infra.
        std::fs::write(
            root.join("src/domain/order.ts"),
            "import x from \"@company/infra-sdk\";\n",
        )
        .unwrap();
        let idx = index(&["src/domain/order.ts"]);
        let r = forbidden(
            "src/**/*.ts",
            r#"from\s+"([^"]+)""#,
            Resolve::RelativeToFile,
            "src/domain/**",
            "**/infra*/**",
        );
        assert!(
            eval(&r, root, &idx).is_empty(),
            "bare specifier must not resolve to a path edge",
        );
    }

    #[test]
    fn acyclic_fires_on_two_and_three_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        // a -> b -> c -> a (a 3-cycle).
        std::fs::write(root.join("proto/a.proto"), "import \"proto/b.proto\";\n").unwrap();
        std::fs::write(root.join("proto/b.proto"), "import \"proto/c.proto\";\n").unwrap();
        std::fs::write(root.join("proto/c.proto"), "import \"proto/a.proto\";\n").unwrap();
        let idx = index(&["proto/a.proto", "proto/b.proto", "proto/c.proto"]);
        let r = acyclic(
            "proto/**/*.proto",
            r#"import\s+"([^"]+)""#,
            Resolve::RelativeToRepoRoot,
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "one distinct cycle: {v:?}");
        assert!(v[0].message.contains("dependency cycle"));
        // Canonical: starts at the smallest path (proto/a.proto).
        assert!(
            v[0].message
                .contains("proto/a.proto \u{2192} proto/b.proto")
        );
    }

    #[test]
    fn acyclic_silent_on_a_dag() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        // a -> b -> c, no back-edge.
        std::fs::write(root.join("proto/a.proto"), "import \"proto/b.proto\";\n").unwrap();
        std::fs::write(root.join("proto/b.proto"), "import \"proto/c.proto\";\n").unwrap();
        std::fs::write(root.join("proto/c.proto"), "// leaf\n").unwrap();
        let idx = index(&["proto/a.proto", "proto/b.proto", "proto/c.proto"]);
        let r = acyclic(
            "proto/**/*.proto",
            r#"import\s+"([^"]+)""#,
            Resolve::RelativeToRepoRoot,
        );
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn self_loop_is_not_a_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        std::fs::write(root.join("proto/a.proto"), "import \"proto/a.proto\";\n").unwrap();
        let idx = index(&["proto/a.proto"]);
        let r = acyclic(
            "proto/**/*.proto",
            r#"import\s+"([^"]+)""#,
            Resolve::RelativeToRepoRoot,
        );
        assert!(eval(&r, root, &idx).is_empty(), "a self-edge is degenerate");
    }

    #[test]
    fn resolve_ref_drops_non_path_references() {
        let f = Path::new("src/a/b.ts");
        // Relative-to-file: bare specifier dropped, relative kept.
        assert_eq!(
            resolve_ref("./c", f, Resolve::RelativeToFile),
            Some(PathBuf::from("src/a/c"))
        );
        assert_eq!(
            resolve_ref("../d/e", f, Resolve::RelativeToFile),
            Some(PathBuf::from("src/d/e"))
        );
        assert_eq!(resolve_ref("react", f, Resolve::RelativeToFile), None);
        // Root-escaping reference is dropped.
        assert_eq!(
            resolve_ref("../../../etc/passwd", f, Resolve::RelativeToFile),
            None
        );
        // Relative-to-root: bare path kept, absolute / URL dropped.
        assert_eq!(
            resolve_ref("a/b.proto", f, Resolve::RelativeToRepoRoot),
            Some(PathBuf::from("a/b.proto"))
        );
        assert_eq!(resolve_ref("/abs", f, Resolve::RelativeToRepoRoot), None);
        assert_eq!(
            resolve_ref("https://x/y", f, Resolve::RelativeToRepoRoot),
            None
        );
    }

    /// Generic constructor for the require modes the `forbidden` /
    /// `acyclic` helpers don't cover.
    fn mk(nodes: &str, regex: &str, resolve: Resolve, require: Require) -> FileGraphRule {
        FileGraphRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            nodes: scope(nodes),
            extract: Extract::Regex(regex.into()),
            resolve,
            require,
        }
    }

    #[test]
    fn no_dangling_fires_on_missing_then_silent_when_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/real.md"), "# real\n").unwrap();
        let r = mk(
            "docs/**/*.md",
            r"\]\((\.[^)]+)\)",
            Resolve::RelativeToFile,
            Require::NoDangling,
        );

        // a.md links a sibling that doesn't exist -> dangling.
        std::fs::write(root.join("docs/a.md"), "see [x](./missing.md)\n").unwrap();
        let idx = index(&["docs/a.md", "docs/real.md"]);
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("docs/missing.md"));
        assert!(v[0].message.contains("docs/a.md"));

        // a.md links the existing real.md -> silent.
        std::fs::write(root.join("docs/a.md"), "see [r](./real.md)\n").unwrap();
        assert!(eval(&r, root, &idx).is_empty());
    }

    #[test]
    fn no_orphans_fires_on_unreferenced_node() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        // a -> b -> c. Nothing references a, so a is the orphan.
        std::fs::write(root.join("proto/a.proto"), "import \"proto/b.proto\";\n").unwrap();
        std::fs::write(root.join("proto/b.proto"), "import \"proto/c.proto\";\n").unwrap();
        std::fs::write(root.join("proto/c.proto"), "// leaf\n").unwrap();
        let idx = index(&["proto/a.proto", "proto/b.proto", "proto/c.proto"]);
        let r = mk(
            "proto/**/*.proto",
            r#"import\s+"([^"]+)""#,
            Resolve::RelativeToRepoRoot,
            Require::NoOrphans { roots: None },
        );
        let v = eval(&r, root, &idx);
        assert_eq!(v.len(), 1, "only proto/a.proto is unreferenced: {v:?}");
        assert!(v[0].message.contains("proto/a.proto"));
        assert!(v[0].message.contains("orphan"));
    }

    #[test]
    fn no_orphans_roots_exempts_entry_point() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("proto")).unwrap();
        std::fs::write(root.join("proto/a.proto"), "import \"proto/b.proto\";\n").unwrap();
        std::fs::write(root.join("proto/b.proto"), "import \"proto/c.proto\";\n").unwrap();
        std::fs::write(root.join("proto/c.proto"), "// leaf\n").unwrap();
        let idx = index(&["proto/a.proto", "proto/b.proto", "proto/c.proto"]);
        let r = mk(
            "proto/**/*.proto",
            r#"import\s+"([^"]+)""#,
            Resolve::RelativeToRepoRoot,
            Require::NoOrphans {
                roots: Some(scope("proto/a.proto")),
            },
        );
        assert!(
            eval(&r, root, &idx).is_empty(),
            "the declared root is exempt from the orphan check"
        );
    }

    #[test]
    fn build_accepts_named_and_map_require_forms() {
        use crate::test_support::spec_yaml;
        let base = "id: t\nkind: file_graph\nnodes: \"**/*\"\nedges:\n  \
                    from_content:\n    extract:\n      regex: 'x'\n";
        for tail in [
            "require: no_dangling\nlevel: error\n",
            "require: no_orphans\nlevel: error\n",
            "require:\n  no_orphans:\n    roots: [\"src/main.rs\"]\nlevel: error\n",
        ] {
            let yaml = format!("{base}{tail}");
            assert!(build(&spec_yaml(&yaml)).is_ok(), "should build: {yaml}");
        }
        // Two map modes at once -> rejected.
        let bad = format!(
            "{base}require:\n  forbidden_edges:\n    - {{from: a, to: b}}\n  \
             no_orphans: {{}}\nlevel: error\n"
        );
        assert!(
            build(&spec_yaml(&bad)).is_err(),
            "setting two map modes must be rejected"
        );
    }
}
