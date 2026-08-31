# Phase 5 — real-world demonstrations of the undemonstrated v0.12 features

The v0.12 release-readiness audit flagged five shipped features with **zero
corpus demonstration**: the git-extract kinds (`pair_changed_together`,
`changeset_requires_path`), `file_graph forbidden_edges`, `file_graph fresh`,
and markerless `ordered_block`. A 3-agent workflow attempted to turn each into a
runnable, clone-verified rule. The honest outcome: **demonstrating these on real
repos is more nuanced than the audit assumed**, and the workflow's reproduce-
first verification (against the pinned clones) corrected several mis-matches.
This doc records what is demonstrable, what isn't, and why — reusable
"when does feature X apply" knowledge.

## Demonstrated

- **`pair_changed_together` — dotnet/aspnetcore** (`@423a8c2d`). The SignalR
  TypeScript clients must be accompanied by a `src/SignalR/clients/ts/CHANGELOG.md`
  edit (`eng/scripts/CodeCheck.ps1`). Added as
  `aspnetcore-signalr-ts-changelog-cochange`. This is the git-extract co-change
  class, on a pinned repo. (Diff-scoped: fires only with a real `since:` range
  in PR/CI; no-ops on a shallow clone — its firing logic is covered by the
  synthetic two-commit repos in `crates/alint-rules/tests/shell_out_rules.rs`.)

## Not cleanly demonstrable — and why (verified against the clones)

- **`changeset_requires_path`** was the audit's proposed kind for the aspnetcore
  A2 invariant, but reproduce-first refuted it: `changeset_requires_path` matches
  only **added** (git status A) paths (`collect_changed_paths_filtered(.., "A")`),
  whereas aspnetcore's CHANGELOG is a **single living file edited in place**
  (status M) — there are no per-change changelog *fragments* to add. The faithful
  kind is `pair_changed_together` (above). `changeset_requires_path` fits the
  **added-fragment** changelog style (changeset-d / towncrier `news/*.md`,
  rubocop `changelog/*.md`) — none of which is in a clean, pinned, non-empty
  state in the corpus.

- **`file_graph forbidden_edges` — needs a PATH-import ecosystem, not Python.**
  Attempted on pallets/flask's documented sansio import firewall. `file_graph`'s
  `from_content` resolver is **path-based** and, by explicit design, drops bare
  module names ("resolving module names is the package-graph non-goal — nodes
  stay path-based"). Python's namespace-dotted imports (`from ..config import X`)
  carry no `/` separator, so **zero edges form** and a `forbidden_edges` rule
  would pass vacuously — a false firewall. Verified empirically on the clone
  (three probes, zero edges). `forbidden_edges` is for ecosystems whose imports
  *are* paths: TS/JS (`from './x'`, `'../y'`), proto (`import "a/b.proto"`),
  Markdown doc cross-links. **For Python, `import_gate` is the right tool** (it
  regexes the extracted dotted-import string, not a resolved path) — flask
  already uses it, and the docs already call it "the cheap per-file version".

- **markerless `ordered_block` — pytest AUTHORS is not actually markerless.**
  The A2 evidence ("AUTHORS is wholly a sorted list with no markers") was wrong
  at the pin: AUTHORS has a 2-line maintainer preamble + a load-bearing
  `Contributors include::` header (exactly the marker the existing *marked* rule
  anchors to) before the sorted body — and the body itself has ~13 case-
  insensitive inversions, so a whole-file markerless sort false-positives twice
  over. Markerless wants a file that is a **pure, enforced, header-less sorted
  list** (a spellcheck `.dic`, a sorted allow-list); the corpus's sorted files
  all use header/marker anchors.

- **`file_graph fresh`** was not attempted: it asserts a generated file embeds
  the *source's current content hash* via a `marker` regex. Real repos do not
  embed alint-style content-hash markers in generated files (they use
  `make gen && git diff` or tool-specific stamps), so a `fresh` rule fires on
  every real repo — it is a convention alint defines, not one repos already
  follow. Covered by unit tests + the S14 bench, not corpus-demonstrable.

## Takeaway

The features are correct and tested (unit + e2e + S14 bench + `shell_out_rules.rs`);
the gap the audit named is real but is a **real-world-applicability** gap, not a
correctness one — and applicability is feature-specific: `forbidden_edges` ⇒
path-import ecosystems; `import_gate` ⇒ Python firewalls; markerless
`ordered_block` ⇒ header-less sorted lists; `changeset_requires_path` ⇒
added-fragment changelogs; `pair_changed_together` ⇒ in-place co-change (the one
clean corpus fit found). No rules were forced onto ill-fitting repos.
