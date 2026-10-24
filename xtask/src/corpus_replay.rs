//! `corpus-replay` — run `alint check` over the v0.12 case-study corpus at its
//! pinned commits, and optionally diff two binaries' reports.
//!
//! The corpus is 110 real repositories, each with the config a v0.12 analyst
//! wrote for it (`docs/design/v0.12/corpus/`), pinned by full commit SHA in
//! `pins.tsv`. Each repository is fetched once, at depth 1, into the cache and
//! reused by later runs. `--compare` runs a second binary over the same trees
//! and reports every repository whose JSON report differs: this is how v0.21's
//! shadow port of `cross_file` `equals` is checked on real configurations
//! (ADR-0023).
//!
//! Before a config runs, every rule whose kind spawns a process (for example
//! `command_idempotent`) is dropped from it, so a replay never executes a
//! third-party repository's commands. A config that still does not load is
//! reported and skipped, as is a repository that can no longer be fetched;
//! neither counts as a difference. A crash, a timeout, or a difference between
//! the two binaries fails the run.

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use alint_core::{Capability, KindCatalog};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value as Json;
use yaml_serde::Value as Yaml;

/// The committed pin list, relative to the workspace root.
const PINS: &str = "docs/design/v0.12/corpus/pins.tsv";
/// One `alint check` over the largest corpus trees takes seconds; a run this
/// long is hung, not slow.
const RUN_TIMEOUT: Duration = Duration::from_secs(900);

#[derive(Debug, Clone, clap::Args)]
pub struct Args {
    /// The alint binary to run.
    #[arg(long, default_value = "target/release/alint")]
    pub alint: PathBuf,
    /// A second alint binary. Every repository whose report differs from
    /// `--alint`'s is a difference, and any difference fails the run.
    #[arg(long)]
    pub compare: Option<PathBuf>,
    /// Cache for the repository checkouts, derived configs and run homes.
    #[arg(long, default_value = "target/corpus-replay")]
    pub cache: PathBuf,
    /// Only these repositories (`owner/name`); repeatable.
    #[arg(long = "only")]
    pub only: Vec<String>,
    /// Repositories processed in parallel.
    #[arg(long, default_value_t = 4)]
    pub jobs: usize,
    /// Never fetch; a repository missing from the cache is unavailable.
    #[arg(long)]
    pub offline: bool,
    /// Write the per-repository results as JSON.
    #[arg(long)]
    pub out_json: Option<PathBuf>,
    /// Append the Markdown report here (e.g. `$GITHUB_STEP_SUMMARY`).
    #[arg(long)]
    pub summary: Option<PathBuf>,
}

/// One row of `pins.tsv`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pin {
    repo: String,
    sha: String,
    config: String,
}

impl Pin {
    /// The cache directory name: `owner-name`, as the config files are named.
    fn slug(&self) -> String {
        self.repo.replace('/', "-")
    }
}

fn parse_pins(text: &str) -> Result<Vec<Pin>> {
    let mut pins = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = || format!("{PINS}:{}", index + 1);
        let [repo, sha, config] = line.split('\t').collect::<Vec<_>>()[..] else {
            bail!("{}: expected three tab-separated columns", at());
        };
        let name_ok = |s: &str| {
            !s.is_empty()
                && !s.starts_with('.')
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        };
        let Some((owner, name)) = repo.split_once('/') else {
            bail!("{}: repository {repo:?} is not owner/name", at());
        };
        if !name_ok(owner) || !name_ok(name) {
            bail!("{}: repository {repo:?} has an unexpected character", at());
        }
        if sha.len() != 40 || !sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            bail!("{}: {sha:?} is not a full lowercase commit SHA", at());
        }
        if config != format!("{owner}-{name}.alint.yml") {
            bail!("{}: config {config:?} does not match {repo}", at());
        }
        pins.push(Pin {
            repo: repo.to_string(),
            sha: sha.to_string(),
            config: config.to_string(),
        });
    }
    Ok(pins)
}

