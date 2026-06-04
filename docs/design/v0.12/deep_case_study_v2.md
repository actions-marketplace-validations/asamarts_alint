# Deep case study v2 — synthesis + comprehensive plan (111 repos)

Status: **Analysis — 2026-06-03.** The deeper, more critical successor to the
v0.12 [post-build coverage re-analysis](./post_build_coverage_analysis.md):
a fresh, adversarial per-repo pass over all 111 case-study repos against the
*current* (post-v0.12-build) alint, under the reproduce-first lens. Produced by a
50-agent workflow — 28 deep per-repo dossier agents → cross-repo gap/idea
clustering → 20 adversarial reproduce-first workability verifications → synthesis.
The full **111 per-repo dossiers** are the companion
[`deep_case_study_v2_dossiers.md`](./deep_case_study_v2_dossiers.md).

## 1. Headline

- **~85% of the *addressable* (non-execution) repo-validation surface is natively
  replaceable today** (post-v0.12 build), up from the pre-build 61%. Beyond raw
  replacement, alint *materially improves* validation in ~40–50 repos (one
  reviewable schema'd `.alint.yml` for a pile of untested drift-prone shell/perl),
  and adds net-new **value-add** coherence/structure guards in the execution-only
  repos that gate nothing alint-shaped.
- **77 of 111 dossiers carry no genuine gap** for their addressable surface.
- **The corpus is mature.** After adversarial reproduce-first verification of 20
  clustered candidates, only **3 clean `real_gap` + 13 `partial`** survived — and
  **none rated high priority.** Every "partial" splits into a workable-today half
  plus a narrow genuine residual.
- **The meta-pattern is now overwhelming.** This pass added a **fifth** "gap
  cluster" to dissolve on reproduction (structured-tree iteration — already
  expressible via RFC 9535 `$..` + filter selectors), after file_set, the
  root-file/mid-path-glob no-ops, occurrence:first, and multi-capture-compose. The
  study configs, authored by agents *pre-build*, systematically **over-report
  gaps**. Reproduce a claimed limitation end-to-end before building anything.

## 2. Where alint shines

alint's strength is **declarative cross-file/relational coherence + hygiene +
headers + structure + whole-repo graph invariants + codegen-freshness
verification** — exactly the checks teams hand-roll as piles of under-tested
`scripts/check-*.{sh,py,pl}` and `hack/verify-*.sh`. Execution (AST / lint /
format / type-check / test / sanitizer / NLP) is the deliberate **category-D
non-goal**, correctly orchestrated via the `command` tier, never reimplemented.

By load-bearing capability (corpus frequency):

1. **Cross-file value/version coherence — the most-reached-for capability across
   every archetype (~30+ repos).** The unified `cross_file` kind carries
   archetype-A: pydantic's 4-way version sync, fzf's 4-file version gate,
   prettier's three coherence scripts, curl's `spacecheck.pl` battery → 10 native
   rules, jekyll/redis/prometheus release coherence.
2. **Whole-repo file-graph invariants — the v0.12 build's decisive win
   (`file_graph`, 257 edge-sources).** acyclic / no_dangling / no_orphans /
   forbidden_edges / fresh express the import-firewall + layering + orphan checks
   big monorepos hand-roll (angular ts-circular-deps, k8s import-boss, spring
   layering, eslint/flutter/helm firewalls). No corpus config even *uses* it yet
   (they predate the build) — pure latent uplift.
3. **Per-file header / content-forbidden greps (~15 + ~15 repos).** `file_header`
   (with the `(?m)^` preamble and `(A|B)` alternation idioms) replaces nearly
   every ASF/Google/MS license-header arm; `file_content_forbidden` replaces the
   banned-API/`nocommit`/tab grep suites (ClickHouse `various_checks.sh` ~34
   greps, airflow's ~10 pygrep hooks, pytorch's 7, elasticsearch/spark scalastyle
   bans).
4. **Per-package monorepo structure** (`for_each_dir`/`every_matching_has` +
   `select:` + path templates + nested rules; the value-predicate idiom
   `for_each_dir` + nested `json_path_equals equals:"{path}"`).
5. **Text/unicode hygiene + structured-query (now JSONC-tolerant) + the git-extract
   family** (the universally-applicable baseline + the v0.12
   `git_commit_subject_matches`/`changeset_requires_path`/`pair_changed_together`).
