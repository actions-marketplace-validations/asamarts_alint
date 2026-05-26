# ASF compliance-bundle over-fire fix (+ import_gate presets, docs)

Status: **Planned (v0.12).** The **highest-confidence, lowest-risk**
item in the cut — can land early as a standalone, ahead of the study.

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

## Open questions

- Bundle-version bump: does relaxing `compliance/apache-2@v1` warrant a
  `@v2`, or is it a backward-compatible tightening-of-excludes that can
  ride `@v1`? (Lean: excludes + header tolerance only *reduce* false
  positives, so `@v1` in place; but a stricter audit may disagree.)
- Where the canonical generated-file glob list lives (shared with the
  `read_capped` / hygiene excludes?).
