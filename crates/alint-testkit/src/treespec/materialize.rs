//! `spec → disk`. Write a [`TreeSpec`] into an existing root directory.

use std::path::Path;

use super::spec::{TreeNode, TreeSpec};
use crate::error::{Error, Result};

/// Materialize `spec` under `root`. `root` must exist and be a
/// directory; intermediate directories inside the spec are created
/// as needed. Files are overwritten if they already exist at a path
/// the spec names.
pub fn materialize(spec: &TreeSpec, root: &Path) -> Result<()> {
    if !root.is_dir() {
        return Err(Error::NotADirectory(root.to_path_buf()));
    }
    write_map(&spec.root, root)
}

fn write_map(children: &std::collections::BTreeMap<String, TreeNode>, parent: &Path) -> Result<()> {
    for (name, node) in children {
        let path = parent.join(name);
        match node {
            TreeNode::File(content) => {
                if let Some(pp) = path.parent() {
                    std::fs::create_dir_all(pp).map_err(|source| Error::Io {
                        path: pp.to_path_buf(),
                        source,
                    })?;
                }
                std::fs::write(&path, content).map_err(|source| Error::Io {
                    path: path.clone(),
                    source,
                })?;
            }
            TreeNode::Dir(sub) => {
                std::fs::create_dir_all(&path).map_err(|source| Error::Io {
                    path: path.clone(),
                    source,
                })?;
                write_map(sub, &path)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn writes_nested_tree_onto_disk() {
        let tmp = TempDir::new().unwrap();
        let spec = TreeSpec::from_yaml(
            r#"
Cargo.toml: "[package]\nname = \"x\"\n"
src:
  main.rs: "fn main() {}\n"
empty: {}
"#,
        )
        .unwrap();
        materialize(&spec, tmp.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("Cargo.toml")).unwrap(),
            "[package]\nname = \"x\"\n"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("src/main.rs")).unwrap(),
            "fn main() {}\n"
        );
        assert!(tmp.path().join("empty").is_dir());
    }

    #[test]
    fn writes_binary_magic_bytes_via_unicode_escapes() {
        // Scenario fixtures express raw binary signatures (macOS-junk magic
        // numbers) with YAML \u00NN escapes. Every code point below
        // U+0080 encodes to one byte, so the AppleDouble magic (00 05 16 07)
        // and the .DS_Store "Bud1" prefix round-trip to disk verbatim. Guards
        // the mechanism the hygiene-no-macos-junk content scenarios rely on.
        let tmp = TempDir::new().unwrap();
        let spec = TreeSpec::from_yaml(
            r#"
"._ad": "\u0000\u0005\u0016\u0007payload"
".DS_Store": "\u0000\u0000\u0000\u0001Bud1"
"#,
        )
        .unwrap();
        materialize(&spec, tmp.path()).unwrap();
        assert_eq!(
            &std::fs::read(tmp.path().join("._ad")).unwrap()[..4],
            &[0x00u8, 0x05, 0x16, 0x07],
            "AppleDouble magic must round-trip to raw bytes",
        );
        assert_eq!(
            std::fs::read(tmp.path().join(".DS_Store")).unwrap(),
            b"\x00\x00\x00\x01Bud1",
            "Bud1 magic must round-trip to raw bytes",
        );
    }

    #[test]
    fn overwrites_existing_files() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "OLD").unwrap();
        let spec = TreeSpec::from_yaml(r#"a.txt: "NEW""#).unwrap();
        materialize(&spec, tmp.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "NEW"
        );
    }

    #[test]
    fn errors_when_root_does_not_exist() {
        let bogus = std::path::PathBuf::from("/no/such/dir/here/alint-test");
        let spec = TreeSpec::from_yaml(r#"a.txt: "x""#).unwrap();
        let err = materialize(&spec, &bogus).unwrap_err();
        assert!(matches!(err, Error::NotADirectory(_)));
    }
}
