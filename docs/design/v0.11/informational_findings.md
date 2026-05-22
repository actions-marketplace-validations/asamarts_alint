# Informational findings — the notes/skip-surfacing channel

Status: Design draft, written 2026-05-21 to capture a v0.10 deferment
before it falls off the v0.11 cut.

## Problem

v0.10 shipped two rule kinds that **intentionally skip** entries
rather than fail on them:

- `registry_paths_resolve` skips non-literal (computed /
  interpolated) registry entries — `extract_entries` returns a
  `skipped` count that is then discarded at
  `crates/alint-rules/src/registry_paths_resolve.rs:184`
  (`let _ = skipped;`), with a comment that reads:

  > Non-literal (computed/interpolated) entries are intentionally
  > skipped, not failed. The skip is silent in v0.10 — `alint
  > check` has no informational-finding / `--explain` channel;
  > visibly surfacing the skip list is a tracked v0.11 item (see
  > the design doc).

  **This is that design doc.** It did not exist when the comment
  was written.

- `cross_file_value_equals` filters non-literal values out of both
  source and target sets (`is_non_literal` at
  `cross_file_value_equals.rs:200` and `:261`) before comparing —
  also silently.

The silence is a real gap. A user whose registry is
`members = ["crates/*", "${WORKSPACE_EXTRA}"]` gets a green run
with no indication that the `${WORKSPACE_EXTRA}` entry was never
checked. That can mask a genuine coverage hole: the rule *looks*
like it covered the whole registry but skipped part of it.

## Why this is a real alint-core feature, not a one-liner

The reason it was punted out of v0.10: alint's data model has
**no channel for a finding that isn't a violation.**

- `Violation { path, message, line, column }`
  (`crates/alint-core/src/rule.rs:26`) has no per-violation
  severity. Severity is `RuleResult.level` — one level for *all*
  of a rule's violations. So you can't emit an "info-only" item
  alongside a rule whose `level: error`.
- `Engine::run(&self, root, index) -> Result<Report>`
  (`engine.rs:176`) takes no options — there's no `--explain`/
  verbosity to thread through.
- The 8 output formatters render `violations`; there is no
  parallel "notes" array for them to render.

Adding the channel touches the core data model + every formatter,
which is why it's a deliberate design item rather than a quick
patch tacked onto a rule.

## Proposed surface

### Data model: a `notes` channel on `RuleResult`

```rust
/// A non-violation finding: something the rule chose not to fail
/// on but the user may want to know about (e.g. an entry it could
/// not statically resolve and therefore skipped).
#[derive(Debug, Clone)]
pub struct Note {
    pub message: Cow<'static, str>,
    pub path: Option<Arc<Path>>,
    pub line: Option<usize>,
}

pub struct RuleResult {
    // ... existing fields ...
    pub notes: Vec<Note>,   // NEW; default empty
}
```

Notes are **always collected** (cheap — empty Vec for the 77 rule
kinds that never emit one). Only their *rendering* is gated, so
the default-output snapshot stays byte-stable except for a single
summary line (see below).

### Default rendering: a one-line count, not the list

By default `alint check` adds at most one stderr summary line when
any notes exist:

```
note: 1 registry entry skipped (non-literal path); run with --show-notes to list
```

This keeps default stdout byte-identical for the cross-formatter
snapshot tests (`crates/.../cross_formatter`), which is a hard
invariant the v0.9.2 memory pass and every release since have
preserved. The count line lives on **stderr**, like the
`--progress` spinners — structured stdout stays clean.

### `--show-notes` lists them

```
$ alint check --show-notes
note: registry crates/Cargo.toml: skipped non-literal entry "${WORKSPACE_EXTRA}"
      (cannot statically resolve interpolated/computed paths)
note: registry packages/registry.json: skipped non-literal entry "pkgs/{{env.TIER}}/*"
```

Structured formats (`json`, `sarif`, `agent`) gain a `notes`
array per rule result; `markdown` gets a `## Notes` section;
`human` gets the indented list above.

### Naming: avoid `--explain`

PROPOSAL.md describes a planned **`alint explain <rule-id>`
subcommand** (PROPOSAL.md:610/816/907) — rule-definition
inheritance trace, an entirely different feature. Reusing
`--explain` on `check` for note-listing would collide
confusingly. Recommend **`--show-notes`** (or `--notes`). The
v0.10 source comment's "`--explain` channel" phrasing predates
that realisation and should not be taken as the committed flag
name.

## Producers in scope for v0.11

| Rule kind | Today | v0.11 |
|---|---|---|
| `registry_paths_resolve` | discards `skipped` count (`:184`) | emits one `Note` per skipped entry |
| `cross_file_value_equals` | silently filters non-literal (`:200`,`:261`) | emits a `Note` per dropped value |

Both already compute exactly the data a `Note` needs; the wiring
is "build a `Note` instead of dropping the count."

**Explicitly NOT migrated:** the `read_capped` over-cap path
("too large to analyze (N bytes; 256 MiB cap)") stays a
**violation**, not a note — an unanalysable file is a real
finding the user should act on, by design (v0.10 Phase 3C). Don't
let this feature quietly downgrade it.

## Cross-cutting with the LSP

The LSP work (`lsp_server.md`) is the richest consumer: a hover
on a skipped registry entry can render "skipped: non-literal
path" inline, and notes can surface as LSP diagnostics at
`DiagnosticSeverity::Hint`. The `Note` data model feeds **both**
the CLI `--show-notes` path and the LSP. Sequencing implication:
land this channel **before or alongside** the LSP so the LSP can
consume `notes` from day one rather than re-deriving them.

## Implementation sketch

1. `alint-core`: add `Note` + `RuleResult.notes` (default empty).
   `Report` aggregation carries notes through alongside results.
2. `Engine::run` stays signature-compatible; introduce a
   `RunOptions { show_notes: bool }` only at the *render* layer
   (the engine always collects; the formatter decides). Avoids
   threading opts through evaluation.
3. Formatters: `notes` array (json/sarif/agent), `## Notes`
   section (markdown), stderr count + `--show-notes` list (human).
4. Wire the two producer rules.
5. e2e: a registry with a `${VAR}` entry → green run + one note;
   `--show-notes` lists it; `--quiet` suppresses the count line.

Estimated: ~60 LOC core/model + ~40 LOC across formatters + ~20
LOC producer wiring + 8 unit + 6 e2e.

## Open questions

- **Default visibility — count line, or fully silent until
  `--show-notes`?** Leaning: show the one-line stderr count by
  default (silent skips are what caused this gap; a count with a
  pointer is the minimum honest signal). A `--quiet` /
  `--no-notes` suppresses it.
- **Per-violation severity vs a separate notes channel?** This
  doc proposes a separate channel. The alternative — adding a
  `Level::Hint` that's below `info` and never affects exit code —
  is more general but conflates with the existing `level: info`
  *violations*. Separate channel keeps the "did the run pass?"
  question answerable purely from `violations`.
- **Should over-cap "too large to analyze" migrate to a note?**
  No (see above) — but worth a one-line confirmation when this
  lands so a future reader doesn't "tidy" it into a note.
- **Does this need a schema/CLI-contract bump?** `--show-notes`
  is additive CLI surface (patch-tier per RELEASING.md); the
  `notes` array in json/sarif output is additive to the report
  schema. No `schema_version` bump.
