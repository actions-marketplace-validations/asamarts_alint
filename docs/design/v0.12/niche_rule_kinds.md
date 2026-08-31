# Niche rule kinds (1-2 sources each)

Status: **Planned (v0.12).** Six small, cleanly-scoped kinds batched
into one design doc; each has 1-2 corpus sources. Triage against the
100-repo study before committing — the wider corpus may promote some
to higher demand or add siblings.

## The six

### `embedded_checksum` / `self_checksum`
- **Source:** cpython Argument Clinic — each generated block ends with
  `/*[clinic end generated code: output=<hash>]*/`, a self-referential
  intra-file digest with a bespoke algorithm.
- **Gap:** `pair_hash` assumes *distinct* files + standard sha. This is
  one file whose embedded digest must match a digest of (part of) its
  own content.
- **Sketch:** `kind: embedded_checksum`, `region:` (start/end markers),
  `digest_marker:` (regex capturing the stored hash), `algorithm:`.

### full-file `lines:{}` equality with diff-on-mismatch
- **Source:** tokio (`diff README.md tokio/README.md`); rust rustdoc
  template sync.
- **Gap:** `pair_hash` reports only "digest mismatch" — no offending
  lines. Adopters want a real diff.
- **Sketch:** a `lines:` equality mode on `cross_file_value_equals` (or
  a `files_identical` kind) that emits the differing line range.

### `no_case_collisions`
- **Source:** tensorflow (Windows case-insensitive filesystem dup
  detection, `full.bats:290`). A recurring cross-platform hazard.
- **Sketch:** `kind: no_case_collisions`, `paths:` scope — flags any
  two tracked paths equal under case-folding.

### `dir_name_equals_field`
- **Source:** turbo (crate/package directory name ↔ the `name` field in
  its manifest; kebab vs `@scoped` divergences).
- **Sketch:** `kind: dir_name_equals_field`, `for:` (dir glob),
  `manifest:` (per-dir file), `field:` (path into it), with a
  `normalize:` for scope-prefix stripping.

### `cross_language_implementation_complete`
- **Sources:** arrow, tensorflow, protobuf, angular, flutter (already
  carried in the v0.11 long-tail). Per-platform / per-language surface
  parity (every binding implements the same API set).
- **Note:** the densest of the five (protobuf alone ≈45 cross-language
  assertions). Three topologies: data-format-driven, within-language
  source↔golden, platform-driven. May warrant its own doc once the
  100-repo study sharpens the shape.

### `bazel_licenses_declared` (carried from the v0.11 long-tail)
- **Source:** tensorflow's `licenses(["notice"])` BUILD-file
  discipline (every BUILD package declares a license). Single-source,
  but the source is a 100k+ file tree where the alignment cost of the
  hand-rolled check is high.
- **Gap:** a Bazel-licensing-declaration-aware kind — assert that each
  BUILD/BUILD.bazel under a scope carries a `licenses([...])` (or the
  newer `package(default_applicable_licenses=...)`) declaration.
- **Sketch:** `kind: bazel_licenses_declared`, `paths:` (BUILD-file
  glob), `allowed:` (permitted license tokens). Sibling to
  `cross_language_implementation_complete` in provenance (both were the
  v0.11 opportunistic long-tail); batched here as niche rule kinds.

## Open questions

- Which of these the 100-repo study promotes / merges. `no_case_collisions`
  and `dir_name_equals_field` are likely broadly useful; the others are
  more specialised.
- `embedded_checksum` is close to a non-goal (tool-specific algo) — only
  build it if a second, simpler source appears.
