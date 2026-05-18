//! Shared structured / line / regex extraction for the
//! manifest-driven cross-file rules (`registry_paths_resolve`,
//! `cross_file_value_equals`). One place so the one-of decode
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

/// An extracted entry the caller should skip rather than fail on:
/// non-literal (interpolation / variables / antiquotation). The
/// rules surface it (never silently drop) so `--explain` shows
/// *why* a value was not checked, and it never fails the rule.
pub(crate) fn is_non_literal(entry: &str) -> bool {
    entry.contains("${")
        || entry.contains("{{")
        || entry.contains('$')
        || entry.contains('`')
        // Nix antiquotation / computed path expressions.
        || entry.contains("+ ")
        || entry.contains("(.")
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
