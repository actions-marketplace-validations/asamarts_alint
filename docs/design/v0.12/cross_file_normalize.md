# `normalize:` value-transform on `cross_file_value_equals`

Status: **Shipped — 2026-06-03** (`crate::cross_file`, rule count unchanged,
CHANGELOG `[Unreleased]`). Both corpus signals are reconciled by a SINGLE new
transform — `semver-minor` (the `MAJOR.MINOR` band, each token's leading digits
with a non-digit prefix stripped: `4.36-dev`/`4.36.0`/`pnpm@11.3.0`/`>=22.13`
all collapse to one band), so the `strip_prefix`/`strip_suffix` transforms below
turned out unnecessary and are **deferred** (add only if a non-version case
appears). The "promotion" delivered is the **composable list** form
(`normalize: [trim, semver-minor]`, applied left-to-right; scalar back-compat
preserved) + `semver-minor`. `casefold` also deferred (`lower` covers ASCII).

## Motivation / demand

`cross_file_value_equals` already has a `normalize:` field, but it
supports only `trim` / `lower`. Two corpus repos express the same
logical value in two *forms* that no trim/lower can reconcile, forcing
them to fall back to brittle dual regex pins:

- **protobuf** — `version.json` carries `4.36-dev` while
  `protobuf_version.bzl` carries `4.36.0`: the same release in two
  formats. The canonical demand-driver.
- **pnpm** — `pnpm@11.3.0` (a `packageManager` field) vs `11.3.0`
  (a plain version); nodeVersion `22.13.0` vs a member's `>=22.13`.

## Sketch

Extend the `normalize:` enum (and/or accept a list of transforms
applied in order):

```yaml
- id: protobuf-version-coherent
  kind: cross_file_value_equals
  source:  { file: version.json, extract: { json: "$.cpp" } }
  targets: [{ file: protobuf_version.bzl, extract: { regex: '...' } }]
  normalize:
    - strip_suffix: "-dev"      # 4.36-dev -> 4.36
    - semver_floor                # 4.36.0  -> 4.36  (drop patch)
```

Candidate transforms (all pure string→string, order-applied):

- `strip_prefix` / `strip_suffix` (literal or pattern)
- `semver_floor` / `semver_major_minor` (normalise `x.y.z` ↔ `x.y`)
- existing `trim` / `lower` (keep, compose)

## Open questions

- Scalar vs. list: keep `normalize: trim` working (back-compat) while
  also accepting `normalize: [..]`. Serde untagged-enum or a small
  custom deserializer.
- Should the same `normalize:` apply to `registry_paths_resolve` and
  the membership family? (Likely yes — push it into the shared
  `crate::extract` post-processing so every cross-file kind benefits.)
- Semver handling: pull in a tiny semver parse, or regex-only? (Lean:
  regex-only transforms first; add semver if the study shows demand.)
