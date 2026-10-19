//! Canonical fix-diff and snapshot artifacts for the policy-test runner.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use alint_core::{StagedFix, StagedKind};
use serde::Serialize;

use crate::confinement::PackageRoot;
use crate::error::{Error, Result};
use crate::policy_runner::{Mismatch, MismatchChannel, StepObservation};

#[derive(Debug)]
pub(super) struct FileSnapshot {
    bytes: Vec<u8>,
    mode: u32,
}

pub(super) fn repository_files(root: &Path) -> Result<BTreeMap<PathBuf, FileSnapshot>> {
    fn collect(
        root: &Path,
        directory: &Path,
        files: &mut BTreeMap<PathBuf, FileSnapshot>,
    ) -> Result<()> {
        for entry in std::fs::read_dir(directory).map_err(|source| Error::Io {
            path: directory.to_path_buf(),
            source,
        })? {
            let entry = entry.map_err(|source| Error::Io {
                path: directory.to_path_buf(),
                source,
            })?;
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap_or(&path);
            if relative
                .components()
                .next()
                .is_some_and(|component| component.as_os_str().eq_ignore_ascii_case(".git"))
            {
                continue;
            }
            let metadata = std::fs::symlink_metadata(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            if metadata.is_dir() {
                collect(root, &path, files)?;
            } else if metadata.is_file() {
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt as _;
                    metadata.permissions().mode()
                };
                #[cfg(not(unix))]
                let mode = 0;
                let bytes = std::fs::read(&path).map_err(|source| Error::Io {
                    path: path.clone(),
                    source,
                })?;
                files.insert(relative.to_path_buf(), FileSnapshot { bytes, mode });
            }
        }
        Ok(())
    }

    let mut files = BTreeMap::new();
    collect(root, root, &mut files)?;
    Ok(files)
}

pub(super) fn staged_between(
    mut before: BTreeMap<PathBuf, FileSnapshot>,
    mut after: BTreeMap<PathBuf, FileSnapshot>,
) -> Vec<StagedFix> {
    let common: Vec<_> = before
        .keys()
        .filter(|path| after.contains_key(*path))
        .cloned()
        .collect();
    let mut staged = Vec::new();
    for path in common {
        let old = before.remove(&path).expect("common key in before");
        let new = after.remove(&path).expect("common key in after");
        if old.bytes != new.bytes {
            staged.push(StagedFix {
                path: path.clone(),
                old: old.bytes.clone(),
                new: new.bytes.clone(),
                kind: StagedKind::Modify,
            });
        }
        if old.mode != new.mode {
            staged.push(StagedFix {
                path,
                old: new.bytes.clone(),
                new: new.bytes,
                kind: StagedKind::Chmod {
                    old_mode: old.mode,
                    new_mode: new.mode,
                },
            });
        }
    }

    let mut renamed_targets = BTreeSet::new();
    let mut renamed_sources = BTreeSet::new();
    for (from, old) in &before {
        if let Some((to, new)) = after
            .iter()
            .find(|(to, new)| !renamed_targets.contains(*to) && new.bytes == old.bytes)
        {
            renamed_sources.insert(from.clone());
            renamed_targets.insert(to.clone());
            staged.push(StagedFix {
                path: to.clone(),
                old: old.bytes.clone(),
                new: new.bytes.clone(),
                kind: StagedKind::Rename { from: from.clone() },
            });
            if old.mode != new.mode {
                staged.push(StagedFix {
                    path: to.clone(),
                    old: new.bytes.clone(),
                    new: new.bytes.clone(),
                    kind: StagedKind::Chmod {
                        old_mode: old.mode,
                        new_mode: new.mode,
                    },
                });
            }
        }
    }
    before.retain(|path, _| !renamed_sources.contains(path));
    for target in &renamed_targets {
        after.remove(target);
    }
    for (path, old) in before {
        staged.push(StagedFix {
            path,
            old: old.bytes,
            new: Vec::new(),
            kind: StagedKind::Delete,
        });
    }
    for (path, new) in after {
        staged.push(StagedFix {
            path,
            old: Vec::new(),
            new: new.bytes,
            kind: StagedKind::Create,
        });
    }
    staged.sort_by(|left, right| left.path.cmp(&right.path));
    staged
}

