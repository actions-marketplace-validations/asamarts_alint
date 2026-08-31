# Roadmap generator from canonical ROADMAP.md

Status: **Implemented** (commit `8afc01b7`). All three migration
phases shipped: `xtask/src/roadmap_generator.rs` + the
`gen-public-roadmap` subcommand + the `generate_public_roadmap`
call in `docs_export` (Phase 1, byte-identical via the round-trip
fixture); `alint:internal-*` markers in canonical `docs/design/ROADMAP.md`
(Phase 2); and the convention documented in the ROADMAP.md intro +
CONTRIBUTING.md's "Editing the roadmap" section (Phase 3). Written
2026-05-14 as the v0.9.22 cleanup cycle's E1 batch.

## Why

`docs/design/ROADMAP.md` is the canonical roadmap. alint.org's
`/docs/about/roadmap/` page is the public-facing version. Today
they are intended to be identical: `xtask docs-export` (at
`xtask/src/docs_export.rs:73-77` + `copy_one()` at line 155)
copies the canonical into the bundle, stripping only the leading
`# alint — Roadmap` H1 and injecting a Starlight frontmatter
block (`title: Roadmap`). No content elision.

In practice this has produced two recurring problems:

1. **Drift via sync lag.** The 2026-05-14 audit (Finding D) found
   the public and canonical disagreeing on the v0.11 section
   title ("LSP + developer experience" vs. "LSP + DSL polish")
   and on the headline "Latest release" version stamp. Both
   came from the public being a stale snapshot of canonical that
   hadn't been refreshed since the canonical was last edited.
   The v0.9.22 A1 sweep fixed the immediate symptoms by editing
   canonical and pushing to refresh the docs-bundle; the
   underlying recurrence surface stays.
2. **No way to elide canonical-only content.** Canonical
   contains per-patch v0.9.X breakdowns, "Reopened sub-phases"
   notes, internal-flavored scaffolding ("Held v0.9 follow-ups",
   commit-SHA citations) that are useful for engineering work
   inside the alint repo but adds noise for a public reader who
   wants the scope-per-version story. Today canonical = public,
   so trimming canonical means trimming what the public sees;
   keeping canonical rich means inflicting that detail on every
   site visitor.

This design adds an inline marker convention so canonical can
flag sections as internal-only, plus an `xtask` generator that
emits the public version by stripping those sections. Drift via
sync lag becomes structurally impossible (public is generated,
not edited); per-patch detail can live in canonical without
inflating the public page.

## Surface area

### Marker convention

Two paired HTML comments delimit internal-only sections:

```markdown
<!-- alint:internal-start -->
... content visible only in canonical ROADMAP.md ...
<!-- alint:internal-end -->
```

HTML comments because:

- They render as nothing in standard markdown viewers (GitHub,
  Starlight), so canonical readers can still see the marked
  content but the markers themselves are invisible.
- They're trivial to grep / parse / strip; no need for a full
  markdown parser in the generator.
- They survive copy-paste between editors and round-trip through
  any tool that respects raw HTML in markdown.

The markers nest at section granularity: a `<!-- alint:internal-
start -->` can appear at the start of a paragraph, a list item,
a heading, or even mid-line — the generator strips everything
up to and including the matching `<!-- alint:internal-end -->`.
Nesting (a start-marker inside another start-marker before the
first end-marker) is rejected at generator-build time with a
descriptive error.

### `xtask gen-public-roadmap`

A new subcommand on the existing xtask binary:

```sh
cargo run -p xtask -- gen-public-roadmap \
    --input docs/design/ROADMAP.md \
    --output target/docs-bundle/about/roadmap.md \
    --title Roadmap
```

Behaviour:

1. Read the input file.
2. Walk the content; for each `<!-- alint:internal-start -->`
   marker, drop everything (including the marker line) up to and
   including the matching `<!-- alint:internal-end -->`. Reject
   on unbalanced or nested markers.
3. Strip leading `# heading` (matching the current `strip_first_h1`
   behaviour for source-of-truth parity).
4. Prepend Starlight frontmatter (`---\ntitle: <title>\n---\n\n`).
5. Collapse runs of more than two blank lines into exactly two
   so a stripped section doesn't leave a visible whitespace
   chasm in the output.
6. Write to the output path.

Defaults: input defaults to `docs/design/ROADMAP.md`, output
defaults to `target/docs-bundle/about/roadmap.md`, title defaults
to `Roadmap`. Invocation in `docs-export` becomes a one-line
replacement for the existing `copy_one(ROADMAP_DOC, ...)` call.

