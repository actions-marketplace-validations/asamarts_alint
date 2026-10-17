---
status: accepted
date: 2026-10-17
decision-makers: asamarts
---

# 0021. Policy-source files may decide conformance verdicts

## Status

Accepted (2026-10-17). This records DEC-12 from
[`general-linter.md`](../design/general-linter.md).

## Context

ADR-0010 permits manifest-derived values to narrow a rule's scope, but does not permit them to
decide whether a selected file passes. That boundary prevents incidental build metadata from
silently redefining lint policy. It also prevents `editorconfig_conforms`: the purpose of an
`.editorconfig` file is precisely to declare the formatting policy that editors and checkers
apply.

Copying those values into `.alint.yml` is not safer. It creates two policy sources that can drift,
and a check can then pass while an editor follows the repository's different, canonical setting.
The general-linter program therefore distinguishes a manifest from a file whose documented
purpose is to declare the policy being checked.

## Decision

A rule may derive its verdict from a **policy-source file** when all of these conditions hold:

1. The kind names the policy format and semantics in its public contract; a configured arbitrary
   file is not implicitly promoted to policy.
2. Discovery is deterministic and confined to the lint root. Files above the lint root, network
   state, user configuration, environment-specific search paths, and symlink escapes never
   participate.
3. Precedence and `unset` behavior follow the policy format rather than alint inventing a second
   merge model.
4. Every conformance finding cites the exact policy declaration as a related location. A missing,
   unreadable, malformed, or unsupported declaration needed for a verdict is an analysis failure,
   not a pass or an ordinary policy violation.
5. The kind's `KindMeta` declares every resource and capability it uses. Policy discovery and
   parsing remain bounded by the same repository read limits as other rule inputs.
6. Policy values affect only the kind that explicitly consumes them. They do not mutate alint's
   global configuration, target walk, other rules, or output settings.

`editorconfig_conforms` is the first policy-source consumer. It discovers `.editorconfig` files
only at ancestors of selected repository files, stops inheritance at the nearest `root = true`,
applies matching sections from outermost to innermost and top to bottom, and reports the
controlling declaration with each finding.

## Consequences

- ADR-0010 remains the rule for intent-inferred build manifests. This ADR adds a narrow,
  auditable class rather than weakening that boundary for every manifest read.
- A policy-backed kind can keep one source of truth and cannot silently inspect a developer's
  parent-directory settings.
- Policy parsers and provenance are part of the rule's correctness and security surface. Invalid
  supported properties fail loud.
- Future policy-source kinds require their own documented format contract and the same confinement,
  provenance, and failure behavior; this ADR is not permission to read arbitrary tool config.

## Alternatives considered

- **Repeat policy in `.alint.yml`.** Rejected because the duplicate values drift and defeat the
  config-as-source-of-truth use case.
- **Allow any manifest value to decide any verdict.** Rejected because it removes ADR-0010's
  safety boundary and makes rule behavior implicit.
- **Use the host EditorConfig search without confinement.** Rejected because a checkout's verdict
  would depend on files outside the repository.
