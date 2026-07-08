//! `xtask gen-categories` — generate the in-crate kind-to-category bridge from
//! the `**Categories:**` lines in `docs/rules.md`, validated against the
//! `alint_core::Category` vocabulary and the live rule registry.
//!
//! Mirrors `gen-schema`'s committed in-crate artifact + `--check` gate: the CLI
//! (`alint rules`, `alint list --category`) reads categories at runtime, but
//! rules.md is a docs-tree file the binary never ships, so the associations are
//! generated into `crates/alint-rules/src/categories_gen.rs` and committed.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;

use alint_core::Category;
use anyhow::{Context, Result, bail};

const RULES_MD: &str = "docs/rules.md";
const GEN_PATH: &str = "crates/alint-rules/src/categories_gen.rs";

const META_FAMILIES: &[&str] = &[
    "Contents",
    "Fix operations",
    "Bundled rulesets",
    "Nested `.alint.yml` (monorepo layering)",
];

struct KindCats {
    kind: String,
    cats: Vec<Category>,
}

/// (canonical kind -> categories, alias -> canonical kind).
type Bridge = (Vec<KindCats>, Vec<(String, String)>);

pub fn run(check: bool) -> Result<()> {
    let root = crate::workspace_root()?;
    let md = fs::read_to_string(root.join(RULES_MD)).with_context(|| format!("read {RULES_MD}"))?;

    let (kind_cats, alias_to_canonical) = parse(&md)?;
    validate_against_registry(&kind_cats, &alias_to_canonical)?;

    let rendered = render(&kind_cats, &alias_to_canonical);
    let path = root.join(GEN_PATH);

    if check {
        let committed =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if committed != rendered {
            bail!(
                "{GEN_PATH} is stale. Run `cargo run -p xtask -- gen-categories` to regenerate \
                 and commit the result."
            );
        }
        println!(
            "{GEN_PATH} is up to date; the category bridge matches rules.md + the registry ({} \
             canonical kinds, {} aliases)",
            kind_cats.len(),
            alias_to_canonical.len()
        );
        return Ok(());
    }

    fs::write(&path, &rendered).with_context(|| format!("write {}", path.display()))?;
    println!("wrote {GEN_PATH}");
    Ok(())
}