/// Whether `kind` (canonical or alias) is declared to spawn a process.
fn kind_spawns(kind: &str, catalog: KindCatalog<'_>) -> bool {
    catalog.meta(kind).is_some_and(|meta| {
        meta.capabilities
            .iter()
            .any(|entry| matches!(entry.capability, Capability::SpawnUserCommand))
    })
}

/// Whether a rule, or any rule nested in it (`require:`), spawns a process.
fn rule_spawns(value: &Yaml, catalog: KindCatalog<'_>) -> bool {
    match value {
        Yaml::Mapping(map) => map.iter().any(|(key, value)| {
            (key.as_str() == Some("kind")
                && value.as_str().is_some_and(|k| kind_spawns(k, catalog)))
                || rule_spawns(value, catalog)
        }),
        Yaml::Sequence(items) => items.iter().any(|item| rule_spawns(item, catalog)),
        Yaml::Tagged(tagged) => rule_spawns(&tagged.value, catalog),
        _ => false,
    }
}

/// A config with its spawning rules removed, the ids it dropped, and how many
/// remaining rules take the `cross_file` `equals` path.
struct Derived {
    yaml: String,
    dropped: Vec<String>,
    equals_rules: usize,
}

fn derive_config(source: &str, catalog: KindCatalog<'_>) -> Result<Derived> {
    let mut doc: Yaml = yaml_serde::from_str(source).context("parse the config")?;
    let mut dropped = Vec::new();
    let mut equals_rules = 0;
    if let Some(rules) = doc.get_mut("rules").and_then(Yaml::as_sequence_mut) {
        rules.retain(|rule| {
            if rule_spawns(rule, catalog) {
                let id = rule.get("id").and_then(Yaml::as_str).unwrap_or("<no id>");
                dropped.push(id.to_string());
                return false;
            }
            let kind = rule.get("kind").and_then(Yaml::as_str).unwrap_or_default();
            let relation = rule.get("relation").and_then(Yaml::as_str);
            if catalog.canonical(kind) == "cross_file"
                && (kind != "cross_file" || relation == Some("equals"))
            {
                equals_rules += 1;
            }
            true
        });
    }
    let yaml = yaml_serde::to_string(&doc).context("serialize the derived config")?;
    Ok(Derived {
        yaml,
        dropped,
        equals_rules,
    })
}

/// What one binary did with one repository.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum Outcome {
    /// The check completed (exit 0 or 1) with a parseable report.
    Checked {
        violations: u64,
        analysis_failures: u64,
        #[serde(skip)]
        report: Json,
    },
    /// The config does not load on this binary (exit 2).
    ConfigError { reason: String },
    /// Exit 3, a panic, a signal, a timeout, or an unparseable report.
    Crashed { reason: String },
}

#[derive(Debug, Clone, Serialize)]
struct RepoResult {
    repo: String,
    sha: String,
    /// `replayed`, `skipped` (config does not load), `unavailable` (fetch
    /// failed), `crashed`, or `differs`.
    verdict: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    dropped_rules: Vec<String>,
    equals_rules: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    alint: Option<Outcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compare: Option<Outcome>,
    /// Rule ids whose results differ between the two binaries.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    differing_rules: Vec<String>,
    elapsed_ms: u128,
}

