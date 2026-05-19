//! `pair_hash` — a target file must carry the digest of a source
//! file.
//!
//! The `algorithm` digest of every file matching `source` must
//! appear in the single `target` — either as an embedded hex
//! substring (`contains`) or a coreutils / go-`.sum`-style
//! `<hex>  <path>` manifest line (`sums-line`). Cross-file rule
//! (the `pair` dispatch class). alint never rewrites the manifest
//! (detection-only, like `file_hash`). Design + open-question
//! resolutions: `docs/design/v0.10/pair_hash.md`.
//!
//! ```yaml
//! - id: fips-sum-pins-module
//!   kind: pair_hash
//!   source: "src/crypto/internal/fips140/v1.0.0/**/*.go"
//!   target: "src/crypto/internal/fips140/fips140.sum"
//!   algorithm: sha256          # sha256 (default) | sha512
//!   format: sums-line          # contains (default) | sums-line
//!   level: error
//! ```

use std::path::Path;

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;
use sha2::{Digest, Sha256, Sha512};

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Algorithm {
    #[default]
    Sha256,
    Sha512,
}

impl Algorithm {
    /// Lowercase hex digest of `bytes`.
    fn hex(self, bytes: &[u8]) -> String {
        match self {
            Self::Sha256 => encode_hex(Sha256::digest(bytes).as_slice()),
            Self::Sha512 => encode_hex(Sha512::digest(bytes).as_slice()),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Format {
    /// The digest must appear as a substring anywhere in `target`.
    #[default]
    Contains,
    /// `target` must carry a `sha256sum`-style `<hex> [*]<path>`
    /// line whose path token is the source's path.
    SumsLine,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    source: String,
    /// The single file that must carry the digest (a `.sum` /
    /// `SHA256SUMS` / a file with an embedded hash).
    target: String,
    #[serde(default)]
    algorithm: Algorithm,
    #[serde(default)]
    format: Format,
}

#[derive(Debug)]
pub struct PairHashRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    source_scope: Scope,
    target: String,
    algorithm: Algorithm,
    format: Format,
}

impl Rule for PairHashRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // Cross-file: the verdict for a source depends on the
        // contents of a separate target file, not the diff. Same
        // dispatch class as `pair` — opts out of `--changed`
        // filtering; `path_scope` stays `None`.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let target_path = Path::new(&self.target);
        let b_bytes = match crate::io::read_capped(&ctx.root.join(target_path)) {
            Ok(b) => b,
            Err(crate::io::ReadCapError::TooLarge(n)) => {
                return Ok(vec![
                    Violation::new(format!(
                        "pair_hash target {:?} is too large to analyze \
                         ({n} bytes; 256 MiB cap)",
                        self.target
                    ))
                    .with_path(std::sync::Arc::<Path>::from(target_path)),
                ]);
            }
            Err(crate::io::ReadCapError::Io(_)) => {
                let msg = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "pair_hash target {:?} does not exist or is unreadable",
                        self.target
                    )
                });
                return Ok(vec![
                    Violation::new(msg).with_path(std::sync::Arc::<Path>::from(target_path)),
                ]);
            }
        };
        let b_text = String::from_utf8_lossy(&b_bytes);
        let b_lower = b_text.to_ascii_lowercase();

        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.source_scope.matches(&entry.path, ctx.index) {
                continue;
            }
            let a_bytes = match crate::io::read_capped(&ctx.root.join(&entry.path)) {
                Ok(b) => b,
                Err(crate::io::ReadCapError::TooLarge(n)) => {
                    violations.push(
                        Violation::new(format!(
                            "{} is too large to hash ({n} bytes; 256 MiB cap)",
                            entry.path.display()
                        ))
                        .with_path(entry.path.clone()),
                    );
                    continue;
                }
                // permission / race — silent skip, like content rules
                Err(crate::io::ReadCapError::Io(_)) => continue,
            };
            let digest = self.algorithm.hex(&a_bytes);
            if let Some(desc) = self.check(&entry.path, &digest, &b_text, &b_lower) {
                let msg = self.message.clone().unwrap_or(desc);
                violations.push(Violation::new(msg).with_path(entry.path.clone()));
            }
        }
        Ok(violations)
    }
}

