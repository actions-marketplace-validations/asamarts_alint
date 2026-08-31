# v0.12 case study — calibration worked-example: `tokio-rs/tokio`

Status: **Calibration reference — produced 2026-06-01.** Per
[`case_study_100_repos.md`](./case_study_100_repos.md) §"Consistency + drift
control", this is the ONE repo fully worked under the protocol that **every
batch agent reads first**, so the unit + taxonomy land identically across the
~20 batches. **Read the Rulings section before cataloguing any repo.**

## Repo + pin

- `tokio-rs/tokio` @ `32312ae0d6f0b1c6457f1323e3e7f568f448d0db` (pinned
  2026-06-01; depth-1 fetch of the exact SHA into a scratch dir — no write-back
  to upstream).
- Shape: mid-size single-language **Rust workspace library** — 5 published
  crates (`tokio`, `tokio-macros`, `tokio-stream`, `tokio-test`, `tokio-util`)
  + 5 internal crates (`benches`, `stress-test`, `tests-build`,
  `tests-integration`, `examples`). Governance: community. Build: cargo.
- Why this repo: the deliberately over-weighted high-fit tier (mid-size lib),
  tractable to clone, and a baseline repo with a prior `.alint.yml` to ground
  and stress-test against.

## How the protocol ran here

- **Stage A (deep-read):** catalogued every enforced validation from
  `.github/workflows/**` (`ci.yml`'s ~45 jobs + 6 specialised workflows), the
  `check-readme` / `check-spelling` shell gates, `deny.toml`, `spellcheck.toml`,
  `CONTRIBUTING.md`, and the workspace structure; classified each A/B/C/D;
  ran/validated `examples/tokio-rs-tokio/.alint.yml`.
- **Stage B (adversarial verify):** an independent agent re-read the repo to
  refute Stage A's completeness — reconciled below.
- **Integration:** `alint validate-config` + `alint check` against the pinned
  clone. This caught a latent bug parse-only validation missed (see Validation).

## Rulings — the ambiguities this calibration locks (apply them everywhere)

**R1 — Count behaviors, not instances.** `ci.yml` runs the test suite across 3
OSes × many feature sets × ~15 targets: that is **one** D behavior ("tests
pass"), not 40. Conversely `miri` / `asan` / `valgrind` / `loom` are **distinct**
behaviors (UB / address / leak / concurrency) → separate D units. The MSRV
checklist names 7 files to hand-sync; that is **one** A behavior (one
`cross_file_value_equals`), not 7.

**R2 — Tool execution is D, even though alint can wrap it.** "CI runs
`cargo fmt --check` / clippy / test / miri / semver-checks / cargo-deny /
cargo-spellcheck" is **D** (build/test/AST/NLP execution). alint *can* shell out
via `command` / `command_idempotent` (the prior tokio config wraps 6 such tools
for single-entrypoint orchestration — legitimate value), but **wrapping a tool
is not alint expressing the check's logic.** Counting wraps as A makes
`coverage_today` meaningless (alint can shell out to anything). Tool-runs stay
D; the wrap is recorded as orchestration value-add, separately.

**R3 — Config-policy assertion (A) ≠ policy enforcement (D).** `deny.toml`
*declares* the license allowlist + wildcard ban → alint can assert that
declaration (`toml_path`, A) — but that is **alint value-add**, not a
tokio-enforced check. What tokio *enforces* is "dependencies comply" (cargo-deny
resolves the dep graph) → **D**. The enforced unit is the D compliance check;
the `toml_path` assertion is a bonus alint can add, recorded as value-add, NOT
in the coverage denominator.

**R4 — A rule that PARSES is not a check that's EXPRESSED.** The prior config
expressed "README.md ≡ tokio/README.md" with `pair_hash format: contains`. It
parses and loads — but `pair_hash` asserts "the source's digest appears as a
substring **inside** the target" (a checksum-manifest check), not file equality.
Against the clone it FAILS even though the two files are byte-identical (same
sha256). So this is **not A — it's B** (a missing `files_equal` primitive).
**Classify A only after `alint check` confirms the rule passes/fails correctly
against the clone, never from "the config loads."**

**R5 — The denominator is what the REPO enforces.** Count CI gates, shell-script
assertions, hooks, cargo-deny compliance, AND documented review conventions
(CONTRIBUTING + load-bearing comments). Structure alint *could* assert but the
repo does not gate on (e.g. "ci.yml exists", "every crate has a README" absent
any such gate) is **alint value-add**, recorded separately, **never in the
A/B/C/D ratio** — else coverage inflates with arbitrary assertions.

**R6 — Record absent categories as findings.** tokio has **no per-file license
headers** (one root MIT `LICENSE` + per-crate `Cargo.toml license`) and **no
tracked `Cargo.lock`** (library convention — gitignored). Don't force a rule;
record the absence — it is a corpus data-point (the `hygiene/lockfiles` and
apache-header bundles must NOT fire here).

## Classified catalogue (the worked unit)

### Addressable surface (A / B / C) — what alint is measured on

| # | Enforced validation (where) | Bucket | alint kind | vs. clone |
|---|---|---|---|---|
| A1 | MSRV value coherence: the 5 crates' `[package] rust-version` + `ci.yml rust_min` must all equal `1.71`; `ci.yml` lines 23-31 are a literal "when you change this, also update these files" review checklist | **A** | `cross_file_value_equals` | PASS\* |
| A2 | `tokio` version in `tokio/Cargo.toml` appears in README (check-readme `grep -q`) | **A** | `cross_file_value_equals` | PASS |
| A3 | No trailing whitespace, repo-wide (check-spelling `grep -rne '\s$'`) | **A** | `no_trailing_whitespace` | PASS |
| A4 | `spellcheck.dic` sorted + unique after the header line (check-spelling `sort -uc`) | **A** | `ordered_block` | PASS |
| M1 | clippy-version coherence: `ci.yml` `rust_clippy: '1.88'` must match the `cargo +1.88 clippy` pin in `docs/contributing/pull-requests.md` (the review-checklist twin of A1) | **A** | `cross_file_value_equals` | convention |
| B1 | `README.md` byte-identical to `tokio/README.md` (check-readme `diff`) | **B** | gap: whole-file `files_equal` (`pair_hash` is digest-in-manifest; `pair` is existence-pairing — *verified*, neither does byte-equality) | — |
| B2 | `spellcheck.dic` header is a valid count of the following lines (check-spelling) — the integer-ness of line 1 is this behavior's guard clause, **not** a separate unit | **B** | gap: "line 1 declares the count of the following lines" | — |

\* A1 PASSES only after this calibration fixed a dashed-key JSONPath bug — see
Validation.

**Addressable totals: A = 5, B = 2, C = 0** →
`coverage_today = A/(A+B+C) = 5/7 ≈ 71%`;
`coverage_with_tuning = (A+C)/(A+B+C) = 71%`;
`gap = B/(A+B+C) = 2/7 ≈ 29%`.

### Deliberate non-goals (D) — recorded so the denominator is honest

tokio's enforced surface is **dominated by execution/semantic checks alint does
not reimplement** (~20 behaviors, matrix collapsed per R1): rustfmt · clippy ·
rustdoc-warnings · cargo-deny dependency compliance · cargo-semver-checks · the
test suite (nextest × OS × features) · **miri** · **asan** · **valgrind** ·
**loom** · check-external-types (public-API surface) · cargo-fuzz ·
feature-powerset compile (cargo hack) · MSRV build · minimal-versions ·
cross/exotic-target builds (powerpc / arm / wasm×4 / freebsd×3 / redox / sgx /
haiku / i686-no-AtomicU64) · stress-test · io_uring-per-kernel-version ·
downstream hyper/quinn integration · cargo-spellcheck (prose NLP).

**The honest picture:** of ~27 enforced behaviors, **~20 (74%) are D non-goals**
that alint *orchestrates but does not reimplement*; the **7 addressable** ones
are where alint is scored, and it natively expresses **5/7 (71%)**.

### alint value-add (NOT in the coverage ratio, per R3/R5)

Real alint utility, excluded from the denominator: orchestrating the D tools
through one `command_idempotent` entrypoint (6 wrappers); asserting `deny.toml`'s
license-allowlist + wildcard-ban policy (`toml_path`); structural assertions
tokio doesn't gate on (every published crate has README+CHANGELOG; internal
crates `publish = false`; crates inherit workspace lints; edition coherence; the
`[patch.crates-io]` block; the CI/deny/spellcheck config files exist). These are
why the shipped config carries ~33 repo rules though the *enforced* addressable
surface is 7.

## File-reference-graph harvest (the `file_dependency_graph` gate)

tokio enforces **cross-file VALUE consistency** (A1, A2) and a **file-PAIR
identity** (B1), but **no file→file REFERENCE graph**: no import-layering /
module-boundary firewall, no codegen-freshness `git diff --exit-code`, no
orphan/cycle detection, no "every X has a corresponding Y" edge. **File-graph
edge sources from this repo: 0** — consistent with the protocol's running
"0 file-graph sources" tally feeding the study-gated `file_dependency_graph`
go/no-go.

## New-kind candidates (B) with evidence

- **`files_equal` / whole-file byte-identity** (B1). Source: tokio check-readme
  `diff README.md tokio/README.md`. `pair_hash` can't express it (digest-in-
  manifest only); `pair` is existence-pairing. Demand so far: tokio ×1 — watch
  the corpus (mirrored READMEs / vendored-file copies are common).
