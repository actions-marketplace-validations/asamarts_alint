//! `xtask gen-facts` — emit `facts.json`, the committed surface-area
//! contract.
//!
//! `facts.json` is the single machine-readable manifest of alint's
//! surface area: version, the six headline counts (rule kinds,
//! families, bundled rulesets, auto-fix ops, output formats,
//! subcommands), and catalogue lists the README, docs, and alint.org
//! render from instead of restating numbers in prose. Every field
//! derives from the same canonical source `coverage_audit_readme_claims`
//! pins the README to, so the contract can't disagree with the README.
//!
//! Mirrors `gen_schema`: `run(false)` rewrites the file, `run(true)`
//! content-diffs the committed copy and fails on drift. Carries no
//! volatile fields (no git sha / timestamp) so it commits cleanly and
//! gates on content. Design: `docs/design/facts-json.md` (ADR-0001,
//! Phase 3 / `WS1e`). The build-time `manifest.json` is a separate,
//! deliberately-untouched artifact.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Serialize;

/// Bumped when the `facts.json` shape changes, so a downstream
/// consumer (alint.org) can pin the schema it understands.
const FORMAT_VERSION: u32 = 2;

#[derive(Serialize)]
struct Facts {
    format_version: u32,
    alint_version: String,
    counts: Counts,
    rule_kinds: Vec<String>,
    families: Vec<String>,
    categories: Vec<CategoryEntry>,
    rule_categories: BTreeMap<String, Vec<String>>,
    rule_aliases: BTreeMap<String, String>,
    rule_source_files: BTreeMap<String, String>,
    bundled_rulesets: Vec<String>,
    bundled_ruleset_sizes: BTreeMap<String, usize>,
    output_formats: Vec<String>,
    subcommands: Vec<String>,
    fact_predicates: Vec<String>,
}

#[derive(Serialize)]
struct Counts {
    rule_kinds: usize,
    families: usize,
    bundled_rulesets: usize,
    auto_fix_ops: usize,
    output_formats: usize,
    subcommands: usize,
}

/// A category in the taxonomy vocabulary: URL slug, display title, and
/// zero-based display order. Emitted so a facts.json consumer can map a
/// `rule_categories` slug back to its title/order (and validate family order)
/// without re-slugifying. Sourced from the `alint_core::Category` enum.
#[derive(Serialize)]
struct CategoryEntry {
    slug: String,
    title: String,
    order: usize,
}

fn facts_path() -> Result<PathBuf> {
    Ok(crate::workspace_root()?.join("facts.json"))
}

fn build_facts() -> Result<Facts> {
    let root = crate::workspace_root()?;
    let rule_kinds = rule_kinds(&root)?;
    let families = families(&root)?;
    let bundled_rulesets = bundled_rulesets(&root)?;
    let bundled_ruleset_sizes = bundled_ruleset_sizes(&root)?;
    let output_formats = output_formats(&root)?;
    let mut subcommands: Vec<String> = crate::docs_export::CLI_REFERENCE_SUBCMDS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    subcommands.sort();
    let fact_predicates = fact_predicates();
    let auto_fix_ops = auto_fix_ops_count(&root)?;
    let categories: Vec<CategoryEntry> = alint_core::Category::ALL
        .iter()
        .map(|c| CategoryEntry {
            slug: c.slug().to_string(),
            title: c.title().to_string(),
            order: c.order(),
        })
        .collect();
    // Sourced from the generated in-crate bridge (validated against rules.md +
    // the registry by gen-categories), so facts.json and the CLI agree. Slugs,
    // primary first.
    let rule_categories: BTreeMap<String, Vec<String>> = alint_rules::categories::KIND_CATEGORIES
        .iter()
        .map(|(kind, cats)| {
            (
                (*kind).to_string(),
                cats.iter().map(|c| c.slug().to_string()).collect(),
            )
        })
        .collect();
    // Alias -> canonical kind, from the same generated bridge. Lets a facts.json
    // consumer (e.g. alint.org's cross-repo alias-parity gate) resolve the
    // page-less legacy aliases to their canonical kind without a hand-kept map,
    // so the site's alias resolution can be gated against this authoritative set.
    let rule_aliases: BTreeMap<String, String> = alint_rules::categories::ALIAS_TO_CANONICAL
        .iter()
        .map(|(alias, canon)| ((*alias).to_string(), (*canon).to_string()))
        .collect();
    let rule_source_files = rule_source_files(&root)?;

    let counts = Counts {
        rule_kinds: rule_kinds.len(),
        families: families.len(),
        bundled_rulesets: bundled_rulesets.len(),
        auto_fix_ops,
        output_formats: output_formats.len(),
        subcommands: subcommands.len(),
    };

    Ok(Facts {
        format_version: FORMAT_VERSION,
        alint_version: env!("CARGO_PKG_VERSION").to_string(),
        counts,
        rule_kinds,
        families,
        categories,
        rule_categories,
        rule_aliases,
        rule_source_files,
        bundled_rulesets,
        bundled_ruleset_sizes,
        output_formats,
        subcommands,
        fact_predicates,
    })
}

