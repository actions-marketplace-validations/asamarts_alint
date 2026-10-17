use std::cmp::Ordering;
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Maximum number of navigable secondary locations retained on one finding.
pub const MAX_RELATED_LOCATIONS: usize = 20;

/// A half-open location in original UTF-8 source text.
///
/// Lines and columns are 1-based Unicode-scalar positions. `end_*` identifies
/// the position immediately after the range, matching LSP and SARIF.
#[derive(Debug, Clone, Serialize)]
pub struct SourceRange {
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_end: Option<usize>,
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
    #[serde(skip)]
    baseline_identity_line: Option<usize>,
}

impl PartialEq for SourceRange {
    fn eq(&self, other: &Self) -> bool {
        self.byte_start == other.byte_start
            && self.byte_end == other.byte_end
            && self.start_line == other.start_line
            && self.start_column == other.start_column
            && self.end_line == other.end_line
            && self.end_column == other.end_column
    }
}

impl Eq for SourceRange {}

impl SourceRange {
    /// Construct a checked half-open source range.
    ///
    /// Coordinates are 1-based. Empty ranges are valid, but an end before the
    /// start (in line/column order), or any zero coordinate, is rejected.
    #[must_use]
    pub const fn new(
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
    ) -> Option<Self> {
        if start_line == 0
            || start_column == 0
            || end_line == 0
            || end_column == 0
            || end_line < start_line
            || (end_line == start_line && end_column < start_column)
        {
            return None;
        }
        Some(Self {
            byte_start: None,
            byte_end: None,
            start_line,
            start_column,
            end_line,
            end_column,
            baseline_identity_line: None,
        })
    }

    /// Convert a valid UTF-8 byte range to 1-based line/column coordinates.
    /// Returns `None` for reversed, out-of-bounds, or non-character-boundary
    /// offsets.
    #[must_use]
    pub fn from_byte_range(text: &str, range: Range<usize>) -> Option<Self> {
        if range.start > range.end
            || range.end > text.len()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            return None;
        }

        let mut line = 1;
        let mut column = 1;
        let mut start = None;
        let mut end = None;
        for (offset, ch) in text.char_indices() {
            if offset == range.start {
                start = Some((line, column));
            }
            if offset == range.end {
                end = Some((line, column));
                break;
            }
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        if range.start == text.len() {
            start = Some((line, column));
        }
        if range.end == text.len() {
            end = Some((line, column));
        }
        let ((start_line, start_column), (end_line, end_column)) = (start?, end?);
        Some(Self {
            byte_start: Some(range.start),
            byte_end: Some(range.end),
            start_line,
            start_column,
            end_line,
            end_column,
            baseline_identity_line: None,
        })
    }

    #[must_use]
    pub const fn byte_start(&self) -> Option<usize> {
        self.byte_start
    }

    #[must_use]
    pub const fn byte_end(&self) -> Option<usize> {
        self.byte_end
    }

    #[must_use]
    pub const fn start_line(&self) -> usize {
        self.start_line
    }

    #[must_use]
    pub const fn start_column(&self) -> usize {
        self.start_column
    }

    #[must_use]
    pub const fn end_line(&self) -> usize {
        self.end_line
    }

    #[must_use]
    pub const fn end_column(&self) -> usize {
        self.end_column
    }

    pub(crate) const fn baseline_identity_line(&self) -> Option<usize> {
        self.baseline_identity_line
    }

    pub(crate) fn set_baseline_identity_line(&mut self, line: Option<usize>) {
        self.baseline_identity_line = line;
    }

    fn sort_key(&self) -> (u8, usize, usize, usize, usize, usize, usize) {
        match (self.byte_start, self.byte_end) {
            (Some(start), Some(end)) => (
                0,
                start,
                end,
                self.start_line,
                self.start_column,
                self.end_line,
                self.end_column,
            ),
            _ => (
                1,
                self.start_line,
                self.start_column,
                self.end_line,
                self.end_column,
                0,
                0,
            ),
        }
    }
}