### Integration into `docs-export`

The current `copy_one()` call site (lines 73-77 of
`docs_export.rs`) is replaced with a call to a new internal
function `generate_public_roadmap(workspace, target_dir)` that
calls the same generator logic. The standalone `xtask gen-
public-roadmap` subcommand reuses this logic for ad-hoc /
debugging invocations; it's not the canonical pipeline entry
point.

## Semantics

- **Idempotent.** Running the generator twice on the same input
  produces byte-identical output. The output of the generator is
  not itself a valid input (its frontmatter would clash); the
  generator only consumes canonical-shaped markdown.
- **Deterministic.** No timestamps in the output; no host
  fingerprinting. The same input always produces the same bytes.
- **Order-preserving.** Sections retain their canonical order;
  the generator never reorders content, only deletes marked
  blocks.
- **Byte-faithful in unmarked regions.** A character that was in
  canonical and not inside an internal-marker block appears at
  the same logical position in the output (modulo the H1 strip
  + frontmatter injection that the current pipeline already
  does).

## False-positive surface

What can go wrong, and the design's mitigations:

- **Unbalanced markers.** A `<!-- alint:internal-start -->`
  without a matching `<!-- alint:internal-end -->` (or vice
  versa). Generator fails hard with a one-line error pointing at
  the line number of the orphan marker. Surfaces during
  `ci/scripts/docs.sh` (which runs `xtask docs-export --check`).
- **Nested markers.** A second `<!-- alint:internal-start -->`
  inside an unclosed first internal block. Rejected with a
  line-numbered error; nested elision is out of scope.
- **Markers inside fenced code blocks.** A marker that's part of
  example markdown inside a ```` ```markdown ```` block could
  trip up a naïve scanner. The generator tracks fence depth and
  ignores marker syntax inside code blocks. Tests cover this.
- **Markers in HTML comment chains.** A line containing both
  `<!-- alint:internal-start -->` and `<!-- alint:internal-end -->`
  (a same-line wrapper) is interpreted as "strip nothing"
  (no content between them). This is the conservative reading;
  if authors want intra-line elision they can split the markers
  onto separate lines.
- **Section-marker drift between canonical and tests.** Adding a
  new marker pair to canonical that the tests don't know about
  is fine — tests assert generator behaviour, not the specific
  set of markers. A separate audit can list "all internal-only
  sections by line range" if that becomes a maintenance need.

## Implementation notes

- New module: `xtask/src/roadmap_generator.rs`. The parsing is
  small enough (~150 lines) that it doesn't need a markdown crate
  — a single forward pass with a small state machine handles
  marker tracking + code-fence depth. The `pulldown-cmark` /
  `comrak` ecosystem would be overkill and adds a dep just for
  one transformation.
- New xtask subcommand: thread `gen_public_roadmap` into
  `xtask/src/main.rs`'s clap enum.
- `docs-export` integration: replace the `copy_one(ROADMAP_DOC,
  ...)` call (line 73-77) with `generate_public_roadmap(&workspace,
  &target_dir)?`. Existing tests under `xtask/` that exercise
  `docs-export --check` cover the integration; specific
  generator semantics get their own unit-test module under
  `roadmap_generator.rs`.
- No new workspace dependencies. The generator uses only
  `std::fs`, `std::path`, and `anyhow::Result` (already pulled
  into xtask).
- Complexity estimate: one engineering day. ~150 lines of
  generator logic, ~150 lines of tests, ~10 lines of xtask plumbing.

## Tests

Three test categories under `xtask/src/roadmap_generator.rs#tests`:

1. **Unit tests on the parser** — marker recognition, code-fence
   handling, nested-marker rejection, unbalanced-marker rejection,
   same-line wrapper handling, blank-line collapse. Quick.

2. **Round-trip equivalence** — given an input with zero markers,
   the generator output equals the current `copy_one` output
   byte-for-byte. This is the migration guard: landing the
   generator without adding any markers must not change what
   alint.org renders. A fixture file checked into
   `xtask/tests/fixtures/roadmap_no_markers.md` exercises this.

3. **Marker-elision fixture** — a curated input with two
   `<!-- alint:internal-start -->` / `<!-- alint:internal-end -->`
   blocks (one whole-section, one paragraph-inside-a-section),
   with the expected output stored as a sibling fixture. Asserts
   the generator's elision semantics against the design here.

The `coverage_audit_*` test family on alint-e2e doesn't get a
new audit for this — generator behaviour is xtask-internal, and
the docs-bundle pipeline already audits its own output via the
manifest + the alint.org build itself failing if content is
malformed.

