//! Repository paths as relations compare them (ADR-0023, decision V21-2).

use std::fmt;
use std::path::{Component, Path};
use std::sync::Arc;

/// A repository path as relations compare it: root-relative, `/`-separated,
/// with `.` and empty segments dropped and `..` resolved. Equality is byte for
/// byte on every operating system; case folding is an explicit operation
/// ([`RelPath::eq_ignore_case`]), never a property of the host file system.
/// Paths order segment by segment, as the walker orders its index, so `a/b`
/// sorts before `a-c/x` (a whole-string byte order would put `-` first).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RelPath(Arc<str>);

impl Ord for RelPath {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .split('/')
            .map(str::as_bytes)
            .cmp(other.0.split('/').map(str::as_bytes))
    }
}

impl PartialOrd for RelPath {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl RelPath {
    /// A walked index path: its components joined with `/`, whatever the host
    /// separator. `None` for an absolute, root-escaping, empty, or non-UTF-8
    /// path.
    #[must_use]
    pub fn from_index_path(path: &Path) -> Option<Self> {
        let mut segments: Vec<&str> = Vec::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    segments.pop()?;
                }
                Component::Normal(name) => segments.push(name.to_str()?),
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        Self::join(&segments)
    }

    /// A path written in a file. It splits on `/` only, so a backslash stays
    /// part of a name on every host, and resolves `.`, `..`, repeated and
    /// trailing slashes. `None` for an absolute path (a leading `/`), a path
    /// that escapes the root, or one that resolves to nothing.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.starts_with('/') {
            return None;
        }
        let mut segments: Vec<&str> = Vec::new();
        for segment in text.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    segments.pop()?;
                }
                name => segments.push(name),
            }
        }
        Self::join(&segments)
    }

    fn join(segments: &[&str]) -> Option<Self> {
        if segments.is_empty() {
            return None;
        }
        Some(Self(Arc::from(segments.join("/"))))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Case-insensitive equality, for a comparison that opts into it. Uses
    /// Unicode lowercasing, the same on every host.
    #[must_use]
    pub fn eq_ignore_case(&self, other: &Self) -> bool {
        self.0 == other.0 || self.0.to_lowercase() == other.0.to_lowercase()
    }
}

impl fmt::Debug for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_normalizes_like_manifest_scope() {
        let cases = [
            ("docs/README.md", Some("docs/README.md")),
            ("./docs/../docs/README.md", Some("docs/README.md")),
            ("docs//api/", Some("docs/api")),
            ("a/./b/../c", Some("a/c")),
            ("a\\b", Some("a\\b")),
            ("/etc/passwd", None),
            ("../outside", None),
            ("a/../..", None),
            ("", None),
            (".", None),
            ("a/..", None),
        ];
        for (input, want) in cases {
            assert_eq!(
                RelPath::parse(input).as_ref().map(RelPath::as_str),
                want,
                "{input:?}"
            );
        }
    }

    #[test]
    fn index_paths_join_components_with_forward_slashes() {
        let path = Path::new("crates").join("core").join("lib.rs");
        assert_eq!(
            RelPath::from_index_path(&path).unwrap().as_str(),
            "crates/core/lib.rs"
        );
        assert!(RelPath::from_index_path(Path::new("/abs")).is_none());
        assert!(RelPath::from_index_path(Path::new("../up")).is_none());
        assert!(RelPath::from_index_path(Path::new("")).is_none());
    }

    #[test]
    fn paths_order_like_the_walked_index() {
        let parse = |p: &str| RelPath::parse(p).unwrap();
        assert!(parse("a/b") < parse("a-c/x"));
        assert!(parse("a") < parse("a/b"));
        assert!(parse("B") < parse("a"));
        let mut paths = [parse("a-c/x"), parse("a/b/c"), parse("a/b"), parse("a.d")];
        paths.sort();
        let ordered: Vec<&str> = paths.iter().map(RelPath::as_str).collect();
        assert_eq!(ordered, ["a/b", "a/b/c", "a-c/x", "a.d"]);
        // The same order std gives the walker's `Path`s.
        let mut std_paths: Vec<&Path> = ["a-c/x", "a/b/c", "a/b", "a.d"].map(Path::new).to_vec();
        std_paths.sort();
        assert_eq!(std_paths, ["a/b", "a/b/c", "a-c/x", "a.d"].map(Path::new));
    }

    #[test]
    fn comparison_is_byte_exact_unless_folding_is_asked_for() {
        let upper = RelPath::parse("Docs/README.md").unwrap();
        let lower = RelPath::parse("docs/readme.md").unwrap();
        assert_ne!(upper, lower);
        assert!(upper.eq_ignore_case(&lower));
        assert!(!upper.eq_ignore_case(&RelPath::parse("docs/other.md").unwrap()));
    }

    proptest::proptest! {
        #[test]
        fn parse_is_idempotent(text in "[a-c./\\\\]{0,24}") {
            if let Some(path) = RelPath::parse(&text) {
                proptest::prop_assert_eq!(RelPath::parse(path.as_str()), Some(path.clone()));
                proptest::prop_assert!(!path.as_str().starts_with('/'));
                proptest::prop_assert!(!path.as_str().ends_with('/'));
                proptest::prop_assert!(!path.as_str().split('/').any(|s| s.is_empty() || s == "." || s == ".."));
            }
        }
    }
}