- **count-header — "line 1 declares the count of the following lines"** (B2).
  Source: tokio `spellcheck.dic` (hunspell format). Niche; tokio ×1.

## Validation against the pinned clone (the integration step — and why it matters)

- **`validate-config`:** the full config parses — 73 rules (5 bundles + 33 repo
  rules) load at the pin.
- **`alint check` (native rules vs the clone):** 13/15 repo rules pass,
  **including A1 (MSRV coherence) — but only after this calibration fixed a
  latent bug the re-pin surfaced.** The rule used `toml: "$.package.rust-version"`;
  the dashed key needs bracket notation `$.package['rust-version']` (a documented
  alint pitfall). Parse-only `validate-config` passed *with the bug present*;
  only running `alint check` against the clone exposed it (`bad JSONPath …
  parser error`). **This is why Stage A integrates against the clone, not just
  `validate-config`.**
- Remaining repo-rule failures are the findings above, not surprises: B1
  readme-mirror (the `pair_hash` misuse — reclassified B); the prior config's
  `tokio-util-dep-major-matches` is a *demonstrative* rule, not a tokio-enforced
  check (ambiguous regex matching several `version =` lines) — dropped from the
  denominator.
- Bundle findings against tokio (real or tuning, all enumerated):
  `gha-pin-actions-to-sha` ×172 (tokio pins actions by tag, not SHA — real,
  noisy); `rust-sources-snake-case` ×3 on the intentional `test-*.rs` bin names
  (**C tuning** — wants a `paths.exclude`); `rust-cargo-lock-exists` (library
  gitignores Cargo.lock — **R6 absent-category**, disable for libs);
  `oss-codeowners-exists`, `rust-toolchain-pinned`, `gha-workflow-contents-read`
  (real, low-severity).

