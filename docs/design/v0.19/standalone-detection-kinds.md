# Design doc: standalone detection kinds

Status: Implemented (2026-10-17).
Decisions: [ADR-0021](../../adr/0021-policy-source-files.md) (policy-source files),
[ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md) (Phase 0/1a gates).
Demand evidence: A1, E2, E3, E5, E7, and F2 in
[`../rule-coverage-gaps.md`](../rule-coverage-gaps.md).

Part of [v0.19](README.md) (work item W9).

## 1. Problem

The Phase 0 inventory identifies several high-value checks that need no relation IR or plugin
surface, but cannot be expressed faithfully by composing today's kinds:

- `.editorconfig` is already the repository's formatting source of truth, while duplicating its
  values in `.alint.yml` drifts.
- CommonMark already recognizes images, headings, and links, but alint does not assert alt text,
  a required heading sequence, unique generated anchors, or fragment resolution.
- alint can find an SPDX header with a regex but cannot distinguish a valid SPDX license expression
  from a misspelled or deprecated identifier.

The risks are false confidence from partial parsers, host-dependent policy discovery, and two
Markdown grammars disagreeing. All kinds therefore consume bounded repository files and reuse the
shipped `pulldown-cmark` event stream where applicable.

## 2. Surface area

Six canonical kinds are added:

| Kind | Required options | Purpose |
|---|---|---|
| `editorconfig_conforms` | `paths` | Apply supported properties from confined ancestor `.editorconfig` files. |
| `markdown_images_have_alt` | `paths` | Reject rendered Markdown images whose alt text is empty or whitespace-only. |
| `markdown_required_headings` | `paths`, `headings` | Require the complete ordered heading template; `*` matches zero or more headings. |
| `markdown_no_duplicate_headings` | `paths` | Reject a later heading with the same generated base anchor as an earlier heading. |
| `markdown_fragments_resolve` | `paths`; optional `relative`, `root` | Require local Markdown link fragments to name a generated target heading anchor. |
| `spdx_identifier_valid` | `paths` | Validate each `SPDX-License-Identifier:` value as a complete SPDX expression. |

All six are check-only in v0.19. Fixing policy-derived formatting requires a multi-operation edit
plan, adding alt text requires author intent, inserting/rewording headings is structural, and
changing a license expression is a legal choice. A configured `fix` is rejected at load.

`markdown_fragments_resolve` uses the same `relative` modes and rendered-site `root` mapping as
`markdown_links_resolve`. Missing target paths remain the latter kind's finding; fragment checking
runs only when a Markdown target can be resolved. Same-document links are supported.

## 3. Semantics

### 3.1 Policy discovery and conformance

`editorconfig_conforms` follows ADR-0021. For each selected file it considers only
`.editorconfig` files at repository-relative ancestor directories, never files above the lint
root. The nearest `root = true` discards outer files. Files merge outermost to innermost; matching
sections merge top to bottom; `unset` removes an inherited property.

The v0.19 property set is:

- `indent_style = tab|space`, with numeric `indent_size` enforced for spaces and canonical
  tab-first indentation plus a sub-`tab_width` space remainder for tabs;
- `end_of_line = lf|crlf|cr`, with every text check recognizing all three physical separators;
- `charset = latin1|utf-8|utf-8-bom|utf-16be|utf-16le`;
- `trim_trailing_whitespace = true` (`false` imposes no prohibition);
- `insert_final_newline = true|false` (empty files satisfy either value);
- positive integer `max_line_length`, with `off` disabling the limit.

The last effective declaration of each property is retained as provenance. Findings point to the
selected file and attach that declaration as a related location. Policy files accept only the
specified UTF-8 LF/CRLF grammar and `=` delimiter; unknown non-`root` preamble pairs are valid but
have no effect. Invalid syntax, invalid values for this supported set, unreadable policy files, and
over-limit policy files are typed analysis failures. Unknown section properties are ignored because
EditorConfig permits plugin-specific keys. Binary files are checked only for `charset`; text-layout
properties do not reinterpret binary bytes as lines.

### 3.2 CommonMark structure

A shared parser projection extracts images, headings, and links from live CommonMark events.
Fenced/indented code, inline code syntax, comments, and metadata blocks do not become structure.
A leading UTF-8 BOM is transparent and all source ranges remain offsets in the original text.
Markdown input must be valid UTF-8; otherwise the owning rule reports a `resource_parse` analysis
failure.