## Open questions

1. **Should the marker convention be reused for ARCHITECTURE.md?**
   The same drift class could affect ARCHITECTURE — canonical has
   v0.9-era engine detail that may be over-detailed for the
   public. Defer: ship the ROADMAP generator first, evaluate
   whether ARCHITECTURE wants the same treatment afterward. The
   marker convention is forward-compatible (use the same syntax
   in any file the generator is wired into).

2. **Should there be a `<!-- alint:public-only -->` complementary
   marker?** The asymmetric "internal-only" form is sufficient
   today. A `public-only` marker would let canonical have a
   compressed-for-public alternative paragraph; that's more
   power but also more conceptual surface and a step toward
   maintaining two parallel narratives in the same file. Hold
   until a concrete demand surfaces.

3. **Should the generator emit a `<!-- generated by xtask gen-
   public-roadmap; edit ROADMAP.md instead -->` watermark in
   output?** Argument for: someone editing
   `src/content/docs/docs/about/roadmap.md` locally on alint.org
   gets a strong "don't edit this" signal. Argument against: it
   adds a non-content line at the top of every public roadmap
   page. The file's gitignored on alint.org (synced subtree), so
   the "don't edit" signal is already there structurally. Hold
   the watermark unless a clear miss surfaces.

4. **Author UX: marker placement.** Should markers always be on
   their own line, or is mid-line marker support useful? The
   semantics section above accepts both; the convention can
   tighten to "own-line only" if mid-line cases turn out to
   surface bugs in practice. Initial recommendation in the
   generated docs: own-line, indented to match surrounding
   content. The generator accepts either.

5. **First markers to add.** Once the generator lands with no-op
   behaviour, what's the first content the maintainer marks as
   internal-only? Plausible candidates from the canonical ROADMAP:
   - The "Reopened sub-phases (2026-05-01)" sub-section under v0.9
     (engineering process detail; outcome is captured in the
     numbered v0.9.5.* entries below).
   - The "Held v0.9 follow-ups" section under v0.9.11 (forward-
     looking debug list; resolved or moved on by the time this
     doc ships).
   - Per-commit-SHA citations (`commit 261dda5`) inside the v0.5
     composition-and-reuse section.

   None of these are immediately load-bearing for any reader, so
   the first marker rollout doesn't need to ship with content
   gating — that's a separate PR after the generator lands and
   contributors can evaluate per-section.

## Migration plan

Three phases, each its own commit:

1. **Land the generator, no behaviour change.** Add
   `xtask/src/roadmap_generator.rs`; wire the `gen-public-roadmap`
   subcommand and the internal `generate_public_roadmap` call in
   `docs_export`. With zero markers in canonical ROADMAP.md, the
   output is byte-identical to the current `copy_one` output —
   round-trip test enforces this. Replaces the `copy_one(ROADMAP_DOC,
   ...)` line in `docs_export`.

2. **Add the first markers.** A separate commit identifies which
   sections of canonical are internal-only and wraps them in
   markers. The docs-bundle workflow regenerates the public
   roadmap; the diff is reviewable.

3. **Document the convention.** Add a paragraph to the top of
   `docs/design/ROADMAP.md` (after the existing intro) explaining
   the marker convention and pointing at this design doc. Mention
   in CONTRIBUTING.md under "Editing the roadmap" so contributors
   know to use markers rather than maintaining a separate
   public-side file.

Backout: removing the generator and restoring the original
`copy_one` call is one-line revert; canonical ROADMAP.md keeps
its markers (which render as invisible HTML comments in any
markdown viewer), so no data loss.

## Out of scope for v0.11

- **Two-way sync.** If alint.org's roadmap ever needs public-
  only content (announcement banners, blog cross-refs, marketing
  copy that doesn't belong in canonical), that's a separate
  feature. Today the public roadmap is a pure subset of canonical,
  and this generator's elision-only semantics fits that.
- **Cross-repo round-trip.** The generator runs at docs-export
  time inside the alint repo; alint.org consumes the output as
  a generated artefact. There's no path back from alint.org to
  canonical, and the v0.9.22 cleanup cycle's C3 cross-repo
  version-pin check is the only cross-repo enforcement we need
  for the foreseeable future.
- **Multiple output flavours.** Generating both a "public lean"
  version and a "public-with-detail" version from the same
  canonical, gated by markers. Single output for now; if a
  multi-flavour need surfaces, the generator's parameterisation
  hook is `--public-tier {lean|full}` plus a richer marker
  vocabulary.
