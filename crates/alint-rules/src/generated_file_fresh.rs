//! `generated_file_fresh` — a committed file must match a
//! declared generator's stdout. A **non-mutating freshness
//! check**: alint does NOT run codegen as a build step; this
//! only *verifies* that the committed artefact equals what the
//! user-declared, maintainer-trusted generator produces, by
//! capturing its stdout — it never writes the working tree.
//! Same trust tier as the `command` rule (which already shells
//! out). Single-shot (one spawn, one declared file), not
//! per-file. Design + open-question resolutions:
//! `docs/design/v0.10/generated_file_fresh.md`.
//!
//! ```yaml
//! - id: bindings-fresh
//!   kind: generated_file_fresh
//!   file: crates/ffi/include/core.h
//!   command: ["cbindgen", "--config", "cbindgen.toml", "crates/core"]
//!   workdir: "."                 # generator cwd (default: lint root)
//!   normalize: final-newline     # none (default) | trim | final-newline
//!   level: error
//! ```

use std::path::Path;
use std::process::{Command as StdCommand, Stdio};

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Violation};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Normalize {
    /// Exact byte equality.
    #[default]
    None,
    /// Trim leading/trailing whitespace of the whole output.
    Trim,
    /// Normalise only a single trailing newline (the most common
    /// generator/editor diff).
    FinalNewline,
}

impl Normalize {
    fn apply(self, s: &str) -> String {
        match self {
            Self::None => s.to_string(),
            Self::Trim => s.trim().to_string(),
            Self::FinalNewline => s.strip_suffix('\n').unwrap_or(s).to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    file: String,
    command: Vec<String>,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    normalize: Normalize,
}

#[derive(Debug)]
pub struct GeneratedFileFreshRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    file: String,
    command: Vec<String>,
    workdir: String,
    normalize: Normalize,
}

impl Rule for GeneratedFileFreshRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // Single-shot: staleness is independent of which files
        // changed, so it always evaluates (never `--changed`-
        // filtered). `path_scope` stays `None` (default) so the
        // engine doesn't skip-by-intersection. Same dispatch
        // class as `pair`.
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let file = Path::new(&self.file);
        let (program, rest) = self
            .command
            .split_first()
            .expect("build() rejects an empty command");

        let output = match StdCommand::new(program)
            .args(rest)
            .current_dir(ctx.root.join(&self.workdir))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("ALINT_ROOT", ctx.root.to_string_lossy().as_ref())
            .env("ALINT_RULE_ID", &self.id)
            .env("ALINT_LEVEL", self.level.as_str())
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                return Ok(vec![self.violation(
                    file,
                    &format!("generator `{program}` could not be spawned: {e}"),
                )]);
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let snippet: String = stderr.trim().chars().take(400).collect();
            let code = output
                .status
                .code()
                .map_or_else(|| "a signal".to_string(), |c| c.to_string());
            return Ok(vec![self.violation(
                file,
                &format!("generator exited with {code}: {snippet}"),
            )]);
        }

        let Ok(committed) = std::fs::read(ctx.root.join(file)) else {
            return Ok(vec![self.violation(
                file,
                "is not on disk, but the generator produced output for it",
            )]);
        };

        let stale = if self.normalize == Normalize::None {
            committed != output.stdout
        } else {
            let produced = self
                .normalize
                .apply(&String::from_utf8_lossy(&output.stdout));
            let on_disk = self.normalize.apply(&String::from_utf8_lossy(&committed));
            produced != on_disk
        };
        if stale {
            return Ok(vec![self.violation(
                file,
                &format!(
                    "is stale — its committed contents differ from `{}` output{}",
                    self.command.join(" "),
                    first_diff_hint(&output.stdout, &committed),
                ),
            )]);
        }
        Ok(Vec::new())
    }
}

impl GeneratedFileFreshRule {
    fn violation(&self, file: &Path, desc: &str) -> Violation {
        let msg = self
            .message
            .clone()
            .unwrap_or_else(|| format!("{}: {desc}", file.display()));
        Violation::new(msg).with_path(file.to_path_buf())
    }
}