impl PairHashRule {
    /// `None` ⇒ the source's digest is properly present in the
    /// target; `Some(desc)` ⇒ a violation description.
    fn check(&self, src: &Path, digest: &str, b: &str, b_lower: &str) -> Option<String> {
        match self.format {
            Format::Contains => {
                if b_lower.contains(digest) {
                    return None;
                }
                Some(format!(
                    "{} of {} ({digest}) not found in {}",
                    self.algorithm.label(),
                    src.display(),
                    self.target,
                ))
            }
            Format::SumsLine => {
                let want = src.to_string_lossy();
                for line in b.lines() {
                    let mut tok = line.split_whitespace();
                    let (Some(hex), Some(path_tok)) = (tok.next(), tok.next()) else {
                        continue;
                    };
                    // Normalise the coreutils binary-mode `*`
                    // marker and a `find`-style `./` prefix
                    // (`<hex>  ./path`, what `find … -exec
                    // sha256sum` and Go tooling emit) so the
                    // token compares against the source's
                    // repo-root-relative index path. Backslash
                    // separators are not normalised — the `.sum`
                    // formats in scope are forward-slash.
                    let path_tok = path_tok.strip_prefix('*').unwrap_or(path_tok);
                    let path_tok = path_tok.strip_prefix("./").unwrap_or(path_tok);
                    if path_tok != want {
                        continue;
                    }
                    return if hex.eq_ignore_ascii_case(digest) {
                        None
                    } else {
                        Some(format!(
                            "{} digest mismatch for {} in {}: manifest has {hex}, \
                             file hashes to {digest}",
                            self.algorithm.label(),
                            src.display(),
                            self.target,
                        ))
                    };
                }
                Some(format!(
                    "{} is not listed in manifest {}",
                    src.display(),
                    self.target,
                ))
            }
        }
    }
}