## Stage B reconciliation

An independent Stage-B agent re-read the repo (every workflow, all four
`docs/contributing/*.md`, buildomat, PR/issue templates, `Cross.toml`,
`netlify.toml`, all six Cargo.tomls) to refute Stage A. Three corrections were
accepted; the headline metric is unchanged but the A-list composition is fixed:

- **M1 added (A).** Clippy-version coherence: `ci.yml` `rust_clippy: '1.88'` is a
  documented cross-file convention — `docs/contributing/pull-requests.md` says to
  use the same clippy version as CI and hard-codes `cargo +1.88 clippy`, with the
  same review-checklist enforcement as MSRV. The twin of A1; Stage A missed it by
  reading only the short top-level `CONTRIBUTING.md`, not `docs/contributing/*`. →
  `cross_file_value_equals`, **A**.
- **A5 folded into B2 (−1 A).** "dic first line is an integer" has no independent
  failure semantics — it is the guard clause of the header-count check (B2), the
  same shell step (`ci.yml:1237-1251`). One behavior, not two.
- **Net on the count: +1 A (M1) and −1 A (A5→B2) cancel → A = 5, B = 2, C = 0,
  `coverage_today` = 5/7 ≈ 71% (unchanged).** Stage A's *cardinality* survived;
  its *enumerated A-list* was corrected (swap the integer-guard for clippy
  coherence).