/// A short hint at where the generator output and the committed
/// file first diverge (line-based; lossy is fine for a hint).
fn first_diff_hint(produced: &[u8], committed: &[u8]) -> String {
    let p = String::from_utf8_lossy(produced);
    let c = String::from_utf8_lossy(committed);
    for (i, (lp, lc)) in p.lines().zip(c.lines()).enumerate() {
        if lp != lc {
            return format!(" (first differs at line {})", i + 1);
        }
    }
    let (np, nc) = (p.lines().count(), c.lines().count());
    if np == nc {
        String::new()
    } else {
        format!(" (generator produced {np} lines, file has {nc})")
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    if opts.file.trim().is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "generated_file_fresh `file` must not be empty",
        ));
    }
    if opts.command.is_empty() {
        return Err(Error::rule_config(
            &spec.id,
            "generated_file_fresh requires a non-empty `command` argv \
             (the generator that produces `file` on stdout)",
        ));
    }
    Ok(Box::new(GeneratedFileFreshRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        file: opts.file,
        command: opts.command,
        workdir: opts.workdir.unwrap_or_else(|| ".".to_string()),
        normalize: opts.normalize,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(file: &str, command: &[&str], normalize: Normalize) -> GeneratedFileFreshRule {
        GeneratedFileFreshRule {
            id: "t".into(),
            level: Level::Error,
            policy_url: None,
            message: None,
            file: file.into(),
            command: command.iter().map(ToString::to_string).collect(),
            workdir: ".".into(),
            normalize,
        }
    }

    fn eval(r: &GeneratedFileFreshRule, root: &Path) -> Vec<Violation> {
        let idx = alint_core::FileIndex::from_entries(Vec::new());
        let ctx = Context {
            root,
            index: &idx,
            registry: None,
            facts: None,
            vars: None,
            git_tracked: None,
            git_blame: None,
        };
        r.evaluate(&ctx).unwrap()
    }

    #[test]
    fn fresh_file_is_silent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("out.txt"), "alpha\nbravo\n").unwrap();
        let r = rule(
            "out.txt",
            &["sh", "-c", "printf 'alpha\\nbravo\\n'"],
            Normalize::None,
        );
        assert!(eval(&r, dir.path()).is_empty());
    }

    #[test]
    fn stale_file_fails_with_line_hint() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("out.txt"), "alpha\nWRONG\n").unwrap();
        let r = rule(
            "out.txt",
            &["sh", "-c", "printf 'alpha\\nbravo\\n'"],
            Normalize::None,
        );
        let v = eval(&r, dir.path());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("stale"));
        assert!(v[0].message.contains("line 2"), "{:?}", v[0].message);
    }

    #[test]
    fn final_newline_normalize_absorbs_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        // File has no trailing newline; generator emits one.
        std::fs::write(dir.path().join("out.txt"), "alpha\nbravo").unwrap();
        let g = ["sh", "-c", "printf 'alpha\\nbravo\\n'"];
        assert_eq!(
            eval(&rule("out.txt", &g, Normalize::None), dir.path()).len(),
            1,
            "exact-byte compare sees the newline diff"
        );
        assert!(
            eval(&rule("out.txt", &g, Normalize::FinalNewline), dir.path()).is_empty(),
            "final-newline normalize absorbs it"
        );
    }

    #[test]
    fn missing_committed_file_is_a_violation() {
        let dir = tempfile::tempdir().unwrap();
        let r = rule("nope.txt", &["sh", "-c", "printf x"], Normalize::None);
        let v = eval(&r, dir.path());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("not on disk"));
    }

    #[test]
    fn generator_nonzero_exit_is_a_violation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("out.txt"), "x").unwrap();
        let r = rule(
            "out.txt",
            &["sh", "-c", "echo boom >&2; exit 3"],
            Normalize::None,
        );
        let v = eval(&r, dir.path());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("exited with 3"));
        assert!(v[0].message.contains("boom"));
    }

    #[test]
    fn missing_generator_program_is_a_violation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("out.txt"), "x").unwrap();
        let r = rule("out.txt", &["alint-no-such-generator-xyz"], Normalize::None);
        let v = eval(&r, dir.path());
        assert_eq!(v.len(), 1);
        assert!(v[0].message.contains("could not be spawned"));
    }
}
