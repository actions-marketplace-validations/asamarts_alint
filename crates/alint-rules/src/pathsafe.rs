//! Lexical path confinement — keep a config-author-controlled path
//! inside the repo root so a rule can never read or resolve a file
//! outside the tree (the untrusted-`extends:` threat the
//! `SPAWNING_RULE_KINDS` gate also defends against). Pure lexical, no
//! filesystem access. Design: `docs/design/v0.12/path-confinement.md`.

use std::path::{Component, Path, PathBuf};

/// Normalise `p` lexically (collapsing `.` and `a/../b`) and return it
/// **only if it stays within the repo root**.
///
/// Returns `None` when the path escapes the root:
/// - an absolute component (`RootDir` / Windows `Prefix`) — because
///   `root.join(absolute)` discards `root`, so reading it would touch
///   an arbitrary host path;
/// - a `..` that cannot pop a real component (caught *during* the
///   walk, so `../../escape` and `a/../../x` are rejected, not merely
///   inspected after the fact);
/// - a result that collapses to empty (`.`, `a/..`) — the root itself
///   is never a valid edge / target / reference.
///
/// A `Some(_)` result is guaranteed root-relative: safe to
/// `root.join(..)` and to look up in the `FileIndex`.
pub(crate) fn normalize_confined(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                // A `..` that can't pop a real component escapes root.
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(c) => out.push(c),
            // Absolute (Unix root or Windows prefix) escapes by
            // definition — never let it reach `root.join`.
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if out.as_os_str().is_empty() {
        return None;
    }
    Some(out)
}

/// The verdict for a config-derived *read* path, accounting for the
/// rule's `allow_out_of_root` permission (see
/// `docs/design/v0.12/allow_out_of_root.md`).
pub(crate) enum Confined {
    /// In-tree (lexically normalised) — read as today.
    In(PathBuf),
    /// Escapes the root, but the rule is permitted to read it. The
    /// caller reads `root.join(path)` (absolute → itself; `../../x` →
    /// up) and emits an informational note via [`out_of_root_note`].
    AllowedEscape(PathBuf),
    /// Escapes the root and the rule is not permitted — the caller
    /// emits an "escapes the repo root" violation and does not read.
    Denied,
}

/// Confine a config-derived read path, honouring an
/// `allow_out_of_root` permission. `allow_escape` is the per-rule
/// flag the loader resolved from the top-level policy; it is `false`
/// for every rule unless the user's own top-level config opted the
/// rule (or its kind) in.
pub(crate) fn confine(path: &Path, allow_escape: bool) -> Confined {
    match normalize_confined(path) {
        Some(p) => Confined::In(p),
        None if allow_escape => Confined::AllowedEscape(path.to_path_buf()),
        None => Confined::Denied,
    }
}

/// The informational-note message for a permitted out-of-root read.
pub(crate) fn out_of_root_note(path: &Path) -> String {
    format!(
        "reading out-of-root path {} — permitted by `allow_out_of_root`",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::normalize_confined;
    use std::path::{Path, PathBuf};

    fn confined(s: &str) -> Option<PathBuf> {
        normalize_confined(Path::new(s))
    }

    #[test]
    fn in_tree_paths_normalise_and_pass() {
        assert_eq!(confined("a/b.rs"), Some(PathBuf::from("a/b.rs")));
        assert_eq!(confined("./a/./b"), Some(PathBuf::from("a/b")));
        assert_eq!(confined("a/x/../b"), Some(PathBuf::from("a/b")));
        // pops back to root then descends — stays in-tree.
        assert_eq!(confined("a/../b"), Some(PathBuf::from("b")));
    }

    #[test]
    fn absolute_paths_are_rejected() {
        // root.join(absolute) would discard root — the read-oracle.
        assert_eq!(confined("/etc/passwd"), None);
        assert_eq!(confined("/tmp/secret.txt"), None);
    }

    #[test]
    fn root_escaping_dotdot_is_rejected_including_cancellation() {
        assert_eq!(confined("../x"), None);
        // The double-dot-cancellation escape a first-component check
        // misses: `../../escape` must NOT collapse to in-tree `escape`.
        assert_eq!(confined("../../escape"), None);
        assert_eq!(confined("a/../../x"), None);
        assert_eq!(confined("a/b/../../../c"), None);
    }

    #[test]
    fn empty_or_root_collapse_is_rejected() {
        assert_eq!(confined(""), None);
        assert_eq!(confined("."), None);
        assert_eq!(confined("a/.."), None); // collapses to the root itself
    }

    #[cfg(windows)]
    #[test]
    fn windows_prefix_and_unc_paths_are_rejected() {
        // On Windows a drive-letter `Prefix` or a UNC `\\server\share`
        // is absolute → escapes the root, same as a Unix `/etc`. (On
        // Unix these parse as ordinary `Normal` components, so the test
        // is Windows-only.)
        assert_eq!(confined(r"C:\Windows\System32"), None);
        assert_eq!(confined(r"\\server\share\x"), None);
    }
}