6. **Codegen-freshness verification without becoming a build tool** —
   `generated_file_fresh` (incl. the now-shipped mutating `outputs:` mode),
   `command_idempotent`, `file_graph fresh` keep `alint check` pure
   (snapshot→run→diff→restore).

The 5-axis **"compose, don't accrete"** thesis is vindicated: ~50 study candidates
collapsed to 2 new kinds (`cross_file`, `file_graph`) + shared-axis extensions.

## 3. What % alint replaces & improves — by archetype

Coverage is predicted almost entirely by archetype; *within*-archetype variance is
small, *cross*-archetype variance is the whole story.

| Archetype | Repos | Addressable replaceable | Why |
|---|---|---|---|
| **A — Focused libraries + coherence-heavy** | ~40 | **70–100%** (most 95–100%) | pydantic/serde/clap/curl/flask/prometheus/scikit-learn/phoenix/prettier/ruff/axum/pandas/numpy/black/pytest/jekyll/rails — bespoke version-sync + hygiene + headers replaced nearly entirely |
| **B — Big polyglot monorepos w/ grep-suites** | ~30 | **40–70%** | kubernetes/llvm/spring/nixpkgs low (codegen + import-boss + bytecode-ArchUnit dominate); rust/symfony/airflow/ClickHouse/elasticsearch/deno/grafana ~75–90% (grep + structure + header + firewall native) |
| **C — Pure-execution** | ~40 | **0–30% addressable (≈100% of that small surface)** | gin/express/axios/swift/alamofire/rich/vapor/gohugo/tensorflow/valkey — enforcement is ~100% tool execution; the 0% is correct **non-goal** coverage, not a shortfall. alint's role is value-add coherence, not gating |

The corpus-wide weighted average lands at **~85% of addressable** because the
addressable mass lives in A + the grep-suite half of B (well-covered), while C
contributes near-zero addressable denominator *by design*. **alint's ceiling is
the addressable slice — and within that slice, post-v0.12, it is dominant.**

## 4. The genuine residual — adversarially verified

Twenty clustered candidates were each reproduced end-to-end. Survivors, with the
workable-half / genuine-residual split that recurs in nearly every one:

