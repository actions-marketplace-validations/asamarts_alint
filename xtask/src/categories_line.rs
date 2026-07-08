//! Shared parser for the `**Categories:**` line in `docs/rules.md`.
//!
//! Each `### kind` section may carry a `**Categories:**` line as its first
//! non-blank body line, listing the kind's categories (primary first) by title,
//! comma-separated. It is the many-to-many association SSOT (see
//! docs/design/rule-categories.md).
//!
//! CRITICAL: the line lives inside the H3 body, which docs-export both
//! summarizes (`first_sentence`) and renders (`emit_rule_page`). If it were not
//! removed, every summary and SEO description would become the literal
//! `**Categories:** ...` string. So this helper strips it, returning the raw
//! content for the categories consumers and the body with the line (and one
//! trailing blank) removed for the summarizer/renderer. Callers that need the
//! parsed categories validate the content against `alint_core::Category`.

/// The bold marker that opens a categories line.
const MARKER: &str = "**Categories:**";

/// Split a `**Categories:**` line out of an H3 body.
///
/// Only the FIRST non-blank line is eligible, so a stray `**Categories:**` in
/// prose is never stripped. Returns `(raw_content, stripped_body)` where
/// `raw_content` is the trimmed text after the marker (e.g.
/// `"Encoding, Security / Unicode sanity"`) or `None` if there is no line. When
/// there is no line the body is returned byte-identical (behavior-neutral).
pub(crate) fn split_categories_line(body: &str) -> (Option<String>, String) {
    let Some((idx, line)) = body.lines().enumerate().find(|(_, l)| !l.trim().is_empty()) else {
        return (None, body.to_string());
    };

    let trimmed = line.trim();
    if !trimmed.starts_with(MARKER) {
        return (None, body.to_string());
    }
    let content = trimmed[MARKER.len()..].trim().to_string();

    // Rebuild the body without the marker line and one immediately-following
    // blank line (so we don't leave a double blank at the top).
    let mut out = String::with_capacity(body.len());
    let mut collapse_blank = true;
    for (i, l) in body.lines().enumerate() {
        if i == idx {
            continue;
        }
        if i > idx && collapse_blank {
            collapse_blank = false;
            if l.trim().is_empty() {
                continue;
            }
        }
        out.push_str(l);
        out.push('\n');
    }
    (Some(content), out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_marker_returns_body_unchanged() {
        let body = "Every glob match must exist.\n\nMore prose.\n";
        let (cats, out) = split_categories_line(body);
        assert_eq!(cats, None);
        assert_eq!(out, body, "no-marker case must be byte-identical");
    }

    #[test]
    fn strips_marker_line_and_following_blank() {
        let body = "**Categories:** Encoding, Security / Unicode sanity\n\nThe real summary.\n";
        let (cats, out) = split_categories_line(body);
        assert_eq!(cats.as_deref(), Some("Encoding, Security / Unicode sanity"));
        assert_eq!(out, "The real summary.\n");
    }

    #[test]
    fn handles_leading_blank_lines_before_marker() {
        let body = "\n**Categories:** Content\n\nSummary here.\n";
        let (cats, out) = split_categories_line(body);
        assert_eq!(cats.as_deref(), Some("Content"));
        // The leading blank is preserved; the marker + its trailing blank go.
        assert_eq!(out, "\nSummary here.\n");
    }

    #[test]
    fn marker_only_eligible_as_first_non_blank_line() {
        // A `**Categories:**` string deeper in the prose is NOT stripped.
        let body = "The summary sentence.\n\n**Categories:** not a real marker here.\n";
        let (cats, out) = split_categories_line(body);
        assert_eq!(cats, None);
        assert_eq!(out, body);
    }

    #[test]
    fn single_category_line() {
        let body = "**Categories:** Existence\n\nEvery glob match must exist.\n";
        let (cats, out) = split_categories_line(body);
        assert_eq!(cats.as_deref(), Some("Existence"));
        assert_eq!(out, "Every glob match must exist.\n");
    }
}
