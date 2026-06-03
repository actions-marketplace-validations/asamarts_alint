//! Shared structured / line / regex extraction for the
//! manifest-driven cross-file rules (`registry_paths_resolve`,
//! `cross_file`, `file_graph`). One place so the one-of decode
//! (`serde_yaml` can't decode an externally-tagged enum from a
//! `{ key: value }` map; an untagged enum can't tell the three
//! `JSONPath` string variants apart) and the non-literal skip
//! can't drift between consumers.

use regex::Regex;
use serde::Deserialize;
use serde_json_path::JsonPath;

use crate::structured_path::Format;

/// Runtime extraction mode, resolved from [`ExtractSpec`].
#[derive(Debug, Clone)]
pub(crate) enum Extract {
    /// Structured-query (RFC 9535 `JSONPath` over the parsed tree).
    Toml(String),
    Json(String),
    Yaml(String),
    /// One path per non-blank, non-comment line.
    Lines(LinesOpts),
    /// Capture group 1 of each match is the value.
    Regex(String),
}

/// The deserialised `extract:` block — exactly one field set,
/// validated in [`ExtractSpec::resolve`].
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExtractSpec {
    #[serde(default)]
    toml: Option<String>,
    #[serde(default)]
    json: Option<String>,
    #[serde(default)]
    yaml: Option<String>,
    #[serde(default)]
    lines: Option<LinesOpts>,
    #[serde(default)]
    regex: Option<String>,
}

impl ExtractSpec {
    pub(crate) fn resolve(self) -> std::result::Result<Extract, String> {
        let set: Vec<&str> = [
            ("toml", self.toml.is_some()),
            ("json", self.json.is_some()),
            ("yaml", self.yaml.is_some()),
            ("lines", self.lines.is_some()),
            ("regex", self.regex.is_some()),
        ]
        .into_iter()
        .filter_map(|(n, on)| on.then_some(n))
        .collect();
        match set.as_slice() {
            [] => Err(
                "`extract` must set exactly one of toml/json/yaml/lines/regex (none set)"
                    .to_string(),
            ),
            [_] => Ok(if let Some(q) = self.toml {
                Extract::Toml(q)
            } else if let Some(q) = self.json {
                Extract::Json(q)
            } else if let Some(q) = self.yaml {
                Extract::Yaml(q)
            } else if let Some(o) = self.lines {
                Extract::Lines(o)
            } else {
                Extract::Regex(self.regex.expect("exactly-one ensures regex set"))
            }),
            many => Err(format!(
                "`extract` must set exactly one of toml/json/yaml/lines/regex (got {})",
                many.join(", ")
            )),
        }
    }
}

impl From<Extract> for ExtractSpec {
    fn from(e: Extract) -> Self {
        let mut s = ExtractSpec::default();
        match e {
            Extract::Toml(q) => s.toml = Some(q),
            Extract::Json(q) => s.json = Some(q),
            Extract::Yaml(q) => s.yaml = Some(q),
            Extract::Lines(o) => s.lines = Some(o),
            Extract::Regex(q) => s.regex = Some(q),
        }
        s
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LinesOpts {
    /// Lines starting with this (after trim) are skipped.
    #[serde(default = "default_comment")]
    pub(crate) comment: String,
}

fn default_comment() -> String {
    "#".to_string()
}

// `#[serde(default = "default_comment")]` only fires on the
// deserialize path; `LinesOpts::default()` (used by the
// `Lines(#[serde(default)] …)` variant and tests) needs the
// same `#` default, so derive can't be used here.
impl Default for LinesOpts {
    fn default() -> Self {
        Self {
            comment: default_comment(),
        }
    }
}

/// True when `entry` is a *computed* value (interpolation /
/// concatenation), which the caller skips rather than checks.
/// Genuine markers only: shell/Nix `${var}` and `$(cmd)`,
/// mustache/jinja `{{ … }}`, string concatenation `"a" + b`.
/// A bare `$`, backtick, or `(.` is legal in a real filename, so
/// it is **not** treated as non-literal — over-matching those
/// silently dropped real literal paths (a false negative; v0.10
/// post-audit P2). The skip never fails the rule and is
/// intentionally silent; visibly surfacing skipped entries is a
/// tracked v0.11 item (`alint check` has no `--explain` /
/// informational-finding channel).
pub(crate) fn is_non_literal(entry: &str) -> bool {
    entry.contains("${") || entry.contains("$(") || entry.contains("{{") || entry.contains("+ ")
}

/// Every string match for `extract` over `text`, raw (the caller
/// applies [`is_non_literal`] filtering as it needs). Structured
/// modes yield string-valued `JSONPath` matches; `lines` yields
/// trimmed non-comment lines; `regex` yields capture group 1.
pub(crate) fn extract_values(
    extract: &Extract,
    text: &str,
) -> std::result::Result<Vec<String>, String> {
    Ok(match extract {
        Extract::Toml(q) => structured(Format::Toml, q, text)?,
        Extract::Json(q) => structured(Format::Json, q, text)?,
        Extract::Yaml(q) => structured(Format::Yaml, q, text)?,
        Extract::Lines(opts) => text
            .lines()
            .map(str::trim)
            .filter(|l| {
                if l.is_empty() {
                    return false;
                }
                if opts.comment.is_empty() {
                    return true;
                }
                !l.starts_with(opts.comment.as_str())
            })
            .map(ToString::to_string)
            .collect(),
        Extract::Regex(pat) => {
            let re = Regex::new(pat).map_err(|e| format!("bad regex: {e}"))?;
            re.captures_iter(text)
                .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                .collect()
        }
    })
}

/// Run a structured-query (`Format::parse` + RFC 9535 `JSONPath`),
/// returning every string-valued match. Non-string nodes are
/// dropped (a value the manifest expresses as a table/array is
/// skipped, not failed).
fn structured(fmt: Format, query: &str, text: &str) -> std::result::Result<Vec<String>, String> {
    let value = fmt.parse(text)?;
    let path = JsonPath::parse(query).map_err(|e| format!("bad JSONPath {query:?}: {e}"))?;
    Ok(path
        .query(&value)
        .iter()
        .filter_map(|v| v.as_str().map(ToString::to_string))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::is_non_literal;

    #[test]
    fn genuine_interpolation_is_non_literal() {
        for e in [
            "${pkgs.foo}/bin",
            "$(date +%s)/x",
            "{{ pkg }}/lib",
            "crates/a + crates/b",
        ] {
            assert!(is_non_literal(e), "{e:?} must be non-literal");
        }
    }

    #[test]
    fn bare_dollar_backtick_dotparen_are_literal() {
        // v0.10 post-audit P2 regression: all legal in real
        // filenames — must be CHECKED, not silently skipped.
        for e in [
            "foo$bar.rs",
            "weird`name`.txt",
            "a/b (.c)/d",
            "./relative/path",
            "pkg-1.0",
            "crates/serde_json",
        ] {
            assert!(!is_non_literal(e), "{e:?} must be literal");
        }
    }
}
