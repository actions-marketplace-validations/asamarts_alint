# Design doc: `^` and `$` in content patterns

Status: Draft. Proposal for [#343](https://github.com/asamarts/alint/issues/343).
Decisions: none (the recommended path changes no matching semantics).

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
patterns. Fourteen are wrong, judged by what each rule is evidently for, and each was checked
against a realistic file:

| Where | Rule | Pattern | Failure |
|---|---|---|---|
| `docs/site/about/monorepos.md` | `workspace-bazel-version-pinned` | `^\d+\.\d+\.\d+$` | fails every version file that ends in a newline |
| `docs/site/configuration/index.md` | fact `has_exact_sdk` | `^[0-9]+\.[0-9]+\.[0-9]+$` | false for every newline-terminated file |
| `examples/angular-angular` | `angular-bazel-version-shape`, `angular-nvmrc-shape` | `^\d+\.\d+\.\d+$` | as above |
| `examples/apache-airflow` | `airflow-no-urlparse` | `^\s*from urllib\.parse import .*urlparse` | only fires when the import is the first non-blank text in the file |
| `examples/flutter-flutter` | `flutter-no-trailing-whitespace` | `[ \t]+$` | never fires on a file that ends in a newline |
| `examples/pytorch-pytorch/README.md` | `pytorch-pypi-install-must-be-pinned` | `… [A-Za-z0-9._\-*+!]*$` | misses an unpinned install when any later line contains `/`, `=`, `<`, `>`, `~` or `!` |
| `xtask/src/bench/scenarios/s2_content.yml`, `xtask/src/bench_config.yml` | `py-no-print` | `^\s*print\s*\(` | only a `print(` at the top of the file |
| v0.12 corpus (design record) | ClickHouse, gradle, TypeScript, pytorch, vim | `\r$`, `^\s*\*\s*@author`, `^\* -text$`, `…*$`, `;;+$` | same two failure modes |

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

- A "How patterns match" section in `docs/rules.md`: the haystack table above, the Rust anchor
  rules, `\A` and `\z` for explicit file start and end, `(?m)` for line anchors, `(?Rm)` for CRLF
  files, and `\s*$` (or `\n?\z`) for "the file ends here, newline or not".
- One sentence in the entries for `file_content_forbidden`, `file_content_matches`, `file_header`,
  `file_footer` and the `file_content_matches` fact that points to it. The `file_content_forbidden`
  entry also says that it reports the first match in each file.
- The schema `description` of each of those `pattern` fields gains "matched against the whole
  file; `^` and `$` anchor to its start and end unless the pattern starts with `(?m)`", so editors
  show it on hover.

### 3.2 Corrected examples

Fix the docs and `examples/` rows in section 1 (`(?m)` for per-line intent, `\s*$` for
whole-file shape checks), with a test or `alint check` run against a fixture for each. Re-check
the alint.org case-study pages that mirror those examples (a scan found none of the fourteen there). The v0.12 corpus is an archival design
record: leave it and note the finding in the corpus log.

The bench workload is a separate decision. Fixing `py-no-print` changes what S2 measures, so it
needs a deliberate S2 workload-revision bump. Otherwise leave it and record it as known.

### 3.3 Load-time hints

Two narrow checks, run when a rule is built, on the regex AST (`regex-syntax` 0.8, already in the
dependency tree through `regex`). The AST tells `^` (`StartLine`) apart from `\A` (`StartText`)
and carries the `m`, `s` and `R` flag scopes, including group-scoped flags such as `(?m:...)`.

- **H1: a `$` that cannot match a newline-terminated file.** In a whole-file or block kind, a `$`
  outside multi-line mode that directly follows an atom which cannot match `\n`, and that is not
  one branch of an alternation. Examples: `foo$`, `[ \t]+$`, `^\d+\.\d+\.\d+$`. Such a `$` matches
  only when the file does not end in a newline, which is almost never what the author meant.
- **H2: a leading `^` in `file_content_forbidden`.** A pattern that begins with `^` outside
  multi-line mode. Every such pattern in the corpus meant "a line starting with". To keep a
  deliberate file-start anchor and silence the hint, write `\A`.

Message, once per rule:

```text
warning: rule `file_content`: `^` matches only at the start of the file, not at each line.
         Use `(?m)^` to anchor to lines, or `\A` if you mean the start of the file.
```

Hints are warnings. They never change findings or exit codes. They go to stderr on `check` and
`fix`, appear in `validate-config` output, and become diagnostics on the config file in the LSP.

On the 478 shipped patterns, H1 and H2 flag exactly the 14 rows in section 1 and nothing else.
They miss a few ambiguous cases, such as `(^|[ \t])if\(`, which is acceptable for a warning.

### 3.4 Gates

The same hint function backs `coverage_audit_doc_examples` and `coverage_audit_examples_parse`: a
doc or example config that produces a hint fails, unless the block carries the existing
`<!-- alint:ignore-example -->` marker. The 14 cannot come back.

### 3.5 The report

Reply on #343 with the cause, the `(?m)^test` workaround, and a link to the fix.

## 4. False-positive surface

- H1 on a deliberate end-of-file anchor in a file without a trailing newline: write `\z`, which
  the hint ignores.
- H2 on a deliberate "must not start with" rule: write `\A`.
- Both are warnings only, so a false positive costs a line of stderr, never a finding.

## 5. Implementation notes

- A pure function in `alint-core` (say `regex_hints::anchor_hints(pattern, Haystack) -> Vec<Hint>`)
  plus a non-fatal warnings list on the loaded config. alint has no such channel today; the
  existing hints (did-you-mean, JSONPath dashed keys) only enrich load errors. The LSP and
  `validate-config` read the same list.
- `regex-syntax` becomes a direct dependency of `alint-core` at the version `regex` already pulls in.
- Size: S to M. Docs and examples are a few hours; the hint, its channel and the gates are about
  a day.

## 6. Tests

- A unit table for `anchor_hints`:
  - **Fire:** the 14 shipped patterns.
  - **Silent:** `\s*$`, `\n?\z`, `([^\[]|$)`, `(?m)^x`, `(?m:^x)`, `(?s).*$`, `[\s\S]*$`,
    `\Ax`, `x\z`, `(?:^|[\s;{(])x`.
  - **Scope:** a `$` inside a class or escaped.
- e2e: `check` prints the warning once and keeps the exit code; `validate-config` shows it;
  `--format json` output is unchanged.
- The gates in 3.4 run against the corrected docs and examples.

## 7. Open questions

1. Options A, B, C or D, and whether D goes into the v2 DSL design.
2. The release: v0.20.1 (docs, examples, hints) or v0.21. The docs and example fixes can go live
   without a release through the docs-bundle rule-page bridge.
3. Hint channel: always on, or behind `--quiet` suppression and a JSON `warnings` field.
4. Whether to fix the S2 bench workload now with a revision bump.
5. When to reply on #343.