- **A1 enforcement tier clarified.** A1 and M1 are **review-checklist
  conventions, not CI gates** — `minrust` (`cargo check` at the pinned `1.71`)
  would catch a crate demanding a *higher* MSRV than the pin, but it does not
  enforce cross-crate *equality*, so the coherence itself is un-gated. They count
  because the protocol's denominator (R5) explicitly includes "rules humans
  enforce in review." Factual fix: the `ci.yml:23-31` checklist names
  `CONTRIBUTING.md`, but that file carries no numeric MSRV (only "6-month-old
  Rust") — so A1's machine-extractable coherence set is the **6 value-sites**
  (5 crate `rust-version` + `ci.yml rust_min`); the README/CONTRIBUTING mentions
  are prose, not a 7th site.
- **The denominator fork — the one place the metric moves.** If the study counts
  only CI-*gated* checks and excludes review-enforced conventions, A1 + M1 drop →
  **A = 3, B = 2 → 3/5 = 60%**. Under the protocol's stated denominator
  (conventions included) it is **5/7 ≈ 71%**. **Ruling for batch agents: apply R5
  as written — conventions count** — for comparability; flag both numbers only
  when a repo's headline hinges on this fork.
- **B1 stays B (highest-leverage item, now verified).** Stage B flagged that
  Stage A never tested the `pair` kind for whole-file equality. Checked against
  the source: `pair` asserts **partner existence** ("every `.c` has a same-dir
  `.h`"), `pair_hash` is digest-in-manifest; neither expresses byte-identity, so
  **B1 (`files_equal`) is a real gap** — coverage does NOT rise to 6/7.
- **File-graph 0 confirmed.** Stage B's refutation failed: **no `build.rs`
  anywhere** (nothing to diff for codegen freshness), no `git diff --exit-code`,
  no commit-lint / husky / lefthook, no module/import/layering gate. The only
  file→file assertion is the README pair-identity (B1), a 2-file equality, not a
  reference graph. **0 file-graph edge sources stands.**
- **No D→A/B/C upgrades.** The closest call is `deny.toml`'s `wildcards = "deny"`
  (a dependency-spec policy that *looks* `file_content_forbidden`-shaped), but
  tokio enforces it by running cargo-deny over the resolved graph → **D** per R2.

## Worksheet for batch agents (replicate the unit exactly)

1. Enumerate enforced validations from CI jobs + shell gates + hooks + cargo-deny
   + documented review conventions (R5). Collapse matrix instances (R1).
2. For each: **A** only if a native alint kind expresses the *logic* and
   `alint check` confirms it (R2, R4); **B** if a new primitive is needed (name
   it + the gap shape); **C** if expressible-but-awkward (preset / normalize /
   bundle relaxation); **D** if execution / AST / semantic / NLP (R2).
3. Exclude alint value-add assertions from the ratio (R3, R5); record absent
   categories (R6).
4. Report A/B/C/D counts → `coverage_today`, `coverage_with_tuning`, `gap`.
   Harvest file→file reference edges (or record 0).