| Candidate | Verdict | The genuine residual (after the workable half) | Cleanest extension |
|---|---|---|---|
| Capture-aware name-template | **real_gap** (med) | no regex sub-capture in `pair.partner` / nested `paths` / `unique_by.key` / `file_graph derive_target`+`no_dangling` (elasticsearch `-LICENSE`↔`-NOTICE`, git `tNNNN` dup, git gitlink) | `from:`/`$1` on the ④ name-template (reuse file_graph's `caps.expand`); decouple `derive_target` from `fresh` (delete the `file_graph.rs:833` rejection) |
| `split:` on extract | **real_gap** (low) | N resolvable paths packed in one string by a non-newline delimiter (quarkus `native-tests.json`) | `split:` on the ① shared extract axis (last step of `extract_values`; inherited by all consumers) |
| Markerless `ordered_block` | **real_gap** (low) | sort-to-EOF with no `end:` marker (airflow `spelling_wordlist.txt`) | make `end:` optional (+ `skip_leading:`) on the existing kind |
| Forbid-with-exception | partial (med, freq 9) | inline `allow:`/`except:` ergonomics + a lookaround-companion (pandas `# type:` except ` ignore`) — the bulk (set ⊆ allowlist) is workable via `cross_file subset` | inline `allow:`/`except:` reusing capture-1; `unless_followed_by`/`unless_preceded_by` (two linear regexes, no PCRE) |
| Map-intersection coherence | partial (med, freq 1 real) | dynamic shared-key intersection (vscode `checkPackageJSON`) — lockstep half workable via glob targets | keyed `json_pairs:` extract + `relation: intersection_equals` |
| Glob→set / file_set | partial (med) | bare-token entry→path transform (git command-list, eslint barrel) — the reverse-set half already ships as `registry orphans` | `entry_template:` + `orphans.space` exclude on `registry_paths_resolve` |
| `content_class` size-cap | partial (med) | "cap only binaries" (k8s `verify-file-sizes`) — binary *detection* already ships as `file_is_text` | `content_class: text|binary` on `scope_filter` (reuse `classify_bytes` sniff) |
| `for_each_match` (block grammar) | partial (med) | monolithic-changelog per-line conjunction with strip-then-assert (rubocop) — fragment changelogs workable via stacked rules | **the one new kind worth minting** — `for_each_line`/`for_each_match` (select + `capture:` + nested `require:`) |
| git-tag value source | partial (low) | manifest == pushed tag (ripgrep/postgrest) — runtime ref, category-D-adjacent; committed-to-committed coherence already works | opt-in `git: { describe }` source + `strip_prefix` normalize, CI-gated |
| value_set inline allowlist | partial (low) | inline `values:` set (vs a committed allowlist file) — the file-backed form already ships | `values:` literal extract source (mirrors `import_gate allow:`) |
| set-minus / `except:` | partial (low) | set subtraction before a relation (vim hlgroups, 1 repo) | `except:` ExtractSpec on `cross_file` set relations |
| structured-tree iteration | partial (low) | **dissolved** — `$..` + RFC 9535 filters express it; residual is ergonomic | `assert_empty:` / violator-selector sugar on structured-query |
| dirname predicate | partial (low) | **dissolved** — `dir_absent` + globset char-classes express it; residual is case-insensitive glob | `case_insensitive:` on the shared glob axis |
| version fan-out | partial (low) | **dissolved** — 1-source→N-target is already one `cross_file` rule; residual is component-split (rails, Tier-3) | optional `components:` on extract (gate behind recurrence) |
| self-ref count/checksum | partial (low) | checksum = tool-specific digest (**non-goal**, `command_idempotent` covers); count-header is Tier-3 | (defer) cardinality extract + numeric relation arm |

**Recurring shape:** almost every candidate is a *small, additive enrichment of
the 5 shared axes* (extract sources/options · normalize ops · selectors · relations
· graph), not a new kind. The lone genuinely-new behavior worth minting is
`for_each_match` (the ④ in-file block/line quantifier).

## 5. Major opportunities — ranked

Ranked by cross-repo demand × leverage × architectural cleanliness. The former #1
(mutating codegen-freshness, ≈23 repos) **shipped 2026-06-03**.

1. **[HIGH — product, zero engine code] `php@v1` / `composer@v1` bundled ruleset.**
   The single highest-leverage *product* move: PHP is the one ecosystem with
   first-class corpus demand (composer/laravel/symfony/guzzle/phpstan) and no
   bundled ruleset (rust/go/java/node/dotnet/python all have one). Composed
   entirely of existing kinds — mirror `dotnet@v1` (facts.has_php gate + per-rule
   `when:`, 6–8 rules, 3 e2e scenarios). Ships in one design-doc-first cycle, zero
   engine risk.
2. **[MED] Glob-union source for `cross_file` + `entry_template:`** — closes the
   #6 symbol-set/cross-lang parity bucket (8 repos: vim hlgroups, protobuf binding
   parity, eslint barrel, git command-list). Reuses the shipped set relations.
3. **[MED] Capture-aware name-template (+ decouple `derive_target`).** The most
   *generative* primitive: `from:`/`$1` on `pair.partner`/nested `paths`/`unique_by.key`,
   reusing file_graph's proven `caps.expand`. Half-B (decouple) is a near-free quick win.
4. **[MED] `for_each_match`** — the one new kind worth minting; absorbs rubocop's
   ~15-clause changelog grammar + count-header + max-consecutive-spaces + intra-file
   reference checks, and enables a future `keep-a-changelog@v1`.
5. **[MED] Keyed extract + `relation: intersection_equals`** (vscode shape) and
   **`content_class:` scope predicate** (binary-only size caps) — both clean axis
   enrichments, smaller buckets.
6. **[LOW, batch as one cheap polish cycle — outsized aggregate unblocking]** the
   extract/normalize long tail: `split:`, `occurrence: first|nth` (ergonomic sugar
   over the working `(?s)\A.*?` idiom), markerless `ordered_block`, inline
   `values:`/`except:`, `strip_prefix` normalize. Each is a one-field additive that
   all consumers inherit.
7. **[LOW — gate behind ≥2–3 repo recurrence] the single-repo gaps:** `git: {describe}`
   source, `at:` prev-git-rev append-only, lookaround-companion, `components:`,
   self-ref count-header. Each has a working committed-file workaround and 1-repo
   demand.

