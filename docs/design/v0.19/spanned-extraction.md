# Design doc: duplicate-aware spanned extraction

Status: Accepted design (2026-10-16); implemented in the v0.19 development tree.
Decisions: ADR-0019 question Q5.
Demand evidence: B1 `no_duplicate_keys` and B3 `structured_key_sort` in [`../rule-coverage-gaps.md`](../rule-coverage-gaps.md); W8 in [`README.md`](README.md).

## 1. Problem

The shared structured parser normalizes documents into `serde_json::Value`. That representation is appropriate for existing JSONPath rules, but an object map is last-wins and cannot represent two entries with the same key. It also loses source order and source spans. Consequently a future duplicate-key rule would miss the condition it is meant to detect, and a key-order rule could neither judge source order nor point at the offending key.

W8 adds a parallel extraction substrate. It does not change the value tree or any existing rule's results.

## 2. Surface area

There is no DSL or registered-kind change.

`alint-core::extract` exposes:

- `EntryPathSegment::{Key, Index}` for a containing object's structural path;
- `ByteSpan`, an original-source half-open byte range;
- `StructuredEntryRow { parent, key, key_span, value_span }`;
- `extract_entry_rows(format, text)`, returning source-ordered rows or a typed unsupported result.

Each object member is one row. Duplicate names are separate adjacent or non-adjacent rows; they are never inserted into a map. The `parent` identifies the containing object. Array positions are explicit `Index` segments. Repeated keys do not need an occurrence suffix because row order and key span are their source identities.

The first span-capable adapters are exactly W8's required set:

- dotenv through `adotenv`'s lossless document entries;
- JSON/JSONC through `jsonc-parser`'s AST property vector;
- YAML through `saphyr-parser` events, preserving mapping events before a map can collapse duplicates;
- INI and Java properties through shared raw-source scanners derived from the structured-fix locators.

TOML, XML, and HCL return `UnsupportedFormat` in v0.19. This is explicit rather than presenting a flattened, duplicate-losing fallback as trustworthy extraction. Their eventual adapters must emit the same row model.

## 3. Semantics

Rows follow physical source order. `key_span` covers the source token that denotes the key, including quotes or escapes where the parser reports the complete token. `value_span` covers the corresponding value node/token when it is unambiguous; it may be absent for a bare dotenv declaration, a YAML alias/implicit value, an INI continuation, or another construct whose current locator cannot identify one safe value span.

All byte offsets address the original UTF-8 input, including a leading BOM. Parsers that strip BOMs add the removed byte count back before returning spans.

Nested object paths are semantic decoded names. For JSON and YAML, arrays contribute `Index`. INI section members use parent `[Key(section)]`; pre-section INI keys, dotenv keys, and properties keys have an empty parent. Dots in a flat key remain literal and are never split.

YAML mapping keys must be scalar strings to become rows. Complex keys are skipped because they have no string key compatible with B1/B3; their nested values are still traversed where structurally possible. Aliases are not expanded by this extraction pass, preventing duplicate rows invented by expansion. Multi-document YAML is rejected, matching the conservative fix bridge.

Parsers retain their existing size, depth, and syntax guards. A parse error is returned, never converted to an empty row set.

## 4. False-positive surface

This substrate emits data but no findings, so it cannot yet create a policy false positive. Its critical risk is false completeness: claiming duplicate-aware extraction after a parser has already collapsed keys. Every supported adapter therefore reads a lossless document/AST/event stream or the raw bytes directly. Unsupported formats fail explicitly.

INI and properties have dialect ambiguity. Their row scanners share the existing parser conventions: comments, separators, BOM treatment, sections, continuation, and trimming. Constructs whose value boundary is uncertain keep the key row but omit `value_span`; no guessed span is returned.

YAML merge keys and aliases can make an effective map differ from lexical entries. W8 records lexical source entries only. Future rules must state whether they inspect lexical duplicates or effective merged keys; B1 starts lexical because that is what a user can edit and what spans can identify.

## 5. Implementation notes

The public model and dispatch live in `alint-core/src/extract.rs`. Format-specific collectors reuse or sit beside `structured_fix` locators so editing and extraction do not grow divergent dialects. `saphyr-parser` becomes a direct dependency because the high-level `MarkedYaml` map has already collapsed duplicate keys; it is already present transitively through `saphyr`, so this adds no package to the lockfile.

Collection is one pass and `O(number of syntax events)`. Rows own decoded keys and structural paths because they outlive parser AST borrows. Spans are two `usize` values. Existing `extract_values` remains unchanged and pays no additional parse or allocation cost.

## 6. Tests

A table-driven unit gate covers every `Format` variant. The five supported formats must expose two rows for repeated keys with distinct, exact source spans and stable source order. TOML, XML, and HCL must return `UnsupportedFormat`, not an empty successful result.

Additional tests cover nested JSON/YAML arrays, JSONC comments and trailing commas, BOM offsets, quoted/escaped keys, dotenv bare declarations and duplicate assignments, INI sections and continuations, properties separators/escapes, malformed input, YAML aliases, and deterministic repeated runs.

Existing scenario, output, fingerprint, and full workspace suites must remain byte-identical except for separately specified W6 report projections. This proves W8 does not alter existing kinds.

## 7. Open questions

Q5 is resolved: **object members are ordered entry rows, never a JSON object**. Duplicate names remain separate rows. The source span is the row identity; `parent + key` is the semantic grouping key.

Deferred, without weakening W8:

- TOML/XML/HCL adapters. They are explicit unsupported cases until a lossless traversal is implemented.
- Whether YAML merge expansion should be offered as a second, derived view.
- Public relation values and provenance links. W8 supplies only the key/order/span substrate needed by B1 and B3.