/// Lowercase hex. Local (mirrors `file_hash`'s private encoder)
/// to avoid a crate-wide pub helper for one rule.
fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(s, "{b:02x}").unwrap();
    }
    s
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    alint_core::reject_scope_filter_on_cross_file(spec, "pair_hash")?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.source.trim().is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "pair_hash `source` must not be empty",
        ));
    }
    if opts.target.trim().is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "pair_hash `target` (the file that must carry the digest) must not be empty",
        ));
    }
    if spec.fix.is_some() {
        return Err(Error::rule_config(
            &spec.id,
            "pair_hash has no fix op — regenerating a checksum manifest is the \
             manifest generator's job, not alint's",
        ));
    }
    let source_scope = Scope::from_patterns(std::slice::from_ref(&opts.source))?;
    Ok(Box::new(PairHashRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        source_scope,
        target: opts.target,
        algorithm: opts.algorithm,
        format: opts.format,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, tempdir_with_files};

    // sha256("hello") — well-known vector.
    const HELLO_SHA256: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    fn rule(source: &str, target: &str, algorithm: Algorithm, format: Format) -> PairHashRule {
        PairHashRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            source_scope: Scope::from_patterns(&[source.to_string()]).unwrap(),
            target: target.into(),
            algorithm,
            format,
        }
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(Algorithm::Sha256.hex(b"hello"), HELLO_SHA256);
    }

    #[test]
    fn contains_passes_when_digest_embedded() {
        let manifest = format!("// pinned\nHASH = {HELLO_SHA256}\n");
        let (tmp, idx) =
            tempdir_with_files(&[("a.txt", b"hello"), ("pin.txt", manifest.as_bytes())]);
        let r = rule("a.txt", "pin.txt", Algorithm::Sha256, Format::Contains);
        assert!(r.evaluate(&ctx(tmp.path(), &idx)).unwrap().is_empty());
    }

    #[test]
    fn contains_fires_when_digest_absent() {
        let (tmp, idx) =
            tempdir_with_files(&[("a.txt", b"hello"), ("pin.txt", b"nothing relevant here\n")]);
        let r = rule("a.txt", "pin.txt", Algorithm::Sha256, Format::Contains);
        let v = r.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].path.as_deref(), Some(Path::new("a.txt")));
        assert!(v[0].message.contains("not found in"));
    }

    #[test]
    fn contains_is_case_insensitive() {
        let manifest = format!("HASH={}\n", HELLO_SHA256.to_ascii_uppercase());
        let (tmp, idx) =
            tempdir_with_files(&[("a.txt", b"hello"), ("pin.txt", manifest.as_bytes())]);
        let r = rule("a.txt", "pin.txt", Algorithm::Sha256, Format::Contains);
        assert!(r.evaluate(&ctx(tmp.path(), &idx)).unwrap().is_empty());
    }

    #[test]
    fn sums_line_passes_on_matching_line() {
        let manifest = format!("{HELLO_SHA256}  a.txt\n");
        let (tmp, idx) =
            tempdir_with_files(&[("a.txt", b"hello"), ("SHA256SUMS", manifest.as_bytes())]);
        let r = rule("a.txt", "SHA256SUMS", Algorithm::Sha256, Format::SumsLine);
        assert!(r.evaluate(&ctx(tmp.path(), &idx)).unwrap().is_empty());
    }

    #[test]
    fn sums_line_tolerates_binary_marker() {
        let manifest = format!("{HELLO_SHA256} *a.txt\n");
        let (tmp, idx) =
            tempdir_with_files(&[("a.txt", b"hello"), ("SHA256SUMS", manifest.as_bytes())]);
        let r = rule("a.txt", "SHA256SUMS", Algorithm::Sha256, Format::SumsLine);
        assert!(r.evaluate(&ctx(tmp.path(), &idx)).unwrap().is_empty());
    }

    #[test]
    fn sums_line_tolerates_dot_slash_prefix() {
        // `find … -exec sha256sum` / Go tooling emit
        // `<hex>  ./path`; the `./` must not cause a false
        // "not listed in manifest" on a correctly-pinned file.
        let manifest = format!("{HELLO_SHA256}  ./a.txt\n");
        let (tmp, idx) =
            tempdir_with_files(&[("a.txt", b"hello"), ("SHA256SUMS", manifest.as_bytes())]);
        let r = rule("a.txt", "SHA256SUMS", Algorithm::Sha256, Format::SumsLine);
        assert!(
            r.evaluate(&ctx(tmp.path(), &idx)).unwrap().is_empty(),
            "a ./-prefixed sums-line path must match the index path"
        );
    }

    #[test]
    fn sha512_sums_line_round_trips() {
        let digest = Algorithm::Sha512.hex(b"hello");
        let manifest = format!("{digest}  a.txt\n");
        let (tmp, idx) =
            tempdir_with_files(&[("a.txt", b"hello"), ("SHA512SUMS", manifest.as_bytes())]);
        let r = rule("a.txt", "SHA512SUMS", Algorithm::Sha512, Format::SumsLine);
        assert!(r.evaluate(&ctx(tmp.path(), &idx)).unwrap().is_empty());
    }

    #[test]
    fn sums_line_fires_on_wrong_hash() {
        let bad = "0".repeat(64);
        let manifest = format!("{bad}  a.txt\n");
        let (tmp, idx) =
            tempdir_with_files(&[("a.txt", b"hello"), ("SHA256SUMS", manifest.as_bytes())]);
        let r = rule("a.txt", "SHA256SUMS", Algorithm::Sha256, Format::SumsLine);
        let v = r.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("digest mismatch"));
    }

    #[test]
    fn sums_line_fires_when_path_not_listed() {
        let (tmp, idx) = tempdir_with_files(&[
            ("a.txt", b"hello"),
            ("SHA256SUMS", b"deadbeef  other.txt\n"),
        ]);
        let r = rule("a.txt", "SHA256SUMS", Algorithm::Sha256, Format::SumsLine);
        let v = r.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("not listed in manifest"));
    }

    #[test]
    fn missing_in_is_one_violation_on_in() {
        let (tmp, idx) = tempdir_with_files(&[("a.txt", b"hello")]);
        let r = rule("a.txt", "nope.sum", Algorithm::Sha256, Format::Contains);
        let v = r.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].path.as_deref(), Some(Path::new("nope.sum")));
        assert!(v[0].message.contains("does not exist"));
    }

    #[test]
    fn sha512_contains_round_trips() {
        let digest = Algorithm::Sha512.hex(b"hello");
        let manifest = format!("sha512 = {digest}\n");
        let (tmp, idx) =
            tempdir_with_files(&[("a.txt", b"hello"), ("pin.txt", manifest.as_bytes())]);
        let r = rule("a.txt", "pin.txt", Algorithm::Sha512, Format::Contains);
        assert!(r.evaluate(&ctx(tmp.path(), &idx)).unwrap().is_empty());
    }

    #[test]
    fn glob_source_one_violation_per_offender() {
        // ok.txt is listed correctly; bad.txt is not in the
        // manifest at all → exactly one violation (on bad.txt).
        let ok_hash = Algorithm::Sha256.hex(b"ok");
        let manifest = format!("{ok_hash}  ok.txt\n");
        let (tmp, idx) = tempdir_with_files(&[
            ("ok.txt", b"ok"),
            ("bad.txt", b"bad"),
            ("SHA256SUMS", manifest.as_bytes()),
        ]);
        let r = rule("*.txt", "SHA256SUMS", Algorithm::Sha256, Format::SumsLine);
        let v = r.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].path.as_deref(), Some(Path::new("bad.txt")));
    }

    #[test]
    fn build_rejects_empty_source_and_fix_block() {
        let spec = crate::test_support::spec_yaml(
            "id: t\nkind: pair_hash\nsource: \"\"\ntarget: s.sum\nlevel: error\n",
        );
        assert!(
            build(&spec)
                .unwrap_err()
                .to_string()
                .contains("`source` must not be empty")
        );
        let spec = crate::test_support::spec_yaml(
            "id: t\nkind: pair_hash\nsource: a.txt\ntarget: s.sum\nlevel: error\n\
             fix:\n  file_remove: {}\n",
        );
        assert!(build(&spec).unwrap_err().to_string().contains("no fix op"));
    }
}
