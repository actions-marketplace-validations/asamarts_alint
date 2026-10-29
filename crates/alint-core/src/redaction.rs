//! Secret-safe report markers.
//!
//! Markers deliberately hash a structural locator, never the redacted value. A
//! value digest would be an offline dictionary oracle for low-entropy secrets.

use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"alint-redaction-marker-v1\0";
const DIGEST_HEX_LEN: usize = 12;

/// Render a stable short marker for a non-secret structural locator.
///
/// `locator` must identify where the evidence came from (rule, path, query,
/// role), not contain the evidence itself.
#[must_use]
pub fn redaction_marker(locator: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update(locator.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(DIGEST_HEX_LEN);
    for byte in digest.iter().take(DIGEST_HEX_LEN / 2) {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
    }
    format!("<redacted:{hex}>")
}

/// Replace non-empty evidence strings with one locator-derived marker.
///
/// Longer strings are replaced first so overlapping values cannot leave a
/// suffix behind. Empty values are ignored: replacing `""` would inject a
/// marker between every character.
#[must_use]
pub fn redact_evidence<'a>(
    message: &str,
    locator: &str,
    evidence: impl IntoIterator<Item = &'a str>,
) -> String {
    let marker = redaction_marker(locator);
    let mut values: Vec<&str> = evidence
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect();
    values.sort_unstable_by_key(|value| std::cmp::Reverse(value.len()));
    values.dedup();
    values
        .into_iter()
        .fold(message.to_string(), |redacted, value| {
            replace_outside_markers(&redacted, value, &marker)
        })
}

/// Replace evidence without damaging markers inserted by an earlier
/// producer-side redaction pass. Cross-file findings can redact source and
/// target evidence separately. If later evidence occurs inside an earlier
/// marker, replace that whole marker rather than leaking the evidence or
/// producing a malformed marker.
fn replace_outside_markers(message: &str, value: &str, marker: &str) -> String {
    const PREFIX: &str = "<redacted:";
    const MARKER_LEN: usize = PREFIX.len() + DIGEST_HEX_LEN + 1;

    let mut out = String::with_capacity(message.len());
    let mut segment_start = 0;
    let mut search_start = 0;
    while let Some(offset) = message[search_start..].find(PREFIX) {
        let start = search_start + offset;
        let end = start + MARKER_LEN;
        let valid = message.get(start..end).is_some_and(|candidate| {
            candidate.ends_with('>')
                && candidate.as_bytes()[PREFIX.len()..PREFIX.len() + DIGEST_HEX_LEN]
                    .iter()
                    .all(u8::is_ascii_hexdigit)
        });
        if valid {
            out.push_str(&message[segment_start..start].replace(value, marker));
            let prior_marker = &message[start..end];
            out.push_str(if prior_marker.contains(value) {
                marker
            } else {
                prior_marker
            });
            segment_start = end;
            search_start = end;
        } else {
            // `start` points at ASCII `<`, so advancing one byte stays on a
            // UTF-8 boundary and guarantees progress.
            search_start = start + 1;
        }
    }
    out.push_str(&message[segment_start..].replace(value, marker));
    out
}

#[cfg(test)]
mod tests {
    use super::{redact_evidence, redaction_marker};

    #[test]
    fn marker_is_stable_short_and_value_free() {
        let marker = redaction_marker("rule\0source\0.env\0$.TOKEN");
        assert_eq!(marker, "<redacted:d9da80d26c94>");
        assert_eq!(marker.len(), 23);
        assert!(!marker.contains("TOKEN_VALUE_DO_NOT_REPORT"));
        assert_eq!(marker, redaction_marker("rule\0source\0.env\0$.TOKEN"));
        assert_ne!(marker, redaction_marker("rule\0target\0.env\0$.TOKEN"));
    }

    #[test]
    fn later_short_evidence_keeps_existing_markers_well_formed() {
        let first = redaction_marker("first");
        let second = redaction_marker("second");
        let message = format!("{first} and value a");
        let redacted = redact_evidence(&message, "second", ["a"]);
        assert!(redacted.starts_with(&second), "{redacted}");
        assert_eq!(redacted.matches("<redacted:").count(), 4, "{redacted}");
    }

    #[test]
    fn marker_shaped_evidence_is_still_redacted() {
        let secret = "<redacted:123456789abc>";
        let redacted = redact_evidence(secret, "actual-locator", [secret]);
        assert_eq!(redacted, redaction_marker("actual-locator"));
        assert!(!redacted.contains("123456789abc"));
    }
}
