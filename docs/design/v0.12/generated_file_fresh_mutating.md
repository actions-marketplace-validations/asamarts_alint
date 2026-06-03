# `generated_file_fresh` — mutating / in-place mode

Status: **Implemented — 2026-06-03** (`crate::generated_file_fresh`, mode added,
rule count unchanged, CHANGELOG `[Unreleased]`). The `outputs:` field selects the
mutating mode; snapshot → run → diff → restore via a Drop-guard `OutputRestorer`;
shape-validated at load (exactly one of `file` / `outputs`); covered by native
unit tests (fresh-silent + tree-untouched · stale-fires-then-restores ·
new-file-flagged-and-deleted · final-newline normalize · generator-failure
rolls back) + two e2e scenarios. Resolves the
**Q1 "in-place / temp-sandbox" open question explicitly deferred** by the v0.10
[`generated_file_fresh`](../v0.10/generated_file_fresh.md) doc. The **#1 residual
of the post-v0.12 [coverage re-analysis](./post_build_coverage_analysis.md)
(≈23 repos)** — the largest single gap left after the v0.12 build. Adds a *mode*
to the existing kind (per the [architecture synthesis](./architecture_synthesis.md):
"`generated_file_fresh` mutating-mode (a flag)"), not a new kind; rule count
unchanged. CHANGELOG `[Unreleased]`.

## Motivation / demand

The shipped `generated_file_fresh` is **stdout-only**: it runs a generator that
writes its single output to *stdout* and compares that to one committed `file:`.
But the dominant real-world codegen-freshness pattern is a generator that **writes
its outputs in place**, after which CI runs `git diff --exit-code` (or
`git status --porcelain`) to assert nothing changed:

- **redis** — `make commands.def` rewrites `src/commands.def` from 442 JSON specs.
- **ruff** — `cargo dev generate-all` rewrites a tree, then git-clean.
- **pytorch** — `.github/scripts/generate_ci_workflows.py` writes ~30 files.
- **symfony** — `php …/sync-translations.php` then `git diff`.
- **postgres**, **protobuf** (`regenerate_stale_files.sh`), **svelte**, **vim**,
  **valkey**, **grafana**, **neovim**, **cpython** (`make regen-all`), … — ≈23
  corpus repos in total.

`file_graph fresh` (the generated file embeds the source's content-hash, no run)
and `command_idempotent` (the tool has a `--check` mode) each cover a *slice*. The
bare **multi-file, in-place, no-`--check` generator** — by far the most common —
has no clean native expression today. This is that mode.

## The non-goal tension (the design's hinge)

The v0.10 doc draws a hard line: **"alint's deliberate non-goal is running codegen
as a build step. A linter that silently regenerates files is a build tool, and
that is out of scope."** A mode that runs an in-place generator appears to cross
it. It does not — because it **restores the tree afterwards**:

> The mutating mode **snapshots** the declared `outputs:`, runs the generator,
> **diffs** the result against the snapshot, **reports** any file the generator
> would change, and **restores the snapshot** — leaving the working tree
> byte-identical to how it started. alint *verifies* freshness; it never *performs*
> codegen. `alint check` remains pure: it does not leave regenerated files behind.

This is the same principled stance as the stdout mode (capture-and-compare, never
write), generalised from one stdout stream to a declared set of on-disk outputs.
The restore is made **panic-safe with a Drop guard** (below), so an early return,
error, or panic still restores.

## Shape — a second source shape on the same kind

The mode is selected by **which output field is present**, validated at load
(exactly one required) — the same shape-dispatch the codebase uses elsewhere
(e.g. `cross_file` relations):

```yaml
# stdout mode (shipped, unchanged) — generator → stdout, compared to one file
- id: bindings-fresh
  kind: generated_file_fresh
  file: crates/ffi/include/core.h
  command: ["cbindgen", "--config", "cbindgen.toml", "crates/core"]

# mutating mode (new) — generator writes in place; assert the outputs are unchanged
- id: commands-def-fresh
  kind: generated_file_fresh
  outputs: "src/commands.def"           # glob OR list; selects mutating mode
  command: ["make", "commands.def"]
  workdir: "."                          # generator cwd (default: lint root)
  normalize: final-newline              # none (default) | trim | final-newline (per file)
  timeout: 300                          # seconds (default 120)
  level: error
```

- `outputs:` — a glob string **or** a list of globs (a `Scope`): the files the
  generator (re)writes. Its presence selects the mutating mode.
- **Exactly one** of `file:` / `outputs:` is required (load error otherwise).
- `command:` / `workdir:` / `normalize:` / `timeout:` are shared with the stdout
  mode. `normalize:` applies **per output file**.

## Mechanism

1. **Snapshot.** Enumerate the files matching `outputs:` (from the pre-run index)
   and read each via `crate::io::read_capped` into a `{path → bytes}` map. Register
   the map with a Drop-guard `OutputRestorer`.
2. **Run.** Spawn `command:` in `workdir` via `crate::spawn::run_capturing`
   (shared timeout / spawn-error / non-zero-exit handling, identical to the stdout
   mode — a failed generator is one violation, not a panic).
3. **Diff.** Re-enumerate `outputs:` from disk (so newly-created files are caught),
   register any *new* in-scope paths with the restorer for deletion, then compare:
   - in snapshot **and** on disk, bytes differ (after `normalize`) → **stale**;
   - in snapshot, **gone** from disk (generator deleted it) → **stale (removed)**;
   - **new** on disk, not in snapshot → **stale (uncommitted generated file)**.
4. **Report.** One violation per stale path (capped, with a "+N more" note),
   message: `<path> is out of date — re-run \`<command>\` and commit the result`.
5. **Restore.** The Drop guard writes every snapshot file back and deletes every
   registered new file, restoring the tree byte-for-byte — on the normal path *and*
   on any early return / error / panic.

## Reuse / trust

- **Trust-gate:** already covered. `generated_file_fresh` is in
  `SPAWNING_RULE_KINDS`, so an `extends:`'d ruleset declaring it is refused; only
  the user's own top-level config may run a generator. No new gate.
- **Dispatch:** `requires_full_index() == true`, `path_scope() == None` — single
  shot, same class as the stdout mode and `pair`.
- **Determinism:** like the stdout mode, the generator must be deterministic w.r.t.
  committed inputs; `normalize:` absorbs the common trailing-newline diff.

## Open questions / risks

- **Restore robustness.** The Drop guard covers panics/early-returns within the
  process. A hard kill (SIGKILL) between run and Drop can still leave the `outputs:`
  modified — documented; recovery is the real generator or `git checkout`. A
  generator that writes **outside** `outputs:` leaks (not snapshotted) — the
  `outputs:` glob is the trusted declaration of what it touches; document the
  contract.
- **Re-walk cost.** Catching new files needs a post-run enumeration of the
  `outputs:` scope. Single-shot, and the generator run dominates; acceptable. (A
  later optimisation can walk only the scope's base dirs.)
- **Testability.** Covered the same way as the stdout mode: native unit tests
  (a `sh -c` generator that rewrites a tempdir file — fresh-silent, stale-fires,
  new-file-flagged, generator-failure-rolls-back, each asserting the tree is
  restored) + a fire/silent pair of e2e scenarios under
  `scenarios/check/plugin/`. No `NATIVE_FIRES_ALLOWLIST` entry needed — the e2e
  scenarios run a real generator through the engine.