/// Pretty JSON plus a trailing newline (git-friendly, and what
/// `--check` compares against byte-for-byte).
fn render(facts: &Facts) -> Result<String> {
    let mut s = serde_json::to_string_pretty(facts).context("serialize facts.json")?;
    s.push('\n');
    Ok(s)
}

pub fn run(check: bool) -> Result<()> {
    let facts = build_facts()?;
    let rendered = render(&facts)?;
    let path = facts_path()?;

    if check {
        let committed =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if committed != rendered {
            bail!(
                "facts.json is stale. Run `cargo run -p xtask -- gen-facts` to \
                 regenerate and commit the result."
            );
        }
        println!("facts.json is up to date");
        return Ok(());
    }

    fs::write(&path, &rendered).with_context(|| format!("write {}", path.display()))?;
    println!("wrote facts.json");
    Ok(())
}

// ---- canonical computations -----------------------------------------------
//
// Each mirrors the source `coverage_audit_readme_claims` pins the
// README to, so `facts.json`, the README, and the engine agree.

/// Distinct `kind:` values in the `all_kinds.yaml` fixture, sorted.
fn rule_kinds(root: &Path) -> Result<Vec<String>> {
    let path = root.join("crates/alint-dsl/tests/fixtures/all_kinds.yaml");
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut set = BTreeSet::new();
    for raw in text.lines() {
        let line = raw.trim_start();
        if let Some(rest) = line.strip_prefix("kind:") {
            let value = rest.trim().trim_end_matches(',');
            if !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                set.insert(value.to_string());
            }
        }
    }
    Ok(set.into_iter().collect())
}

