# Design doc: `^` and `$` in content patterns

Status: Implemented in #351 (option B, chosen 2026-10-23). Proposal for
[#343](https://github.com/asamarts/alint/issues/343).
Decisions: none (the chosen path changes no matching semantics).

## 1. Problem

[#343](https://github.com/asamarts/alint/issues/343) reports that `file_content_forbidden` with
`pattern: '^test'` flags `a.txt` (`test` on line 1) but not `b.txt` (`test` on line 2). The
reporter expected the pattern to find both, the way `grep '^test'` would.

### Confirmed behavior

The rule compiles `pattern` with `regex::bytes::Regex::new` and no flags, then searches the whole
file once (`self.pattern.find(bytes)` in `crates/alint-rules/src/file_content_forbidden.rs`). In the
Rust `regex` crate, `^` and `$` match only at the start and end of the haystack unless multi-line
mode is on, so `^test` means "the file starts with `test`". Two details make this sharper than in
most regex engines:

- `$` does not match before a trailing newline (Python and PCRE allow that). `foo$` therefore
  never matches a file that ends in `\n`, which is nearly every text file.
- In multi-line mode `$` matches before `\n` only. A CRLF file also needs CRLF mode, `(?R)`.

Reproduced with the reporter's tree on the v0.17.0 release binary, on v0.20.0, and on `main` at
`dbb8b793b`; all three behave the same, so this is not a regression. Files: `a` = `test in first
line`, `b` = `# comment` then `test in 2. line`, `c` = `x` then `end test`, `d` = `c` with CRLF,
`e` = `b` with CRLF.

| Kind | Pattern | a | b | c | d | e |
|---|---|---|---|---|---|---|
| `file_content_forbidden` | `^test` | line 1 | - | - | - | - |
| `file_content_forbidden` | `(?m)^test` | line 1 | line 2 | - | - | line 2 |
| `file_content_forbidden` | `test$` | - | - | - | - | - |
| `file_content_forbidden` | `(?m)test$` | - | - | line 2 | - | - |
| `file_content_forbidden` | `(?Rm)test$` | - | - | line 2 | line 2 | - |
| `file_content_matches` | `^test` | pass | fail | fail | fail | fail |
| `file_content_matches` | `(?m)^test` | pass | pass | fail | fail | pass |

`(?m)^test` is the workaround for the report.

### The haystack differs by kind, and nothing says so

| What the regex sees | Kinds | `^` / `$` mean |
|---|---|---|
| one line at a time | `import_gate`, `for_each_match`, `ordered_block`, `git_blame_age` | start / end of the line |
| the whole file (or the whole commit message) | `file_content_forbidden`, `file_content_matches`, the `file_content_matches` fact, `extract:` regexes, `file_graph` edges, `git_commit_message` | start / end of the file or message |
| the first or last N lines, newlines included | `file_header`, `file_footer` | start / end of that block |
| one string | `filename_regex`, `git_commit_subject_matches` | start / end of the name or subject |

`docs/rules.md`, the schema descriptions ("Rust regex. File contents must NOT match.") and
`alint rules show` say nothing about anchors. The only mention is a note in
[`v0.12/sharp_edge_cluster.md`](v0.12/sharp_edge_cluster.md), where the v0.12 case study hit
the same thing in a `file_header` config and resolved it with "`(?m)^` (per-line) is the fix, no
code change". That left the documentation gap in place.

### alint's own material is affected

A scan of every config the repository ships (bundled rulesets, `examples/`, the dogfood config,
fenced YAML in the docs, the v0.12 corpus, the bench scenarios) found 478 whole-file content
patterns. Thirteen are wrong, judged by what each rule is evidently for, and each was checked
against a realistic file:

| Where | Rule | Pattern | Failure |
|---|---|---|---|
| `docs/site/about/monorepos.md` | `workspace-bazel-version-pinned` | `^\d+\.\d+\.\d+$` | fails every version file that ends in a newline |
| `examples/angular-angular` | `angular-bazel-version-shape`, `angular-nvmrc-shape` | `^\d+\.\d+\.\d+$` | as above |
| `examples/apache-airflow` | `airflow-no-urlparse` | `^\s*from urllib\.parse import .*urlparse` | only fires when the import is the first non-blank text in the file |
| `examples/flutter-flutter` | `flutter-no-trailing-whitespace` | `[ \t]+$` | never fires on a file that ends in a newline |
| `examples/pytorch-pytorch/README.md` | `pytorch-pypi-install-must-be-pinned` (its config already has `(?m)`) | `… [A-Za-z0-9._\-*+!]*$` | misses an unpinned install when any later line contains `/`, `=`, `<`, `>`, `~` or `!` |
| `xtask/src/bench/scenarios/s2_content.yml`, `xtask/src/bench_config.yml` | `py-no-print` | `^\s*print\s*\(` | only a `print(` at the top of the file |
| v0.12 corpus (design record) | ClickHouse, gradle, TypeScript, pytorch, vim | `\r$`, `^\s*\*\s*@author`, `^\* -text$`, `…*$`, `;;+$` | same two failure modes |

A first, text-only scan also counted the configuration page's `has_exact_sdk` fact, but that fact
sets `extract:`, so its `^...$` meets the extracted version string and is correct.

The bundled rulesets are correct: `agent-hygiene`'s `(?:^|[\s;{(])console\.log` covers line starts
through `\s`. Many anchored patterns are also deliberately whole-file, for example
`^\d+\.\d+\.\d+\s*$`, which asserts that a VERSION file contains exactly one version.

## 2. Options

**A. Documentation only.** State the haystack and the anchor semantics for every regex kind. Cheap
and safe, but the next user still finds out from a silent wrong result, as the reporter and our
own examples did.

**B. Documentation, corrected examples, and a load-time hint (recommended).** A, plus fixing the
shipped examples above, plus a warning when a pattern is almost certainly mis-anchored (section 3).
No matching result changes, so it fits a patch release.

**C. Make `^` and `$` line anchors in whole-file kinds.** Rejected:
- It changes the meaning of every existing whole-file anchor. An exact-content check such as
  `^\d+\.\d+\.\d+\s*$` would then pass a file with any line that is a version, so `matches`
  rules get weaker without notice.
- `forbidden` rules start reporting new findings on upgrade.
- Design rule 10 for any v2 surface ([`general-linter.md`](general-linter.md), section 7.1) says "a
  v2 surface must not reinterpret existing keys", so even `version: 2` cannot quietly redefine
  `pattern`.

**D. An explicit per-line mode.** For example, a new key whose regex runs once per line, as
`import_gate` and `for_each_match` already do. This belongs in the v2 DSL design, under a new
name. `(?m)` covers the need today.

## 3. Proposed change (option B)

### 3.1 Documentation

- A new configuration page, [`docs/site/configuration/patterns.md`](../site/configuration/patterns.md)
  ("Regex patterns and anchors"): the haystack table above, the Rust anchor rules, `\A` and `\z`
  for explicit file start and end, `(?m)` for line anchors, `(?Rm)` for CRLF files, `\s*$` for "the
  file ends here, newline or not", and the warnings. It is a site page rather than a section of
  `docs/rules.md` because the site renders each rule from its own section only: a family's text
  before its first rule reaches the family page as one short paragraph at most.
- A short paragraph in the entries for `file_content_forbidden`, `file_content_matches`,
  `file_header` and `file_footer`, linking the page, and a sentence under the configuration page's
  `facts`. The `file_content_forbidden` entry also says that it reports the first match in each file.
- The schema `description` of each of those `pattern` fields, and of the `file_content_matches`
  fact's, says the whole file (or block) is one string, so `^` and `$` anchor to it unless the
  pattern turns on `(?m)`. Editors show it on hover.

### 3.2 Corrected examples

Done for the docs and `examples/` rows in section 1, each checked against a fixture: `(?m)` for
per-line intent (airflow uses `(?m)^[ \t]*`, since `\s*` would swallow blank lines and report the
line above the import), `\s*$` or `\A...\s*\z` for whole-file shape checks, and the pytorch README
copy brought back in line with its config. Flutter's flow-control check, whose `(^|[ \t])`
alternative misses a statement at column 0, gained `(?m)` too, although the hints do not catch that
shape. The 30 example configs now load without a hint, and alint.org's case-study pages contain
none of the thirteen.

The v0.12 corpus is an archival design record and stays as it was; this section records the finding.
The S2 bench workload keeps `py-no-print` for now: fixing it changes what S2 measures, so it needs
a deliberate workload-revision bump (open question 4).

### 3.3 Load-time hints

Two narrow checks on the regex AST (`regex-syntax` 0.8, already in the dependency tree through
`regex`). The AST tells `^` (`StartLine`) apart from `\A` (`StartText`) and carries the flags.

- **H1: a `$` that only text without a final newline can reach.** In a whole-file or block kind,
  read back from a `$` in a sequence, past anything that can match the empty string: the first item
  that must consume a character cannot end in `\n`. Examples: `foo$`, `[ \t]+$`, `x.*$`,
  `^\d+\.\d+\.\d+$`. A `$` that opens its sequence (`^$`, the `$` branch of `([^\[]|$)`) never
  fires, and neither does `.*$`, which matches the empty string at the very end of any file.
- **H2: a leading `^` in `file_content_forbidden` that is open at the end.** A pattern that begins
  with `^` and does not end with `$` or `\z`. Every such pattern in the corpus meant "a line
  starting with", while `^$` or `^exact text$` are whole-file shapes. To keep a deliberate
  file-start anchor and silence the hint, write `\A`.

Both stay conservative. A pattern that turns `m` on anywhere gets no hints; an `s` anywhere lets
`.` count as matching `\n`; and whether a character class can match `\n` is decided by compiling
that class alone, so every class syntax counts exactly.

The message, once per rule or fact:

```text
alint: warning: rule `file_content`: `^` matches only at the start of the file, not at the start of each line; use `(?m)^` to anchor to lines, or `\A` if you mean the start of the file
```

Hints are warnings. They never change findings or exit codes, and `--quiet` silences them. They go
to stderr on `check`, `fix` and `baseline` (for the rules left after `--only`), into
`validate-config` output (JSON gains `warnings`), and onto the config file as LSP diagnostics.

On the 478 shipped patterns, H1 and H2 flag 11 of the 13 rows in section 1 and nothing else. The
two pytorch copies reach `$` through `[^/=<>~!]+`, which can take the final newline, so they match
erratically rather than never; they were fixed by hand. The hints also miss ambiguous shapes such
as `(^|[ \t])if\(`, which is acceptable for a warning.

### 3.4 Gates

`published_patterns_raise_no_anchor_hints` (in `coverage_audit_doc_examples.rs`) loads every
config alint publishes as guidance and fails on any hint: fenced configs in the governed docs, every
site page and example README, the example configs, every bundled ruleset and the documented
scenarios (391 today; it fails below 300 so a broken extractor cannot pass vacuously). A deliberate
counter-example can carry the existing `<!-- alint:ignore-example -->` marker. Reverting the
monorepo page's or flutter's fix makes it fail and name the pattern.

### 3.5 The report

Replied on #343 on 2026-10-23 with the cause, the `(?m)^test` workaround and a link to #351.

## 4. False-positive surface

- H1 on a deliberate end-of-file anchor in a file without a trailing newline: write `\z`, which
  the hint ignores.
- H2 on a deliberate "must not start with" rule: write `\A`.
- Both are warnings only, so a false positive costs a line of stderr, never a finding.

## 5. Implementation notes

As built:
- `alint_core::pattern_hints`: `pattern_hints(pattern, PatternTarget)`, plus `rule_tree_hints`
  (a rule and its nested `require:` rules, through the engine's rule-tree visitor) and `fact_hints`
  (whole-file `file_content_matches` facts only), returning `ConfigHint`s.
- `Rule::pattern_hints` defaults to none; `file_content_forbidden`, `file_content_matches`,
  `file_header` and `file_footer` report hints for their own haystack. `RuleEntry::pattern_hints`
  covers a loaded rule's tree.
- alint had no channel for non-fatal config warnings (did-you-mean and JSONPath hints only enrich
  load errors). The CLI prints the hints itself after loading, `validate-config` returns them with
  its result, and the LSP stores them on the session and adds them to the config file's diagnostics
  at the `id:` line of the rule or fact, as findings that are not per-file.
- `regex-syntax` is now a direct dependency of `alint-core`, at the version `regex` already uses.

## 6. Tests

- `pattern_hints` unit tables: the shipped broken shapes fire; `\s*$`, `\n?$`, `(?:\r?\n)?$`,
  `([^\[]|$)`, `^$`, `.*$`, `(?m)`, `(?m:...)`, `(?s).*$`, `[\s\S]*$`, negated classes, `\Ax` and
  `x\z` stay silent; verbose mode; facts with and without `extract`. Three mutations (a literal
  `\n`, the both-ends exemption, class evaluation) each fail them.
- Per kind: each of the four content kinds (and the aliases) reports its own target, and a nested
  `require:` rule contributes its hints.
- CLI e2e (`pattern_hints_cli.rs`): `check` warns once and keeps its findings and exit code; JSON
  stdout is untouched; `--quiet` silences; `fix` warns; `validate-config` in both formats; nested
  rules and facts; `--only`.
- LSP: one warning on the config file at the rule's `id:` line, not per-file.
- The gate in 3.4.

## 7. Open questions

1. Options A, B, C or D, and whether D goes into the v2 DSL design. *Resolved 2026-10-23: B.*
2. The release: v0.20.1 (docs, examples, hints) or v0.21. The docs and example fixes can go live
   without a release through the docs-bundle rule-page bridge. *Open.*
3. Hint channel: always on, or behind `--quiet` suppression and a JSON `warnings` field.
   *Resolved: always on, silenced by `--quiet`, with `warnings` in `validate-config`'s JSON.*
4. Whether to fix the S2 bench workload now with a revision bump. *Open.*
5. When to reply on #343. *Resolved: replied 2026-10-23.*
