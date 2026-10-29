//! Shared fixture for literal repository-fact performance benchmarks.

use std::path::PathBuf;

use alint_core::FileEntry;
use alint_core::facts::{FactKind, FactSpec, FileContentMatchesFact, OneOrMany};

pub const EXISTENCE_FACTS: usize = 8;
pub const CONTENT_FACTS: usize = 12;
pub const FACTS: usize = EXISTENCE_FACTS + CONTENT_FACTS;

#[must_use]
pub fn build_entries(files: usize) -> Vec<FileEntry> {
    (0..files)
        .map(|index| FileEntry {
            path: PathBuf::from(format!("packages/p{}/src/file_{index}.rs", index % 5_000)).into(),
            is_dir: false,
            size: 1_024,
        })
        .collect()
}

#[must_use]
pub fn absent_literal_facts() -> Vec<FactSpec> {
    let mut facts = Vec::with_capacity(FACTS);
    for index in 0..EXISTENCE_FACTS {
        facts.push(FactSpec {
            id: format!("missing_exists_{index}"),
            kind: FactKind::AnyFileExists {
                any_file_exists: OneOrMany::One(format!(".missing-anchor-{index}")),
            },
        });
    }
    for index in 0..CONTENT_FACTS {
        facts.push(FactSpec {
            id: format!("missing_content_{index}"),
            kind: FactKind::FileContentMatches {
                file_content_matches: FileContentMatchesFact {
                    paths: OneOrMany::One(format!(".missing-content-{index}")),
                    extract: None,
                    pattern: "needle".to_string(),
                },
            },
        });
    }
    facts
}