/// Map every registered rule kind (canonical **and** alias) to the stem of the
/// Rust source file it lives in — the module before `::` in each
/// `registry.register("<kind>", <module>::<builder>)` line of `register_builtin`.
///
/// This is the authoritative, drift-proof input for alint.org's `sourceUrlOf`:
/// a kind whose source is NOT `<kind>.rs` (the 8 structured-query kinds share
/// `structured_path`, `cross_file_value_equals` aliases `cross_file`, the legacy
/// short-name aliases reuse their `file_*` file) resolves deterministically with
/// no network probe. The site gate asserts its resolution equals this map, so a
/// new shared-source or aliased kind can never silently 404 (audit P4.2 /
/// documentation-drift.md W3/W4).
///
/// Parsed from `register_builtin` — the single kind→builder table — rather than
/// a hand-kept list, and each stem is verified to exist on disk so a rename that
/// desyncs the table from the tree fails `gen-facts` here, not on the live site.
fn rule_source_files(root: &Path) -> Result<BTreeMap<String, String>> {
    let path = root.join("crates/alint-rules/src/lib.rs");
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    // Bound the scan to the register_builtin body so an unrelated `register(` in
    // another fn can't leak in. The fn ends at the first `}` in column 0.
    let start = text
        .find("pub fn register_builtin")
        .context("register_builtin not found in alint-rules/src/lib.rs")?;
    let body = &text[start..];
    let end = body
        .find("\n}")
        .context("register_builtin closing brace not found")?;
    let body = &body[..end];

    // `registry.register("<kind>", <module>::<builder>)` OR the
    // `.register_optionless(…)` variant — kind is the string literal, the source
    // stem is the module path segment before `::`. `\s` spans the newline in the
    // multi-line register calls.
    let re = Regex::new(r#"\.register(?:_optionless)?\(\s*"([A-Za-z0-9_]+)"\s*,\s*([a-z0-9_]+)::"#)
        .expect("static regex compiles");
    let src_dir = root.join("crates/alint-rules/src");
    let mut map = BTreeMap::new();
    for cap in re.captures_iter(body) {
        let kind = cap[1].to_string();
        let stem = cap[2].to_string();
        anyhow::ensure!(
            src_dir.join(format!("{stem}.rs")).exists(),
            "register_builtin maps kind `{kind}` to `{stem}::…` but \
             crates/alint-rules/src/{stem}.rs does not exist (rename drift)"
        );
        map.insert(kind, stem);
    }
    anyhow::ensure!(
        !map.is_empty(),
        "parsed zero register() entries from register_builtin"
    );
    Ok(map)
}

/// Non-meta `## ` family headings in `docs/rules.md`, sorted.
fn families(root: &Path) -> Result<Vec<String>> {
    const META: &[&str] = &[
        "Contents",
        "Fix operations",
        "Bundled rulesets",
        "Nested `.alint.yml` (monorepo layering)",
    ];
    let path = root.join("docs/rules.md");
    let md = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut set = BTreeSet::new();
    for heading in md
        .lines()
        .filter_map(|l| l.strip_prefix("## "))
        .map(str::trim)
    {
        if !META.contains(&heading) {
            set.insert(heading.to_string());
        }
    }
    Ok(set.into_iter().collect())
}

/// `.yml` files (recursive) under `rulesets/v1/`, as sorted
/// extension-stripped relative identifiers (`apache/governance`, `go`).
fn bundled_rulesets(root: &Path) -> Result<Vec<String>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeSet<String>) -> Result<()> {
        for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                walk(base, &path, out)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("yml") {
                let rel = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel);
            }
        }
        Ok(())
    }
    let dir = root.join("crates/alint-dsl/rulesets/v1");
    let mut set = BTreeSet::new();
    walk(&dir, &dir, &mut set)?;
    Ok(set.into_iter().collect())
}

/// Number of directly-declared rules in each bundled ruleset (its top-level
/// `rules:` array length; 0 for a pure-`extends:` composition), keyed by the
/// same extension-stripped id as `bundled_rulesets`. Sources per-ruleset count
/// claims on alint.org (e.g. "oss-baseline (15 rules)") so they can't drift.
fn bundled_ruleset_sizes(root: &Path) -> Result<BTreeMap<String, usize>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, usize>) -> Result<()> {
        for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                walk(base, &path, out)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("yml") {
                let rel = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/");
                let src = fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&src)
                    .with_context(|| format!("parse {} as YAML", path.display()))?;
                let count = doc
                    .get("rules")
                    .and_then(serde_yaml_ng::Value::as_sequence)
                    .map_or(0, Vec::len);
                out.insert(rel, count);
            }
        }
        Ok(())
    }
    let dir = root.join("crates/alint-dsl/rulesets/v1");
    let mut map = BTreeMap::new();
    walk(&dir, &dir, &mut map)?;
    Ok(map)
}

/// Lowercased `enum Format` variant names from `alint-output`, sorted.
fn output_formats(root: &Path) -> Result<Vec<String>> {
    let path = root.join("crates/alint-output/src/lib.rs");
    let src = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut names: Vec<String> = enum_variant_names(&src, "Format")?
        .into_iter()
        .map(|n| n.to_ascii_lowercase())
        .collect();
    names.sort();
    Ok(names)
}