/// A navigable secondary location attached to a finding.
#[derive(Debug, Clone, Serialize)]
pub struct RelatedLocation {
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<SourceRange>,
    pub label: String,
}

impl RelatedLocation {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, label: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            range: None,
            label: label.into(),
        }
    }

    #[must_use]
    pub fn with_range(mut self, range: SourceRange) -> Self {
        self.range = Some(range);
        self
    }
}
impl PartialEq for RelatedLocation {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for RelatedLocation {}

impl Ord for RelatedLocation {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_paths(&self.path, &other.path)
            .then_with(|| compare_ranges(self.range.as_ref(), other.range.as_ref()))
            .then_with(|| self.label.cmp(&other.label))
    }
}

impl PartialOrd for RelatedLocation {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compare_paths(left: &Path, right: &Path) -> Ordering {
    normalized_path_bytes(left).cmp(normalized_path_bytes(right))
}

fn normalized_path_bytes(path: &Path) -> impl Iterator<Item = u8> + '_ {
    path.as_os_str()
        .as_encoded_bytes()
        .iter()
        .copied()
        .map(|byte| {
            if cfg!(windows) && byte == b'\\' {
                b'/'
            } else {
                byte
            }
        })
}
fn compare_ranges(left: Option<&SourceRange>, right: Option<&SourceRange>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.sort_key().cmp(&right.sort_key()),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_range_uses_scalar_columns_and_exclusive_end() {
        let text = "aé\n日z";
        let range = SourceRange::from_byte_range(text, 1..7).unwrap();
        assert_eq!(range.byte_start(), Some(1));
        assert_eq!(range.byte_end(), Some(7));
        assert_eq!(range.start_line(), 1);
        assert_eq!(range.start_column(), 2);
        assert_eq!(range.end_line(), 2);
        assert_eq!(range.end_column(), 2);
    }

    #[test]
    fn byte_range_rejects_non_character_boundaries() {
        assert!(SourceRange::from_byte_range("é", 1..2).is_none());
    }

    #[test]
    fn checked_ranges_reject_invalid_coordinates_and_allow_empty_ranges() {
        assert!(SourceRange::new(0, 1, 1, 1).is_none());
        assert!(SourceRange::new(1, 0, 1, 1).is_none());
        assert!(SourceRange::new(2, 1, 1, 1).is_none());
        assert!(SourceRange::new(1, 2, 1, 1).is_none());
        assert!(SourceRange::new(3, 4, 3, 4).is_some());
    }

    #[test]
    fn related_locations_sort_ranges_before_path_only_locations() {
        let mut locations = [
            RelatedLocation::new("b", "b"),
            RelatedLocation::new("a", "path only"),
            RelatedLocation::new("a", "range").with_range(SourceRange::new(2, 1, 2, 2).unwrap()),
        ];
        locations.sort();
        assert_eq!(locations[0].label, "range");
        assert_eq!(locations[1].label, "path only");
        assert_eq!(locations[2].label, "b");
    }

    #[test]
    fn related_locations_compare_normalized_full_paths_not_components() {
        let mut locations = [
            RelatedLocation::new("a/z", "nested"),
            RelatedLocation::new("a.b", "sibling"),
        ];
        locations.sort();
        assert_eq!(locations[0].path, Path::new("a.b"));
        assert_eq!(locations[1].path, Path::new("a/z"));
    }
    #[cfg(windows)]
    #[test]
    fn slash_spellings_are_the_same_normalized_location() {
        assert_eq!(
            RelatedLocation::new(r"a\b", "x"),
            RelatedLocation::new("a/b", "x")
        );
    }

    #[cfg(unix)]
    #[test]
    fn related_location_order_preserves_non_utf8_path_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt as _;

        let lower = RelatedLocation::new(PathBuf::from(OsString::from_vec(vec![b'a', 0x80])), "x");
        let upper = RelatedLocation::new(PathBuf::from(OsString::from_vec(vec![b'a', 0x81])), "x");
        assert_eq!(lower.cmp(&upper), Ordering::Less);
        assert_ne!(lower, upper);
    }
}
