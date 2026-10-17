# Design doc: explicit analysis time

Status: Accepted (2026-10-16).
Decisions: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md),
[ADR-0020](../../adr/0020-explicit-analysis-time.md)
Demand evidence: [`general-linter.md` decision DEC-11](../general-linter.md#131-decisions-taken-2026-10-14)

## 1. Problem

`git_blame_age` reads `SystemTime::now()` while evaluating each run. The same commit therefore
changes findings as wall time passes, violating the repository's byte-identical-output guarantee.
It also makes a historical or reproduced run impossible: callers cannot state the reference date
against which line age is measured.

A time-dependent check needs one explicit, deterministic analysis input. The default must remain
convenient in Git repositories without letting the workstation clock affect output.

## 2. Surface area

The CLI gains one global option:

```text
--as-of <YYYY-MM-DD>
```

`ALINT_AS_OF` supplies the same value when the flag is absent; the flag has normal clap precedence
over the environment. Parsing is strict Gregorian `YYYY-MM-DD`, restricted to dates representable
as nonnegative Unix time. An explicit date denotes `00:00:00 UTC` at the start of that date.

The flag is meaningful for `check`, `fix`, `baseline`, `explain`, and `lsp`. Supplying it to a
command that neither evaluates nor explains configured rules is rejected rather than silently
ignored.

`alint-core` gains an immutable `AnalysisTime` value. `Engine::with_analysis_time` injects an
explicit value; otherwise the engine resolves the current repository's HEAD committer timestamp
once for that engine's repository root, and only when a rule declares the analysis-time capability.
`Context` borrows that value. The capability inventory renames `wall_clock` to `analysis_time`:
time-dependent kinds are not permitted to read the process clock.

## 3. Semantics

Precedence is:

1. `--as-of YYYY-MM-DD`;
2. `ALINT_AS_OF=YYYY-MM-DD`;
3. `git log -1 --format=%ct HEAD` in the analyzed repository.

The HEAD fallback uses the commit's committer timestamp, including its time of day, as a Unix
instant. The displayed reference is its UTC calendar date. The fallback ignores dirty working-tree
mtime, the host wall clock, inherited repository-redirecting Git environment variables, and
clone-local replacement refs. When neither explicit source is provided, an unavailable Git
repository or executable, or an unborn HEAD, leaves a time-dependent rule without a reference time
and produces an evaluation
error. W2 classifies that error as an always-error analysis failure; W7 supplies the deterministic
failure and message without changing unrelated error classification ahead of W2. An explicit date
removes only the HEAD-time lookup: `git_blame_age` still independently requires a usable Git
repository for tracked-file selection and blame, as enforced by W3.

`git_blame_age` computes each line's age from the single context value, never by calling
`SystemTime::now()`. A future-dated blame timestamp has age zero. The threshold remains strict:
a line fires only when `age > max_age_days * 86,400 seconds`.

Blame attribution also excludes ambient clone configuration: the command pins the Myers diff
algorithm, clears configured ignore revisions, and then applies `.git-blame-ignore-revs` only when
that path is tracked and is a regular working-tree file. It never follows that convention through
a symlink. The porcelain parser accepts full SHA-1 and SHA-256 object IDs. These controls leave the
repository's tracked ignore list available while preventing global or clone-local settings,
replacement refs, and external symlink targets from changing line ages.

Every `git_blame_age` finding ends with `(as of YYYY-MM-DD)`, including findings whose main message
comes from a user template. `alint explain <id>` shows the same date and whether its source is
`explicit` or `head`; JSON exposes the two fields structurally. If explain cannot resolve HEAD, it
shows an unavailable source rather than pretending to have a date.

The same analysis time is used by check, baseline generation, fix evaluation and verification, LSP
full checks, and LSP buffer re-evaluation for the lifetime of the engine. No pass in a fixpoint run
re-resolves time.

## 4. False-positive surface

Calendar boundaries and future timestamps can make age rules appear one day younger than a user
expects. The mitigation is a precise UTC contract and visible reference date. An explicit date is
midnight UTC; users needing a later cutoff choose the following date. HEAD fallback uses the exact
commit instant so repeated analysis of the same commit is stable.

A dirty tree analyzed against HEAD intentionally uses HEAD's time, not file mtimes or today's date.
That trades real-time debt aging for reproducibility. CI can set `--as-of` when policy requires a
calendar release date instead.

Malformed input fails before repository walking. Git lookup is argument-constant (`HEAD`) and
does not accept configuration-controlled revisions, avoiding option injection. Missing Git is
visible to the rule rather than a silent pass. Blame keeps its pre-W2 advisory behavior for an
individual unblameable file.

## 5. Implementation notes

`crates/alint-core/src/analysis_time.rs` owns strict parsing, UTC civil-date conversion, source
metadata, and display. `crates/alint-core/src/git.rs` owns the HEAD committer-time query and
deterministic blame setup. `crates/alint-core/src/engine.rs` resolves at most once per engine
repository root, guarded by a runtime `wants_analysis_time()` audit that must agree with `KindMeta`.
`git_blame_age` is the first user. No date/time dependency is added.

Constitution invariant 1 is amended by ADR-0020: deterministic inputs include explicit CLI and
environment values plus immutable repository state; ambient wall time is forbidden in analysis.
The direct source audit that previously allowed one wall-clock user changes to reject analysis-time
reads outside the central model.

## 6. Tests

- Strict date-parser tests cover leap years, invalid widths/ranges, the Unix epoch, and the upper
  supported date.
- Git helper tests cover a fixed commit timestamp, non-repository, unborn HEAD, malformed command
  output through the parser seam, SHA-1 and SHA-256 blame records, inherited repository redirects,
  replacement refs, configured ignore-revs lists, the tracked regular
  `.git-blame-ignore-revs` convention, symlink exclusion, and a clone-local diff algorithm.
- Engine tests prove lazy resolution: no time-capable rule means no Git time lookup contract is
  required; explicit time works outside Git.
- `git_blame_age` tests create commits with fixed author/committer dates and prove old, boundary,
  young, and future lines; findings contain the reference date.
- CLI tests cover flag-over-environment precedence, invalid flag/environment values, explicit
  time without a repository still failing `git_blame_age`'s independent Git dependency, the
  unborn-HEAD fallback failure, explain human/JSON fields, and unsupported commands.
- A determinism test runs the same committed fixture with different process wall-clock settings
  and compares bytes. Because the implementation never reads the wall clock, both outputs must be
  identical.
- Capability and source-audit gates prove `git_blame_age` declares `analysis_time` and no rule reads
  `SystemTime::now()`.

The ordinary deterministic performance gate covers the added capability check and single Git
command. There is no per-file or per-finding process spawn.

## 7. Open questions

All decisions are resolved.

- **2026-10-16:** dates mean midnight UTC; no locale or timezone input is accepted.
- **2026-10-16:** fallback uses HEAD committer time (`%ct`), not author time or wall time.
- **2026-10-16:** explicit time is stored on the engine and reused by all passes and LSP checks.
  A fallback resolution is likewise cached for the engine's repository root; reusing that engine
  with a different root is an error rather than silently substituting a second timestamp.
- **2026-10-16:** `explain` reports an unavailable fallback without failing; evaluation reports the
  error for W2 to classify.
- **2026-10-16:** `wall_clock` is replaced by `analysis_time` in capability metadata rather than
  retained as a compatibility alias.