pub fn run(args: &Args) -> Result<()> {
    let root = crate::workspace_root()?;
    let pins_text =
        std::fs::read_to_string(root.join(PINS)).with_context(|| format!("read {PINS}"))?;
    let mut pins = parse_pins(&pins_text)?;
    if !args.only.is_empty() {
        for wanted in &args.only {
            if !pins.iter().any(|pin| &pin.repo == wanted) {
                bail!("--only {wanted}: not in {PINS}");
            }
        }
        pins.retain(|pin| args.only.contains(&pin.repo));
    }
    let alint = absolute(&args.alint)?;
    let compare = args.compare.as_deref().map(absolute).transpose()?;
    for bin in std::iter::once(&alint).chain(compare.as_ref()) {
        if !bin.is_file() {
            bail!(
                "alint binary {} does not exist; build it first",
                bin.display()
            );
        }
    }
    let cache = absolute(&args.cache)?;
    for sub in ["repos", "configs", "home"] {
        std::fs::create_dir_all(cache.join(sub))?;
    }

    let registry = alint_rules::builtin_registry();
    let catalog = registry.catalog();
    let corpus = root.join("docs/design/v0.12/corpus");
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    let jobs = args.jobs.clamp(1, 32);
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(pin) = pins.get(index) else { break };
                    let result = replay_one(
                        pin,
                        &corpus,
                        &cache,
                        &alint,
                        compare.as_deref(),
                        catalog,
                        args.offline,
                    );
                    eprintln!("corpus-replay: {:<40} {}", pin.repo, result.verdict);
                    results.lock().unwrap().push(result);
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|result| result.repo.to_lowercase());

    let report = render_markdown(&results, compare.is_some());
    print!("{report}");
    if let Some(path) = &args.summary {
        let mut existing = std::fs::read_to_string(path).unwrap_or_default();
        existing.push_str(&report);
        std::fs::write(path, existing).with_context(|| format!("write {}", path.display()))?;
    }
    if let Some(path) = &args.out_json {
        std::fs::write(path, serde_json::to_string_pretty(&results)?)
            .with_context(|| format!("write {}", path.display()))?;
    }
    let failed: Vec<&str> = results
        .iter()
        .filter(|r| matches!(r.verdict, "crashed" | "differs"))
        .map(|r| r.repo.as_str())
        .collect();
    if !failed.is_empty() {
        bail!(
            "{} repositories crashed or differ: {}",
            failed.len(),
            failed.join(", ")
        );
    }
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(std::env::current_dir()?.join(path))
}

fn replay_one(
    pin: &Pin,
    corpus: &Path,
    cache: &Path,
    alint: &Path,
    compare: Option<&Path>,
    catalog: KindCatalog<'_>,
    offline: bool,
) -> RepoResult {
    let started = Instant::now();
    let mut result = RepoResult {
        repo: pin.repo.clone(),
        sha: pin.sha.clone(),
        verdict: "replayed",
        reason: None,
        dropped_rules: Vec::new(),
        equals_rules: 0,
        alint: None,
        compare: None,
        differing_rules: Vec::new(),
        elapsed_ms: 0,
    };
    let finish = |mut result: RepoResult, verdict, reason: Option<String>| {
        result.verdict = verdict;
        result.reason = reason;
        result.elapsed_ms = started.elapsed().as_millis();
        result
    };

    let derived = match std::fs::read_to_string(corpus.join(&pin.config))
        .map_err(anyhow::Error::from)
        .and_then(|source| derive_config(&source, catalog))
    {
        Ok(derived) => derived,
        Err(error) => {
            return finish(
                result,
                "crashed",
                Some(format!("{}: {error:#}", pin.config)),
            );
        }
    };
    result.dropped_rules.clone_from(&derived.dropped);
    result.equals_rules = derived.equals_rules;
    let config = cache.join("configs").join(&pin.config);
    if let Err(error) = std::fs::write(&config, &derived.yaml) {
        return finish(
            result,
            "crashed",
            Some(format!("write {}: {error}", config.display())),
        );
    }

    let checkout = cache.join("repos").join(pin.slug());
    if let Err(reason) = ensure_checkout(&checkout, pin, offline) {
        return finish(result, "unavailable", Some(reason));
    }
    let home = cache.join("home").join(pin.slug());
    let _ = std::fs::create_dir_all(&home);

    let first = check(alint, &checkout, &config, &home);
    let second = compare.map(|bin| check(bin, &checkout, &config, &home));
    if let Err(reason) = ensure_pristine(&checkout) {
        result.alint = Some(first);
        result.compare = second;
        return finish(result, "crashed", Some(reason));
    }

    let (verdict, reason, differing) = classify(&first, second.as_ref());
    result.differing_rules = differing;
    result.alint = Some(first);
    result.compare = second;
    finish(result, verdict, reason)
}