/// Parse rules.md into (canonical kind -> categories, alias -> canonical).
fn parse(md: &str) -> Result<Bridge> {
    // Collect (family, h3-title, h3-body) sections.
    let mut family: Option<Category> = None;
    let mut sections: Vec<(Category, String, String)> = Vec::new();
    let mut cur: Option<(Category, String, String)> = None;

    for line in md.lines() {
        if !line.starts_with("### ") && line.starts_with("## ") {
            if let Some(s) = cur.take() {
                sections.push(s);
            }
            let h2 = line[3..].trim();
            family = if META_FAMILIES.contains(&h2) {
                None
            } else {
                Some(Category::from_title(h2).ok_or_else(|| {
                    anyhow::anyhow!("rules.md family heading {h2:?} is not a Category variant")
                })?)
            };
        } else if let Some(h3) = line.strip_prefix("### ") {
            if let Some(s) = cur.take() {
                sections.push(s);
            }
            if let Some(fam) = family {
                cur = Some((fam, h3.trim().to_string(), String::new()));
            }
        } else if let Some((_, _, body)) = cur.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some(s) = cur.take() {
        sections.push(s);
    }

    let mut kind_cats: Vec<KindCats> = Vec::new();
    let mut alias_to_canonical: Vec<(String, String)> = Vec::new();

    for (fam, title, body) in &sections {
        let (canon, aliases) = parse_h3_title(title);
        if canon.is_empty() {
            continue;
        }
        let (content, _) = crate::categories_line::split_categories_line(body);
        let content = content.ok_or_else(|| {
            anyhow::anyhow!("rule kind(s) {canon:?} have no `**Categories:**` line in {RULES_MD}")
        })?;
        let cats = parse_categories(&content, *fam)
            .with_context(|| format!("`**Categories:**` line for {canon:?}"))?;
        for k in &canon {
            kind_cats.push(KindCats {
                kind: k.clone(),
                cats: cats.clone(),
            });
        }
        for a in &aliases {
            alias_to_canonical.push((a.clone(), canon[0].clone()));
        }
    }

    kind_cats.sort_by(|a, b| a.kind.cmp(&b.kind));
    alias_to_canonical.sort();
    Ok((kind_cats, alias_to_canonical))
}

/// Split an H3 title into (canonical kinds, alias kinds). Aliases are the
/// backticked names inside a trailing `(alias: ...)`; canonical are the rest.
fn parse_h3_title(title: &str) -> (Vec<String>, Vec<String>) {
    let (canon_part, alias_part) = match title.split_once("(alias:") {
        Some((a, b)) => (a, b),
        None => (title, ""),
    };
    (backticked(canon_part), backticked(alias_part))
}

/// The backtick-delimited tokens in `s` (odd split segments).
fn backticked(s: &str) -> Vec<String> {
    s.split('`')
        .enumerate()
        .filter(|(i, part)| i % 2 == 1 && !part.is_empty())
        .map(|(_, part)| part.to_string())
        .collect()
}

/// Parse a `**Categories:**` content string into categories, validating the
/// vocabulary, that the primary (the section's family) is listed first, and no
/// duplicates.
fn parse_categories(content: &str, primary: Category) -> Result<Vec<Category>> {
    let mut cats = Vec::new();
    for tok in content.split(',') {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        let c = Category::from_title(t).ok_or_else(|| {
            anyhow::anyhow!("unknown category {t:?} (must be one of the family titles)")
        })?;
        cats.push(c);
    }
    if cats.is_empty() {
        bail!("empty `**Categories:**` line");
    }
    if cats[0] != primary {
        bail!(
            "the primary category must be listed FIRST and equal the section's family {:?}; got {:?}",
            primary.title(),
            cats.iter().map(|c| c.title()).collect::<Vec<_>>()
        );
    }
    let mut seen = BTreeSet::new();
    for c in &cats {
        if !seen.insert(*c) {
            bail!("category {:?} listed twice", c.title());
        }
    }
    Ok(cats)
}

/// Every canonical registered kind must have a categories entry, and vice versa;
/// every alias must be a registered kind mapping to a documented canonical.
fn validate_against_registry(
    kind_cats: &[KindCats],
    alias_to_canonical: &[(String, String)],
) -> Result<()> {
    let registry = alint_rules::builtin_registry();
    let known: BTreeSet<String> = registry.known_kinds().map(str::to_string).collect();
    let alias_set: BTreeSet<&str> = alias_to_canonical.iter().map(|(a, _)| a.as_str()).collect();
    let canonical_registry: BTreeSet<&String> = known
        .iter()
        .filter(|k| !alias_set.contains(k.as_str()))
        .collect();
    let documented: BTreeSet<&String> = kind_cats.iter().map(|kc| &kc.kind).collect();

    let missing: Vec<&&String> = canonical_registry.difference(&documented).collect();
    let extra: Vec<&&String> = documented.difference(&canonical_registry).collect();
    if !missing.is_empty() || !extra.is_empty() {
        bail!(
            "category coverage mismatch vs the registry.\n  canonical kinds with no \
             `**Categories:**` line (add one to {RULES_MD}): {missing:?}\n  documented but not a \
             canonical registered kind: {extra:?}"
        );
    }

    for (a, canon) in alias_to_canonical {
        if !known.contains(a.as_str()) {
            bail!("alias {a:?} in {RULES_MD} is not a registered rule kind");
        }
        if !documented.contains(canon) {
            bail!("alias {a:?} maps to {canon:?}, which has no categories entry");
        }
    }
    Ok(())
}

fn render(kind_cats: &[KindCats], alias_to_canonical: &[(String, String)]) -> String {
    let mut s = String::new();
    s.push_str("//! @generated by `cargo run -p xtask -- gen-categories`. DO NOT EDIT.\n");
    s.push_str("//!\n");
    s.push_str("//! Kind-to-category bridge derived from the `**Categories:**` lines in\n");
    s.push_str("//! docs/rules.md, validated against `alint_core::Category` and the registry.\n");
    s.push_str("//! Regenerate with `cargo run -p xtask -- gen-categories`; gated by `--check`.\n");
    s.push('\n');
    s.push_str("use alint_core::Category;\n\n");
    s.push_str("/// Canonical rule kind -> its categories, primary first. Sorted by kind.\n");
    s.push_str("#[rustfmt::skip]\n");
    s.push_str("pub static KIND_CATEGORIES: &[(&str, &[Category])] = &[\n");
    for kc in kind_cats {
        let cats: Vec<String> = kc.cats.iter().map(|c| format!("Category::{c:?}")).collect();
        let _ = writeln!(s, "    ({:?}, &[{}]),", kc.kind, cats.join(", "));
    }
    s.push_str("];\n\n");
    s.push_str("/// Alias spelling -> canonical kind. Sorted by alias.\n");
    s.push_str("#[rustfmt::skip]\n");
    s.push_str("pub static ALIAS_TO_CANONICAL: &[(&str, &str)] = &[\n");
    for (a, c) in alias_to_canonical {
        let _ = writeln!(s, "    ({a:?}, {c:?}),");
    }
    s.push_str("];\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_h3_title_splits_canonical_and_alias() {
        let (c, a) = parse_h3_title("`file_content_matches` (alias: `content_matches`)");
        assert_eq!(c, vec!["file_content_matches"]);
        assert_eq!(a, vec!["content_matches"]);

        let (c, a) = parse_h3_title("`file_starts_with` / `file_ends_with`");
        assert_eq!(c, vec!["file_starts_with", "file_ends_with"]);
        assert!(a.is_empty());
    }

    #[test]
    fn parse_categories_requires_primary_first() {
        let ok = parse_categories("Content, Security / Unicode sanity", Category::Content).unwrap();
        assert_eq!(ok, vec![Category::Content, Category::SecurityUnicodeSanity]);

        // primary not first
        assert!(parse_categories("Security / Unicode sanity, Content", Category::Content).is_err());
        // unknown token
        assert!(parse_categories("Bananas", Category::Content).is_err());
        // duplicate
        assert!(parse_categories("Content, Content", Category::Content).is_err());
    }
}
