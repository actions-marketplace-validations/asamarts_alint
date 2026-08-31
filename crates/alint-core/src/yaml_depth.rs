//! Pre-parse bound on YAML flow-collection nesting depth.
//!
//! `serde_yaml_ng`/libyaml is super-linear on deeply-nested FLOW collections
//! (`[[[…]]]` / `{{{…}}}`): a ~40 KB `[`×20000 document already takes seconds,
//! and a slightly deeper one hangs the whole run — an algorithmic-complexity `DoS`
//! reachable both from a `yaml_path_*` rule over crafted repo content and from a
//! deeply-nested config / `extends:`'d ruleset (which the config loader parses
//! with the same library). libyaml has no nesting limit and the slowness is in
//! its tokenizer, so only a cheap pre-parse scan of the raw text can bound it.
//!
//! This tracks FLOW nesting specifically (block/indentation nesting is bounded
//! by the file's byte size and isn't the pathological case), distinguishing a
//! genuine flow open from a `[`/`{` that merely sits inside a plain scalar
//! (`key: a[b` is a valid scalar, not a nested sequence) so it never
//! false-rejects ordinary YAML.

/// Real config/manifest YAML nests a handful of flow levels; anything past this
/// is a bomb. Chosen far above any legitimate document yet far below the depth
/// where libyaml starts to slow (~tens of thousands), so the margin is huge in
/// both directions.
pub const MAX_YAML_FLOW_DEPTH: usize = 1024;

/// `true` when the YAML text's flow-collection nesting stays within
/// [`MAX_YAML_FLOW_DEPTH`]. A cheap single-pass scan; skips quoted scalars and
/// end-of-line comments so their contents don't count.
#[must_use]
pub fn flow_depth_within_limit(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut depth = 0usize;
    // The last significant non-space byte seen in BLOCK context, used to decide
    // whether a `[`/`{` opens a flow node (it follows `:`/`-`/`,`/`?`/`[`/`{` or
    // starts a line) or is just a char inside a plain scalar (follows a scalar
    // byte). `0` = line start.
    let mut prev_significant: u8 = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\n' => {
                prev_significant = 0;
                i += 1;
            }
            b' ' | b'\t' | b'\r' => {
                i += 1;
            }
            b'"' => {
                // Double-quoted scalar: `\` escapes the next byte.
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                prev_significant = b'"';
            }
            b'\'' => {
                // Single-quoted scalar: `''` is an escaped quote.
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\'' {
                        if bytes.get(i + 1) == Some(&b'\'') {
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                prev_significant = b'\'';
            }
            b'#' if depth == 0 && (prev_significant == 0 || prev_significant == b' ') => {
                // A `#` is a comment only at line start or after whitespace
                // (which this branch's guard approximates via `prev_significant`,
                // reset to a space by the whitespace arm... conservatively we
                // only treat line-start `#` as a comment to avoid ever skipping a
                // bomb). Skip to end of line.
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'[' | b'{' => {
                // In flow context every open nests. In block context, only an
                // open at a node position (line start, or after `:`/`-`/`,`/`?`)
                // starts a flow collection; one after a scalar byte is part of a
                // plain scalar and must NOT count.
                let opens_flow = depth > 0
                    || matches!(
                        prev_significant,
                        0 | b':' | b'-' | b',' | b'?' | b'[' | b'{'
                    );
                if opens_flow {
                    depth += 1;
                    if depth > MAX_YAML_FLOW_DEPTH {
                        return false;
                    }
                }
                prev_significant = c;
                i += 1;
            }
            b']' | b'}' => {
                depth = depth.saturating_sub(1);
                prev_significant = c;
                i += 1;
            }
            _ => {
                prev_significant = c;
                i += 1;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shallow_and_realistic_yaml_passes() {
        assert!(flow_depth_within_limit(
            "a: [1, 2, [3, {b: 4}]]\nc:\n  - x\n  - y\n"
        ));
        // Plain scalars containing brackets must NOT be counted as flow.
        assert!(flow_depth_within_limit(&"k: value[0][1]\n".repeat(5000)));
        // Brackets inside quoted scalars don't count.
        assert!(flow_depth_within_limit(&"k: \"[[[[[[[[[[\"\n".repeat(5000)));
    }

    #[test]
    fn deep_flow_nesting_is_rejected() {
        let bomb = format!("x: {}{}", "[".repeat(5000), "]".repeat(5000));
        assert!(!flow_depth_within_limit(&bomb));
        // With content between the opens (`[1,[1,[1,…`) it still nests.
        let bomb2 = format!("x: {}1{}", "[1,".repeat(2000), "]".repeat(2000));
        assert!(!flow_depth_within_limit(&bomb2));
        // Curly flow maps too.
        let bomb3 = format!("x: {}1{}", "{a: ".repeat(2000), "}".repeat(2000));
        assert!(!flow_depth_within_limit(&bomb3));
    }
}