/// The verdict for one repository from one or two outcomes.
fn classify(
    first: &Outcome,
    second: Option<&Outcome>,
) -> (&'static str, Option<String>, Vec<String>) {
    let crash = |outcome: &Outcome| match outcome {
        Outcome::Crashed { reason } => Some(reason.clone()),
        _ => None,
    };
    if let Some(reason) = crash(first).or_else(|| second.and_then(crash)) {
        return ("crashed", Some(reason), Vec::new());
    }
    let Some(second) = second else {
        return match first {
            Outcome::ConfigError { reason } => ("skipped", Some(reason.clone()), Vec::new()),
            _ => ("replayed", None, Vec::new()),
        };
    };
    match (first, second) {
        (Outcome::ConfigError { reason: a }, Outcome::ConfigError { reason: b }) if a == b => {
            ("skipped", Some(a.clone()), Vec::new())
        }
        (Outcome::Checked { report: a, .. }, Outcome::Checked { report: b, .. }) => {
            if a == b {
                return ("replayed", None, Vec::new());
            }
            let differing = differing_rules(a, b);
            let reason = if differing.is_empty() {
                "the reports differ outside the rule results".to_string()
            } else {
                format!("{} rule result(s) differ", differing.len())
            };
            ("differs", Some(reason), differing)
        }
        _ => (
            "differs",
            Some("the binaries disagree on whether the config loads".to_string()),
            Vec::new(),
        ),
    }
}

/// Rule ids whose `results` entries differ, or appear in only one report.
fn differing_rules(a: &Json, b: &Json) -> Vec<String> {
    let index = |report: &Json| -> std::collections::BTreeMap<String, Json> {
        report["results"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|rule| {
                (
                    rule["id"].as_str().unwrap_or_default().to_string(),
                    rule.clone(),
                )
            })
            .collect()
    };
    let (a, b) = (index(a), index(b));
    let mut ids: Vec<String> = a.keys().chain(b.keys()).cloned().collect();
    ids.sort();
    ids.dedup();
    ids.retain(|id| a.get(id) != b.get(id));
    ids
}

fn git(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .stdin(Stdio::null());
    command
}

fn git_ok(command: &mut Command, what: &str) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|error| format!("{what}: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "{what}: {}",
            stderr.trim().lines().last().unwrap_or("failed")
        ))
    }
}

/// Make `dir` a checkout of exactly `pin.sha`, fetching it at depth 1 if needed.
fn ensure_checkout(dir: &Path, pin: &Pin, offline: bool) -> Result<(), String> {
    if !dir.join(".git").is_dir() {
        if offline {
            return Err("not in the cache (--offline)".to_string());
        }
        std::fs::create_dir_all(dir)
            .map_err(|error| format!("create {}: {error}", dir.display()))?;
        git_ok(git(dir).args(["init", "-q"]), "git init")?;
        let url = format!("https://github.com/{}.git", pin.repo);
        git_ok(
            git(dir).args(["remote", "add", "origin", &url]),
            "git remote add",
        )?;
    }
    let head = git_ok(
        git(dir).args(["rev-parse", "--verify", "-q", "HEAD"]),
        "git rev-parse",
    );
    if head.as_deref() != Ok(pin.sha.as_str()) {
        if offline {
            return Err("cached checkout is not at the pin (--offline)".to_string());
        }
        git_ok(
            git(dir).args([
                "fetch",
                "-q",
                "--depth",
                "1",
                "--no-tags",
                "origin",
                &pin.sha,
            ]),
            "git fetch",
        )?;
        git_ok(
            git(dir).args([
                "-c",
                "advice.detachedHead=false",
                "checkout",
                "-q",
                "--force",
                "--detach",
                &pin.sha,
            ]),
            "git checkout",
        )?;
    }
    let head = git_ok(git(dir).args(["rev-parse", "HEAD"]), "git rev-parse")?;
    if head != pin.sha {
        return Err(format!("checkout is at {head}, not {}", pin.sha));
    }
    ensure_pristine(dir)
}