/// `pub struct *Fixer` declarations (recursive) under `fixers/`.
fn auto_fix_ops_count(root: &Path) -> Result<usize> {
    fn walk(dir: &Path) -> Result<usize> {
        let mut n = 0;
        for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                n += walk(&path)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let src = fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                for line in src.lines() {
                    let line = line.trim_start();
                    if line.starts_with("pub struct ") && line.contains("Fixer") {
                        n += 1;
                    }
                }
            }
        }
        Ok(n)
    }
    walk(&root.join("crates/alint-rules/src/fixers"))
}

/// The built-in fact-kind names, taken from `alint_core::FactKind::ALL_NAMES`
/// (the single source of truth, gated against `FactKind::name()` by a test
/// in alint-core) rather than a copy here — so `facts.json` can't drift from
/// the engine's fact kinds. `ALL_NAMES` is already sorted; re-sort defensively.
fn fact_predicates() -> Vec<String> {
    let mut v: Vec<String> = alint_core::FactKind::ALL_NAMES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    v.sort();
    v
}

/// Variant identifiers of `enum <name>` in `source` (`PascalCase`, in
/// declaration order). Adapted from
/// `coverage_audit_readme_claims::count_enum_variants` — walks to the
/// matching close brace and ignores nested struct-variant fields.
fn enum_variant_names(source: &str, enum_name: &str) -> Result<Vec<String>> {
    let needle = format!("enum {enum_name} {{");
    let start = source
        .find(&needle)
        .with_context(|| format!("enum {enum_name} not found"))?
        + needle.len();
    let body = &source[start..];

    let mut depth = 1usize;
    let mut end = 0;
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    if end == 0 {
        bail!("unterminated `enum {enum_name}` body");
    }

    let outer = strip_nested_braces(&body[..end]);
    let mut names = Vec::new();
    for line in outer.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") || t.starts_with("#[") {
            continue;
        }
        if t.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            let ident: String = t
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() {
                names.push(ident);
            }
        }
    }
    Ok(names)
}

