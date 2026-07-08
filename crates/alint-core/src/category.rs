//! The rule-category vocabulary.
//!
//! `Category` is the single, closed, typed source of truth for the rule
//! taxonomy. A rule kind can belong to several categories (many-to-many); the
//! associations live in `docs/rules.md` `**Categories:**` lines and are
//! generated into an in-crate table (`alint-rules`), but the VOCABULARY, its
//! display order, its titles, and its URL slugs live here so every consumer
//! (facts.json, docs-export, gen-model, the CLI) imports one definition.
//!
//! Variants are declared in DISPLAY order, which equals the `## ` family-heading
//! sequence in `docs/rules.md`. The derived `Ord` therefore sorts categories
//! into display order, and [`Category::order`] returns that position. An xtask
//! gate asserts the declaration order matches the rules.md H2 sequence, and that
//! [`Category::slug`] equals `docs_export::slugify(title)`.

/// A rule category (a "family" in the docs). Closed set: adding one is a
/// deliberate edit here plus a title/slug review, not an accidental string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Existence,
    Content,
    StructuredQuery,
    Naming,
    TextHygiene,
    SecurityUnicodeSanity,
    Encoding,
    Structure,
    PortableMetadata,
    UnixMetadata,
    GitHygiene,
    CrossFile,
    PluginTier1,
}

impl Category {
    /// Every category, in display order.
    pub const ALL: [Category; 13] = [
        Category::Existence,
        Category::Content,
        Category::StructuredQuery,
        Category::Naming,
        Category::TextHygiene,
        Category::SecurityUnicodeSanity,
        Category::Encoding,
        Category::Structure,
        Category::PortableMetadata,
        Category::UnixMetadata,
        Category::GitHygiene,
        Category::CrossFile,
        Category::PluginTier1,
    ];

    /// All categories, in display order.
    pub fn all() -> &'static [Category] {
        &Self::ALL
    }

    /// The display title, matching the `## ` heading in `docs/rules.md` exactly.
    pub fn title(self) -> &'static str {
        match self {
            Category::Existence => "Existence",
            Category::Content => "Content",
            Category::StructuredQuery => "Structured query",
            Category::Naming => "Naming",
            Category::TextHygiene => "Text hygiene",
            Category::SecurityUnicodeSanity => "Security / Unicode sanity",
            Category::Encoding => "Encoding",
            Category::Structure => "Structure",
            Category::PortableMetadata => "Portable metadata",
            Category::UnixMetadata => "Unix metadata",
            Category::GitHygiene => "Git hygiene",
            Category::CrossFile => "Cross-file",
            Category::PluginTier1 => "Plugin (tier 1)",
        }
    }

    /// The URL slug. Must equal `docs_export::slugify(self.title())` (gated).
    pub fn slug(self) -> &'static str {
        match self {
            Category::Existence => "existence",
            Category::Content => "content",
            Category::StructuredQuery => "structured-query",
            Category::Naming => "naming",
            Category::TextHygiene => "text-hygiene",
            Category::SecurityUnicodeSanity => "security-unicode-sanity",
            Category::Encoding => "encoding",
            Category::Structure => "structure",
            Category::PortableMetadata => "portable-metadata",
            Category::UnixMetadata => "unix-metadata",
            Category::GitHygiene => "git-hygiene",
            Category::CrossFile => "cross-file",
            Category::PluginTier1 => "plugin-tier-1",
        }
    }

    /// Zero-based display position (index into [`Category::ALL`]).
    ///
    /// Exhaustive `match` on purpose: adding a variant is a compile error here
    /// (as in `title`/`slug`), and the `order() == ALL index` test guards these
    /// indices against a reorder of `ALL`. No panic path, unlike
    /// `ALL.position(..).unwrap()`.
    pub fn order(self) -> usize {
        match self {
            Category::Existence => 0,
            Category::Content => 1,
            Category::StructuredQuery => 2,
            Category::Naming => 3,
            Category::TextHygiene => 4,
            Category::SecurityUnicodeSanity => 5,
            Category::Encoding => 6,
            Category::Structure => 7,
            Category::PortableMetadata => 8,
            Category::UnixMetadata => 9,
            Category::GitHygiene => 10,
            Category::CrossFile => 11,
            Category::PluginTier1 => 12,
        }
    }

    /// Parse a category from its display title (the form used on a
    /// `**Categories:**` line). Case- and surrounding-whitespace-sensitive on
    /// the canonical title.
    pub fn from_title(title: &str) -> Option<Category> {
        let t = title.trim();
        Self::ALL.iter().copied().find(|c| c.title() == t)
    }

    /// Parse a category from its URL slug.
    pub fn from_slug(slug: &str) -> Option<Category> {
        Self::ALL.iter().copied().find(|c| c.slug() == slug)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_has_thirteen_in_declaration_order() {
        assert_eq!(Category::ALL.len(), 13);
        for (i, c) in Category::ALL.iter().enumerate() {
            assert_eq!(c.order(), i, "order() must equal ALL index for {c:?}");
        }
    }

    #[test]
    fn ord_matches_display_order() {
        let mut sorted = Category::ALL;
        sorted.sort();
        assert_eq!(
            sorted,
            Category::ALL,
            "derived Ord must equal display order"
        );
        assert!(Category::Existence < Category::PluginTier1);
    }

    #[test]
    fn titles_and_slugs_are_unique() {
        let mut titles: Vec<&str> = Category::ALL.iter().map(|c| c.title()).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), 13, "titles must be unique");

        let mut slugs: Vec<&str> = Category::ALL.iter().map(|c| c.slug()).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), 13, "slugs must be unique");
    }

    #[test]
    fn from_title_round_trips_and_trims() {
        for c in Category::ALL {
            assert_eq!(Category::from_title(c.title()), Some(c));
            assert_eq!(Category::from_title(&format!("  {}  ", c.title())), Some(c));
        }
        assert_eq!(Category::from_title("security / unicode sanity"), None); // case-sensitive
        assert_eq!(Category::from_title("Nonsense"), None);
    }

    #[test]
    fn from_slug_round_trips() {
        for c in Category::ALL {
            assert_eq!(Category::from_slug(c.slug()), Some(c));
        }
        assert_eq!(Category::from_slug("nope"), None);
    }
}