/// The checkout matches its commit exactly: `alint check` must not write.
fn ensure_pristine(dir: &Path) -> Result<(), String> {
    let status = git_ok(
        git(dir).args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignored",
        ]),
        "git status",
    )?;
    if status.is_empty() {
        Ok(())
    } else {
        let first = status.lines().next().unwrap_or_default();
        Err(format!("the checkout is not pristine: {first}"))
    }
}

/// Run one `alint check` in a scrubbed environment and classify the result.
fn check(bin: &Path, checkout: &Path, config: &Path, home: &Path) -> Outcome {
    let mut command = Command::new(bin);
    command
        .args(["check", "--format", "json", "--config"])
        .arg(config)
        .current_dir(checkout)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("LANG", "C.UTF-8")
        .env("NO_COLOR", "1");
    let output = match run_with_timeout(&mut command, RUN_TIMEOUT) {
        Ok(output) => output,
        Err(reason) => return Outcome::Crashed { reason },
    };
    let stderr_line = || {
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .next()
            .unwrap_or_default()
            .to_string()
    };
    match output.code {
        Some(0 | 1) => match serde_json::from_slice::<Json>(&output.stdout) {
            Ok(report) => Outcome::Checked {
                violations: report["summary"]["total_violations"].as_u64().unwrap_or(0),
                analysis_failures: report["summary"]["total_analysis_failures"]
                    .as_u64()
                    .unwrap_or(0),
                report,
            },
            Err(error) => Outcome::Crashed {
                reason: format!("unparseable JSON report: {error}"),
            },
        },
        Some(2) => Outcome::ConfigError {
            reason: stderr_line(),
        },
        Some(code) => Outcome::Crashed {
            reason: format!("exit {code}: {}", stderr_line()),
        },
        None => Outcome::Crashed {
            reason: "killed by a signal".to_string(),
        },
    }
}