**Hold the non-goal boundary firm.** AST / NLP / tool-specific digests (cpython
Argument Clinic) / sanitizers / type-checkers / formatters stay on the
`command`/`command_idempotent` spawn trust-tier. Do **not** build:
`version_consistency`/`version_components` (shadows `cross_file`), `dirname_regex`
(`dir_absent` does it), `embedded_checksum`, a generic "A unless B" combinator,
`structured_for_each` (`$..` + filters do it), a PCRE/fancy-regex engine (forfeits
the linear-time guarantee).

## 6. Comprehensive end-to-end plan

**State (2026-06-03):** 88 distinct rule kinds, 10 bundled rulesets, the 5-axis
architecture intact, **~85% of addressable corpus surface natively covered, the
dominant residual (mutating codegen-freshness) closed.** Every verified gap is
medium/low priority and most are workable today.

**These remaining items are not a new cycle — they are the deferred tail of the
*same* study-gated v0.12 work:** glob-union (#6), the extract/normalize refinements
(#4), `for_each_match` (#2; the architecture synthesis nominated exactly this), and
the study-surfaced PHP ruleset. v0.12 is still on `[Unreleased]`, so they fold in
directly. Shipping the full study-driven surface in one coherent v0.12 beats
cutting at 85% and deferring the rest to a separate minor.

### The v0.12 finish-line backlog (the study's remaining deferrals), in leverage order
1. **`php@v1`/`composer@v1` bundled ruleset** — highest leverage, zero engine code.
2. **The two load-bearing axis-extensions:** glob-union source + `entry_template:`
   (closes the #6 bucket); capture-aware name-template + `derive_target` decouple.
3. **The cheap extract/normalize long tail** as one polish cycle (`split:`,
   `occurrence:`, markerless `ordered_block`, inline `values:`/`except:`), and
   *credit* the already-shipped `casefold`/`basename`/`strip_prefix` rather than
   rebuilding.
4. **`for_each_match`** (the deliberate new ④ kind) + `content_class:` scope
   predicate → unlocks `keep-a-changelog@v1`.
5. **Defer** the single-repo gaps until recurrence; **document the workaround
   idioms now** in `docs/rules.md` (the `cross_file subset` inline-allowlist idiom,
   the `(?s)\A.*?` occurrence idiom, the `dir_absent` directory-name recipe, the
   `$..`+filter violator-selector) — they are the immediate answers and are
   currently undocumented.

### Validation-debt win (cheap, high-credibility — do alongside)
The corpus configs were authored pre-build and **understate** coverage
(`file_graph` appears in zero configs). Re-author the ~15 monorepos whose B-gaps
`file_graph` silently closed (angular/k8s/spring/eslint/flutter/helm/airflow + the
`set_equals` repos istio/mypy/numpy), re-pin + `alint check` each against a fresh
clone (the calibration gotcha catches latent config bugs). Output: the "85% of
addressable" claim demonstrated in runnable configs, not just asserted.

### Then cut v0.12
Build the backlog above on `[Unreleased]`, then the release gate: the **1M-file
macro bench** (extend S11–S13 to cover `for_each_match`'s dispatch class — the only
item here that adds one; the rest are axis-enrichments with no bench impact) → the
**release cut**. Run the bench *once* over the final surface, then tag.

### Release / validation discipline (unchanged, proven)
- **Design-doc-first** per increment (draft commit flips a Status line → atomic
  rule+wiring commit). Apply the preflight lessons preemptively.
- **Every new kind/mode needs a firing AND a silent e2e scenario** (+ the
  cross-file-class lock where applicable); bundled rulesets need +1 well-formed
  +1 ill-formed scenario.
- **Rule-count discipline:** `for_each_match` is the only `+1` in this plan;
  everything else is axis-enrichment or a mode (count unchanged). README/about/
  all_kinds.yaml + the `docs_export.rs` headline move in lockstep.
- **Bench gate (D5) is a release blocker;** extend `Scenario::all()` +
  `bench-record.yml --scenarios` before the cut if a phase adds a dispatch class.
- **Watch the recurring fail-modes:** npm-token mid-release expiry (rotate
  preemptively) and the harness `git push` SIGPIPE (verify by `origin/main` REF).

**Bottom line.** alint is at ~85% of *addressable* corpus coverage with the
dominant residual already closed; the rule surface is mature and the architecture
thesis held. The remaining v0.12 work — folded in from the study's own deferrals —
is one product move (the PHP ruleset, zero engine risk), a small set of clean
shared-axis enrichments, and one deliberate new quantifier (`for_each_match`), then
the bench + cut. **The corpus, not the engine, now understates the win.**