/// Blank out every top-level `{ ... }` so struct-variant field
/// identifiers aren't read as variants.
fn strip_nested_braces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '{' => {
                out.push(' ');
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                out.push(' ');
            }
            '\n' if depth > 0 => out.push('\n'),
            _ if depth > 0 => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `facts.json` must be regenerated and committed when any source
    /// changes — `--check` is what CI + preflight run.
    #[test]
    fn gen_facts_check_passes_on_committed_tree() {
        run(true).expect("gen-facts --check should pass on the committed tree");
    }

    /// Every bundled ruleset has exactly one `bundled_ruleset_sizes` entry (so
    /// the size map and the name list can't drift apart), and oss-baseline's
    /// count matches the "(15 rules)" claim alint.org renders from it
    /// (P2.1 / documentation-drift.md).
    #[test]
    fn ruleset_sizes_cover_every_ruleset_and_anchor_oss_baseline() {
        let f = build_facts().expect("build facts");
        let names: BTreeSet<&String> = f.bundled_rulesets.iter().collect();
        let sized: BTreeSet<&String> = f.bundled_ruleset_sizes.keys().collect();
        assert_eq!(
            names, sized,
            "every ruleset must have exactly one size entry"
        );
        assert_eq!(
            f.bundled_ruleset_sizes.get("oss-baseline"),
            Some(&15),
            "oss-baseline size feeds the '(15 rules)' claim on alint.org"
        );
    }

    /// `rule_source_files` covers EVERY registered kind and alias (so
    /// alint.org's `sourceUrlOf` can resolve any of them without a hand-map),
    /// each mapped stem's `.rs` exists, and the known shared-source / alias
    /// remaps land on the right file. This is the P4.2 deterministic stem gate's
    /// engine half.
    #[test]
    fn rule_source_files_cover_every_kind_and_alias_and_resolve() {
        let f = build_facts().expect("build facts");
        let root = crate::workspace_root().expect("workspace root");
        let src = root.join("crates/alint-rules/src");

        // Every canonical kind and every alias has an entry.
        for kind in &f.rule_kinds {
            assert!(
                f.rule_source_files.contains_key(kind),
                "rule_kind `{kind}` has no rule_source_files entry"
            );
        }
        for alias in f.rule_aliases.keys() {
            assert!(
                f.rule_source_files.contains_key(alias),
                "alias `{alias}` has no rule_source_files entry"
            );
        }

        // Every mapped stem points at a real source file.
        for (kind, stem) in &f.rule_source_files {
            assert!(
                src.join(format!("{stem}.rs")).exists(),
                "kind `{kind}` -> `{stem}.rs`, which does not exist"
            );
        }

        // The non-`<kind>.rs` remaps the site depends on.
        for (kind, expected) in [
            ("json_path_equals", "structured_path"),
            ("xml_path_matches", "structured_path"),
            ("cross_file_value_equals", "cross_file"),
            ("content_matches", "file_content_matches"),
            ("header", "file_header"),
            ("max_size", "file_max_size"),
            ("no_symlinks", "no_symlinks"),
        ] {
            assert_eq!(
                f.rule_source_files.get(kind).map(String::as_str),
                Some(expected),
                "`{kind}` must resolve to `{expected}.rs`"
            );
        }
    }

    /// The five list-backed counts equal their list lengths, and every
    /// list is sorted + de-duplicated.
    #[test]
    fn counts_equal_list_lengths_and_lists_are_sorted() {
        let f = build_facts().expect("build facts");
        assert_eq!(f.counts.rule_kinds, f.rule_kinds.len());
        assert_eq!(f.counts.families, f.families.len());
        assert_eq!(f.counts.bundled_rulesets, f.bundled_rulesets.len());
        assert_eq!(f.counts.output_formats, f.output_formats.len());
        assert_eq!(f.counts.subcommands, f.subcommands.len());

        for list in [
            &f.rule_kinds,
            &f.families,
            &f.bundled_rulesets,
            &f.output_formats,
            &f.subcommands,
            &f.fact_predicates,
        ] {
            let mut sorted = list.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(
                list, &sorted,
                "list must be sorted + de-duplicated: {list:?}"
            );
        }
    }

    /// Bind `facts.json` to the README's claimed numbers. The README is
    /// pinned to the canonical sources by `coverage_audit_readme_claims`,
    /// so this transitively pins `facts.json` to the same truth — and
    /// fails if this module's counting diverges from the audit's.
    #[test]
    fn counts_match_readme_claims() {
        let root = crate::workspace_root().expect("workspace_root");
        let readme = fs::read_to_string(root.join("README.md")).expect("read README.md");
        let f = build_facts().expect("build facts");

        let num_before = |marker: &str| -> usize {
            let pos = readme
                .find(marker)
                .unwrap_or_else(|| panic!("README missing marker {marker:?}"));
            let pre = readme[..pos].trim_end_matches(|c: char| c.is_whitespace() || c == '*');
            let digits: String = pre
                .chars()
                .rev()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            digits.parse().expect("integer before marker")
        };
        // "N rule kinds across M families"
        let after_kinds = {
            let m = "rule kinds across ";
            let pos = readme.find(m).expect("README 'rule kinds across'") + m.len();
            readme[pos..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse::<usize>()
                .expect("families integer")
        };

        assert_eq!(f.counts.rule_kinds, num_before("rule kinds across"));
        assert_eq!(f.counts.families, after_kinds);
        assert_eq!(
            f.counts.bundled_rulesets,
            num_before("bundled ecosystem rulesets")
        );
        assert_eq!(f.counts.auto_fix_ops, num_before("auto-fix ops"));
        assert_eq!(f.counts.output_formats, num_before("output formats"));
        assert_eq!(f.counts.subcommands, num_before("subcommands"));
    }
}