pub(super) fn render_fix_diff(repository: &Path, staged: &[StagedFix]) -> Result<String> {
    let mut bytes = Vec::new();
    alint_output::write_fix_diff(staged, &mut bytes).map_err(|source| Error::Io {
        path: repository.to_path_buf(),
        source,
    })?;
    String::from_utf8(bytes)
        .map_err(|error| Error::runtime(format!("canonical fix diff was not UTF-8: {error}")))
}

pub(super) fn reconcile_snapshot(
    snapshot: Option<&str>,
    package: &PackageRoot,
    observations: &[StepObservation],
    mismatches: &mut Vec<Mismatch>,
    update: bool,
) -> Result<()> {
    let Some(snapshot) = snapshot else {
        return Ok(());
    };
    let target = package
        .resolve_snapshot_target(snapshot)
        .map_err(Error::from)?;
    let rendered = render_snapshot(observations)?;
    if update {
        if mismatches.is_empty() {
            write_snapshot_atomic(&target, &rendered)?;
        }
        return Ok(());
    }
    match std::fs::read_to_string(&target) {
        Ok(actual) if normalize_lf(&actual) == rendered => {}
        Ok(_) => mismatches.push(Mismatch {
            step: None,
            channel: MismatchChannel::Snapshot,
            detail: format!("snapshot {snapshot} differs"),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            mismatches.push(Mismatch {
                step: None,
                channel: MismatchChannel::Snapshot,
                detail: format!("snapshot {snapshot} is missing"),
            });
        }
        Err(source) => {
            return Err(Error::Io {
                path: target,
                source,
            });
        }
    }
    Ok(())
}

fn render_snapshot(observations: &[StepObservation]) -> Result<String> {
    #[derive(Serialize)]
    struct Snapshot<'a> {
        schema_version: u32,
        kind: &'static str,
        steps: &'a [StepObservation],
    }
    let mut rendered = serde_json::to_string_pretty(&Snapshot {
        schema_version: 1,
        kind: "policy-test-snapshot",
        steps: observations,
    })
    .map_err(|error| {
        Error::runtime(format!("could not serialize policy-test snapshot: {error}"))
    })?;
    rendered.push('\n');
    Ok(rendered)
}

fn write_snapshot_atomic(target: &Path, contents: &str) -> Result<()> {
    if std::fs::symlink_metadata(target).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(Error::runtime(format!(
            "snapshot target {} became a symlink",
            target.display()
        )));
    }
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|source| Error::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|source| Error::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    temporary
        .write_all(contents.as_bytes())
        .map_err(|source| Error::Io {
            path: target.to_path_buf(),
            source,
        })?;
    temporary.flush().map_err(|source| Error::Io {
        path: target.to_path_buf(),
        source,
    })?;
    temporary.persist(target).map_err(|error| Error::Io {
        path: target.to_path_buf(),
        source: error.error,
    })?;
    Ok(())
}

fn normalize_lf(value: &str) -> String {
    value.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applied_diff_preserves_content_and_mode_changes() {
        let path = PathBuf::from("script.sh");
        let before = BTreeMap::from([(
            path.clone(),
            FileSnapshot {
                bytes: b"echo old\n".to_vec(),
                mode: 0o100_644,
            },
        )]);
        let after = BTreeMap::from([(
            path,
            FileSnapshot {
                bytes: b"echo new\n".to_vec(),
                mode: 0o100_755,
            },
        )]);

        let staged = staged_between(before, after);
        assert_eq!(staged.len(), 2);
        assert!(matches!(staged[0].kind, StagedKind::Modify));
        assert!(matches!(
            staged[1].kind,
            StagedKind::Chmod {
                old_mode: 0o100_644,
                new_mode: 0o100_755
            }
        ));
    }
}
