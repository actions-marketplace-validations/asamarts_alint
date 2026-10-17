# Design doc: related locations and ranges on findings

Status: Accepted design (2026-10-16); implemented in the v0.19 development tree.
Decisions: ADR-0019 DEC-4 and DEC-5.
Demand evidence: [`../general-linter.md`](../general-linter.md) sections 6.2 and 8.1; W6 in [`README.md`](README.md).

## 1. Problem

The shipped finding model identifies one path and, at most, one line/column point. A relationship failure therefore names its other files only inside prose. SARIF cannot correlate the locations, an editor cannot navigate to them, and one point cannot describe a structured value or capture. Adding presentation fields also risks accidentally changing ADR-0006 baseline identity.

W6 adds location structure before the later relation IR depends on it. Existing kinds retain their current primary anchor and fingerprint. The first producers are the existing multi-resource kinds: `cross_file`, `registry_paths_resolve`, `unique_by`, the pair kinds, and `file_graph`.

## 2. Surface area

There is no DSL change.

`alint-core` adds:

- `SourceRange`: an optional original-byte range plus a 1-based, Unicode-scalar line/column range. Starts are inclusive; ends are exclusive.
- `RelatedLocation`: a repo-relative path, optional range, and a short label.
- `Violation::range`, `Violation::related_locations`, and `Violation::related_locations_omitted`, plus builders that maintain the bounded canonical set.

The existing `path`, `line`, and `column` fields remain the compatibility projection of the primary location. Setting a range also sets the point fields to its start. Existing producers that set only a point continue unchanged.

v0.19 projects related locations into SARIF `relatedLocations` and LSP `Diagnostic.relatedInformation`. The LSP field is emitted only when the client advertises `publishDiagnostics.relatedInformationSupport`. Human output prints indented, sanitized related-location lines. The v1 JSON payload does not gain location fields in v0.19; its schema is relaxed now so consumers have one release to accept additive fields, as required by DEC-4.

## 3. Semantics

The primary location remains exactly the existing anchor for every migrated kind. Future generic findings choose the declared report anchor, then the non-canonical side, then the first canonical location.

Related locations are maintained in canonical order by:

1. normalized forward-slash path bytes;
2. start byte offset when known, otherwise line/column, otherwise the end of that path;
3. end position;
4. label.

Exact duplicates are discarded. A finding retains at most 20 related locations. Builders accept a
complete producer batch, sort and deduplicate it, record the exact number of remaining distinct
locations, and retain the canonical lowest 20. Producer iteration order therefore cannot change
the output; the stored finding remains bounded even though exact counting requires a transient
batch.

Labels are fixed producer text, not extracted evidence. Paths remain visible under the W5 policy: repository locations are navigational structure, not extracted values. Labels and paths pass the existing output sanitizers or format encoders.

A related location never participates in `baseline::fingerprint`. Primary range endpoints also remain outside the fingerprint, matching the existing rule that line and column numbers are not identity.

## 4. False-positive surface

Related locations are explanatory and cannot create or suppress a violation, change severity, enable a fix, or alter an exit code. A missing related file may still be represented when that absence is the relationship failure; consumers receive its intended repo-relative URI.

The main accuracy risk is assigning the canonical side as primary and moving an existing annotation. Migrated rules therefore keep their current primary path and add only secondary paths. Another risk is unstable truncation from hash-map iteration; bounded canonical insertion removes that dependency.

LSP columns require UTF-16 conversion. The server resolves both endpoints against the open overlay when available and disk text otherwise; absent text falls back conservatively to the recorded scalar columns.

## 5. Implementation notes

The model lives in `alint-core/src/location.rs` and `alint-core/src/rule.rs`. SARIF projection lives in `alint-output/src/sarif.rs`; LSP conversion lives in `alint-lsp/src/diagnostics.rs`. No new dependency is required.

The common path allocates nothing: ordinary findings keep an empty related-location vector and no
range. A relation producer materializes its complete location batch once; the builder sorts and
deduplicates that batch, records the exact omitted count, and truncates retained storage to 20.
`SourceRange::from_byte_range` performs one linear pass over only the prefix through the range end
and is used only when a producer requests a span.

The checked-in scenario fingerprint snapshot is a separate W6 gate. It records every initial-check policy fingerprint in the complete Linux scenario corpus and is recomputed in CI. Intentional identity migrations require an explicit `UPDATE_SNAPSHOTS=1` review.

## 6. Tests

- Unit tests pin range conversion over multibyte and multiline text, canonical related-location ordering, deduplication, the 20-location cap, and order-independent truncation.
- A baseline unit test proves changing ranges and related locations does not change a fingerprint.
- SARIF tests prove primary ranges, labeled `relatedLocations`, and the truncation marker.
- LSP tests initialize clients both with and without related-information support and prove the field is present only for the former, with UTF-16-correct ranges.
- Producer tests cover all five required families and preserve their old primary anchors.
- `crates/alint-e2e/tests/fingerprint_golden.rs` recomputes the complete corpus snapshot.
- The JSON-schema contract test proves unknown additive fields are accepted while existing required fields and value types remain enforced.

## 7. Open questions

All W6 questions are resolved.

- **Cap:** 20, matching the program document's concrete bound and small enough for editor/SARIF consumers.
- **End coordinates:** exclusive, matching LSP and SARIF's end-column convention.
- **JSON timing:** schema relaxation only in v0.19; related-location payload fields wait until the next release, per DEC-4.
- **Baseline:** no schema bump; related locations and ranges are non-identity metadata, per DEC-5.
