# ASF compliance-bundle over-fire fix (+ import_gate presets, docs)

Status: **In progress (v0.12); decisions locked 2026-05-31.** The
**highest-confidence, lowest-risk** item in the cut — landing as a
standalone ahead of the 100-repo study. The concrete spec is in
**Decisions** below; the change rides `@v1` (pure false-positive
reduction, backward-compatible).

## Motivation / demand

`compliance/apache-2@v1` and `apache/governance@v1` over-fire on
**every large Apache/CNCF repo in the 30-repo corpus — 5 independent
confirmations**: airflow (`apache/governance@v1`), helm, istio,
kubernetes, tensorflow (`compliance/apache-2@v1`). Every batch
independently re-derived the same `paths.exclude` workaround, which is
a strong signal the bundle *defaults* are wrong for real ASF repos,
not that each repo is unusual.

Universal failure mode:

- branded / abbreviated headers ("The Kubernetes Authors", helm's
  short header) instead of the canonical ASF appendix text;
- thousands of generated files carrying no header or a codegen header
  (`.pbtxt`, `.pb.go`, `.gen.go`, `_pb2.py`, `_generated.h`);
- `third_party/` and vendored trees;
- attribution to other authors ("The X Authors");
- no top-level NOTICE / KEYS (the bundle assumes both).

## Sketch

1. **Ship generated-file + vendored-tree excludes in the bundles.**
   A default `paths.exclude` covering the common generated/vendored
   globs above, so the header sweep doesn't fire on machine-authored
   files.
2. **Header tolerance.** Relax the `file_header` pattern from the
   verbatim ASF appendix to the invariant substring (the
   `apache-gov-notice` v0.10 remediation already did this for the
   NOTICE attribution line — apply the same principle to the source
   header rule), and/or make the header rule `if_present`-style where a
   branded variant is legitimate.
3. **Document the override recipe.** For the residual project-specific
   deviations, a copy-paste `paths.exclude` + relaxed-`file_header`
   override snippet in the bundle docs — the pattern every 30-repo
   batch re-derived.

## Companion items (folded in)

- **`import_gate` presets** for scala / java / dart / nix. `generic` +
  explicit `import_pattern` works today (spark, flutter, nixpkgs used
  it) but a preset is cleaner and removes a copy-paste regex.
- **Docs: `generated_file_fresh` is stdout-only.** Real codegen mutates
  files in place, so `command_idempotent --check` is the
  broadly-applicable form. The corpus was dominated by the mutating
  pattern; make the distinction explicit in the rule reference so users
  don't reach for the wrong kind.

## Decisions (locked 2026-05-31)

- **Ride `@v1`, no `@v2`.** Adding `paths.exclude` entries and
  *broadening* the header accept-pattern only ever *reduce* violations
  (a tree that passed still passes; over-firing trees fire less), so
  this is a backward-compatible false-positive reduction, not a semantic
  change. The bundle stays `@v1`.
- **`if_present`-style header is OUT.** `file_header` is
  `deny_unknown_fields` with only `pattern` + `lines`; an `if_present`
  mode would be a rule-kind change, disproportionate for this FP fix.
  The two FP-reducing levers below need no engine change.
- **Header tolerance = pattern broadening.** Add the modern SPDX form
  `SPDX-License-Identifier:\s*Apache-2\.0` to the accept alternation.
  CNCF / branded-header projects (helm, istio, kubernetes) carry an SPDX
  id rather than the ASF preamble. Applied in lockstep to
  `apache-2-source-has-license-header` and
  `apache-gov-source-license-header` (they share the pattern verbatim).
- **Generated + vendored excludes** added to both header rules (and
  `third_party` / `3rdparty` to `apache-gov-no-binaries-in-source`):
  `third_party/`, `3rdparty/`, plus codegen *naming* globs — `*.pb.go`,
  `*_grpc.pb.go`, `*.gen.go`, `*_generated.go`, `zz_generated.*.go`
  (k8s/istio), `*_pb2.py`, `*_pb2_grpc.py`, `*.pb.cc`, `*.pb.h`,
  `*.pb.swift`, `*_pb.rb`, `*.generated.*`. These complement the
  existing dir excludes (`generated/`, `build/`, `dist/`, `target/`,
  `vendor/`, `node_modules/`).
- **Glob list inlined** in the two bundles for now, consistent with how
  `rust@v1` / `python@v1` inline their excludes.
- **Companions (landed as a follow-on).** The `import_gate`
  scala/java/dart/nix presets and the `generated_file_fresh` stdout-only
  doc clarification shipped in a follow-on commit after the FP-fix core
  (kept separate so the bundle FP fix stayed atomic). The Nix preset
  covers the `import` builtin; the NixOS `imports = [ ... ]` module-list
  form still needs `language: generic` + a custom pattern.

## Residual open question

- Where the canonical generated-glob list should live (a shared module
  vs. inlined). Revisit only if a third bundle needs the same set.
