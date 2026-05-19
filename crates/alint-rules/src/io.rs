//! Shared I/O helpers for content-reading rules.

use std::io::{Read as _, Seek, SeekFrom};
use std::path::Path;

/// How much of a file to sample when classifying text vs. binary.
pub const TEXT_INSPECT_LEN: usize = 8 * 1024;

/// Read up to `TEXT_INSPECT_LEN` bytes from the start of a file. Returned
/// `Ok(None)` means the file was empty; `Err` is propagated I/O error.
pub fn read_prefix(path: &Path) -> std::io::Result<Vec<u8>> {
    read_prefix_n(path, TEXT_INSPECT_LEN)
}

/// Read up to `n` bytes from the start of `path`. Used by rules that
/// only need to inspect a leading window — `executable_has_shebang`
/// (2 bytes for `#!`), `file_starts_with` (`pattern.len()` bytes).
/// Reads less than `n` if the file is shorter; returns the actual byte
/// count in the returned `Vec`'s length.
pub fn read_prefix_n(path: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; n];
    let read = file.read(&mut buf)?;
    buf.truncate(read);
    Ok(buf)
}

/// Read up to `n` bytes from the END of `path`. Used by rules that
/// only need to inspect the tail — `file_ends_with` (`pattern.len()`
/// bytes). Returns the actual byte count in the returned `Vec`'s
/// length; fewer than `n` bytes if the file is shorter. Files smaller
/// than `n` are read whole.
pub fn read_suffix_n(path: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let len = file.seek(SeekFrom::End(0))?;
    // 32-bit platforms: `usize::MAX < u64::MAX`, so a > 4 GiB
    // file would truncate. `try_from` falls back to reading the
    // requested `n` (which is bounded to a sane caller value)
    // when the conversion fails.
    let to_read = usize::try_from(len).unwrap_or(n).min(n);
    file.seek(SeekFrom::Start(len - to_read as u64))?;
    let mut buf = vec![0u8; to_read];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

/// Classification of a file's contents. Computed lazily — callers check the
/// subset they care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    Text,
    Binary,
}

pub fn classify_bytes(bytes: &[u8]) -> Classification {
    match content_inspector::inspect(bytes) {
        content_inspector::ContentType::BINARY => Classification::Binary,
        _ => Classification::Text,
    }
}

/// Hard cap on a single whole-file read by the cross-file /
/// structured rule kinds (`registry_paths_resolve`,
/// `cross_file_value_equals`, `pair_hash`, `generated_file_fresh`).
/// Generous — every realistic manifest / generated file is orders
/// of magnitude smaller — yet bounded so a hostile or accidental
/// multi-GB file in a linted repo yields a clear violation
/// instead of OOM-ing the run.
pub const MAX_ANALYZE_BYTES: u64 = 256 * 1024 * 1024;

/// Failure of [`read_capped`]: the file exceeds
/// [`MAX_ANALYZE_BYTES`] (carrying its size), or an ordinary I/O
/// error (kept distinct so callers turn "too large" into a clear
/// violation rather than reusing their not-found / skip path).
#[derive(Debug)]
pub enum ReadCapError {
    TooLarge(u64),
    Io(std::io::Error),
}

/// Read a whole file, refusing (via a cheap `metadata` stat, so
/// the oversized bytes are never read) anything larger than
/// `max`.
fn read_capped_with(path: &Path, max: u64) -> Result<Vec<u8>, ReadCapError> {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > max => Err(ReadCapError::TooLarge(m.len())),
        Ok(_) => std::fs::read(path).map_err(ReadCapError::Io),
        Err(e) => Err(ReadCapError::Io(e)),
    }
}

/// Whole-file read bounded by [`MAX_ANALYZE_BYTES`]. Used by the
/// cross-file / structured rules for the manifest / source /
/// target / committed-file reads they do themselves.
pub fn read_capped(path: &Path) -> Result<Vec<u8>, ReadCapError> {
    read_capped_with(path, MAX_ANALYZE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_capped_returns_bytes_under_cap() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"hello").unwrap();
        match read_capped(&p) {
            Ok(b) => assert_eq!(b, b"hello"),
            _ => panic!("expected Bytes under the cap"),
        }
    }

    #[test]
    fn read_capped_with_rejects_over_cap_without_reading() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big");
        std::fs::write(&p, b"0123456789").unwrap();
        match read_capped_with(&p, 4) {
            Err(ReadCapError::TooLarge(n)) => assert_eq!(n, 10),
            _ => panic!("a 10-byte file must exceed a 4-byte cap"),
        }
    }

    #[test]
    fn read_capped_missing_path_is_io_error() {
        let dir = tempfile::tempdir().unwrap();
        match read_capped(&dir.path().join("nope")) {
            Err(ReadCapError::Io(_)) => {}
            _ => panic!("a missing path must be an Io error"),
        }
    }
}