struct Output {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Run `command` to completion, killing it after `timeout`. Both pipes are
/// drained on their own threads so a large report cannot block the child.
fn run_with_timeout(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("spawn: {error}"))?;
    let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s", timeout.as_secs()));
            }
            Err(error) => return Err(format!("wait: {error}")),
        }
    };
    Ok(Output {
        code: status.code(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn render_markdown(results: &[RepoResult], compared: bool) -> String {
    let count = |verdict: &str| results.iter().filter(|r| r.verdict == verdict).count();
    let replayed = count("replayed");
    let with_equals = results
        .iter()
        .filter(|r| r.verdict == "replayed" && r.equals_rules > 0)
        .count();
    let mut out = String::new();
    let _ = writeln!(out, "### Corpus replay\n");
    let _ = writeln!(
        out,
        "{} {}: {replayed} replayed ({with_equals} with `cross_file` `equals` rules), \
         {} skipped, {} unavailable, {} crashed{}.\n",
        results.len(),
        if results.len() == 1 {
            "repository"
        } else {
            "repositories"
        },
        count("skipped"),
        count("unavailable"),
        count("crashed"),
        if compared {
            format!(", {} differ", count("differs"))
        } else {
            String::new()
        },
    );
    let notable: Vec<&RepoResult> = results.iter().filter(|r| r.verdict != "replayed").collect();
    if !notable.is_empty() {
        let _ = writeln!(out, "| Repository | Verdict | Reason |\n|---|---|---|");
        for result in notable {
            let mut reason = result
                .reason
                .clone()
                .unwrap_or_default()
                .replace('|', "\\|");
            if !result.differing_rules.is_empty() {
                let _ = write!(reason, " ({})", result.differing_rules.join(", "));
            }
            let _ = writeln!(out, "| `{}` | {} | {reason} |", result.repo, result.verdict);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_test<T>(test: impl FnOnce(KindCatalog<'_>) -> T) -> T {
        let registry = alint_rules::builtin_registry();
        test(registry.catalog())
    }

    #[test]
    fn committed_pins_parse_and_name_existing_configs() {
        let root = crate::workspace_root().unwrap();
        let pins = parse_pins(&std::fs::read_to_string(root.join(PINS)).unwrap()).unwrap();
        assert_eq!(pins.len(), 110);
        for pin in &pins {
            assert!(
                root.join("docs/design/v0.12/corpus")
                    .join(&pin.config)
                    .is_file(),
                "{} names a missing config",
                pin.repo
            );
        }
        let mut repos: Vec<&str> = pins.iter().map(|pin| pin.repo.as_str()).collect();
        repos.sort_unstable();
        repos.dedup();
        assert_eq!(repos.len(), pins.len(), "a repository is pinned twice");
    }

    #[test]
    fn pins_reject_malformed_rows() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert!(parse_pins(&format!("a/b\t{sha}\ta-b.alint.yml\r\n# note\n")).is_ok());
        for bad in [
            format!("a/b\t{sha}"),
            format!("a/../b\t{sha}\ta-..-b.alint.yml"),
            format!("a/b\t{}\ta-b.alint.yml", &sha[..10]),
            format!("a/b\t{}\ta-b.alint.yml", sha.to_uppercase()),
            format!("a/b\t{sha}\tother.alint.yml"),
            format!("a b/c\t{sha}\ta b-c.alint.yml"),
        ] {
            assert!(parse_pins(&bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn derive_drops_spawning_rules_including_nested_ones() {
        let source = r#"
version: 1
rules:
  - id: keep
    kind: file_exists
    paths: README.md
  - id: fmt
    kind: command_idempotent
    command: ["cargo", "fmt"]
  - id: nested
    kind: for_each_dir
    select: "crates/*"
    require:
      - kind: command
        command: ["true"]
  - id: equals-alias
    kind: cross_file_value_equals
  - id: equals-explicit
    kind: cross_file
    relation: equals
  - id: subset
    kind: cross_file
    relation: subset
"#;
        let derived = catalog_test(|catalog| derive_config(source, catalog)).unwrap();
        assert_eq!(derived.dropped, ["fmt", "nested"]);
        assert_eq!(derived.equals_rules, 2);
        let doc: Yaml = yaml_serde::from_str(&derived.yaml).unwrap();
        let ids: Vec<&str> = doc["rules"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|rule| rule["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["keep", "equals-alias", "equals-explicit", "subset"]);
    }

    #[test]
    fn every_spawning_kind_in_the_catalog_is_dropped() {
        catalog_test(|catalog| {
            let spawning: Vec<&str> = catalog
                .kinds()
                .filter(|(kind, _)| kind_spawns(kind, catalog))
                .map(|(kind, _)| kind)
                .collect();
            assert!(spawning.contains(&"command"), "{spawning:?}");
            for kind in alint_dsl::SPAWNING_RULE_KINDS {
                assert!(
                    kind_spawns(kind, catalog),
                    "{kind} is not treated as spawning"
                );
            }
        });
    }

    fn checked(results: &Json) -> Outcome {
        Outcome::Checked {
            violations: 0,
            analysis_failures: 0,
            report: serde_json::json!({ "schema_version": 1, "results": results }),
        }
    }

    #[test]
    fn classify_reports_differences_and_skips() {
        let a = checked(
            &serde_json::json!([{ "id": "r1", "passed": true }, { "id": "r2", "passed": true }]),
        );
        let b = checked(
            &serde_json::json!([{ "id": "r1", "passed": true }, { "id": "r2", "passed": false }]),
        );
        assert_eq!(classify(&a, None).0, "replayed");
        assert_eq!(classify(&a, Some(&a.clone())).0, "replayed");
        let (verdict, _, rules) = classify(&a, Some(&b));
        assert_eq!((verdict, rules), ("differs", vec!["r2".to_string()]));

        let config = Outcome::ConfigError {
            reason: "bad".into(),
        };
        assert_eq!(classify(&config, None).0, "skipped");
        assert_eq!(classify(&config, Some(&config.clone())).0, "skipped");
        assert_eq!(classify(&a, Some(&config)).0, "differs");
        let crash = Outcome::Crashed {
            reason: "exit 101".into(),
        };
        assert_eq!(classify(&a, Some(&crash)).0, "crashed");
        assert_eq!(classify(&crash, None).0, "crashed");
    }
}