Image alt text is the rendered text between an image's start/end events. Whitespace-only text is
empty. HTML `<img>` elements are outside this CommonMark-image contract.

Required heading entries have the exact form `# Text` through `###### Text`, or `*`. Heading text
is compared after the parser's rendered text projection, not source marker spelling. `*` matches
zero or more complete headings; consecutive stars are rejected as redundant. The whole extracted
heading list must match the template.

Heading anchors follow GitHub's slugger: lowercase Unicode text; retain letters, marks, numbers,
`-`, and `_`; turn ASCII spaces into `-`; and remove punctuation, symbols, controls, tabs, and
other separators.
Explicit CommonMark heading attributes (`{#id}`) use that id. Generated anchor collisions receive
`-1`, `-2`, and so on while avoiding every already-issued anchor. Duplicate-heading findings use
the unsuffixed base anchor and cite the first heading as a related location.

Fragment text is percent-decoded as UTF-8 and compared case-sensitively. External URLs and
fragments on non-Markdown targets are ignored. Existing `.md` files, extensionless rendered-site
routes, directory `README.md`, and same-document links are supported. Target read/parse failures
are fail-loud and deduplicated per target file.

### 3.3 SPDX expressions

`spdx_identifier_valid` scans live text lines for `SPDX-License-Identifier:` tags, validates the
entire value using the bundled SPDX license/exception tables, and accepts SPDX expression
operators `AND`, `OR`, and `WITH` plus parentheses with SPDX precedence (`WITH`, then `AND`, then
`OR`). `LicenseRef-*` and `DocumentRef-*:LicenseRef-*` custom references are accepted according to
SPDX syntax. `WITH` requires a license on the left and a known exception on the right.

Unknown and deprecated standard identifiers are findings. Files with no tag are silent; presence
is expressible with `file_content_matches`. Binary-looking files are skipped. The bundled table is
versioned in source, used without network access, and its version is shown in the rule docs.

## 4. False-positive surface

- EditorConfig glob matching implements the specification's slash, `*`, `**`, `?`, character
  class, brace alternation, numeric range, escape, precedence, and `unset` behavior. Invalid
  policy cannot become a silent pass. Parent-directory policy is deliberately excluded.
- Markdown assertions operate on parser events, not regexes. HTML images are explicitly outside
  E2; external and non-Markdown fragments are explicitly outside E7.
- Duplicate headings are judged on generated anchors, not raw text, so punctuation/case collisions
  are found. Explicit IDs avoid a false collision between intentionally distinct anchors.
- SPDX validation is syntactic and table-driven. It does not infer a project's license or judge
  compatibility; custom `LicenseRef` values remain valid.

## 5. Implementation notes

`crates/alint-rules/src/markdown.rs` owns the shared event projection and anchor generation.
Per-file Markdown kinds use the engine's shared bounded read. Fragment resolution is full-index
because a changed target heading can invalidate an unchanged source link.

`editorconfig_conforms` owns a bounded parser that retains declaration line numbers and compiles
EditorConfig globs to Rust `regex`; it does not invoke a host EditorConfig library that can walk
above the lint root. `spdx_identifier_valid` uses the pinned, offline SPDX 3.29 data and strict
expression parser in `spdx` 0.13.6; it performs no network or runtime data loading.

Every kind declares `KindMeta`, participates in schema/facts/category/doc generation, has firing
and silent docs fixtures, and remains compatible with nesting and `expect_matches` according to
its metadata.

## 6. Tests

- Unit tests cover EditorConfig inheritance, `root`, `unset`, all glob constructs, all supported
  properties, invalid declarations, encoding boundaries, and policy provenance.
- CommonMark tests cover code/comment exclusion, BOM offsets, formatted alt text, wildcard heading
  templates, Unicode/punctuation anchors, collision suffixes, explicit IDs, encoded fragments,
  relative targets, rendered routes, and target failures.
- SPDX tests cover every table identifier, deprecated IDs, precedence, parentheses, `WITH`,
  exceptions, custom references, malformed tokens, and line/range locations.
- Each kind has one passing and one firing declarative end-to-end scenario. Fragment scenarios
  exercise cross-file related locations and changed-mode full-index behavior.
- The existing all-kinds instance, schema, output-completeness, fingerprint, docs-export, facts,
  and deterministic performance gates cover the new registrations.

## 7. Open questions

None. The policy boundary, property subset, CommonMark/anchor semantics, fragment target behavior,
SPDX expression surface, and fix posture are decided above.
