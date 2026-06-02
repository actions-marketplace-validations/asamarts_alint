# v0.12 100-repo case study — running log

The per-batch record of the study defined in
[`case_study_100_repos.md`](./case_study_100_repos.md), calibrated by
[`case_study_calibration.md`](./case_study_calibration.md) (`tokio-rs/tokio`,
the six rulings R1-R6). Each batch is run as a multi-agent workflow (per repo:
Stage A deep-read + draft/validated config → Stage B adversarial verify) against
depth-1 clones at the pinned SHAs in [`case_study_repos.md`](./case_study_repos.md).
Counts below are **Stage-B reconciled**. Coverage = `coverage_today = A/(A+B+C)`
(D non-goals excluded from the denominator, per R5).

Validated draft configs land per repo; corpus-storage (in-repo `examples/` vs a
separate corpus) is still the protocol's open question, so this log is the
durable artifact and the configs are staged separately pending that decision.

## Running totals

| | repos | A | B | C | D | coverage_today | file-graph edge sources |
|---|--:|--:|--:|--:|--:|--:|--:|
| calibration (tokio) | 1 | 5 | 2 | 0 | ~20 | 71% | 0 |
| batch 1 | 5 | 31 | 6 | 5 | 81 | **74%** | **21** |
| batch 2 | 10 | 25 | 10 | 9 | 127 | 57% | 13 |
| **cumulative** | **16** | **61** | **18** | **14** | **~228** | **66%** | **34** |

---

## Batch 1 — 2026-06-01 (diverse 5: Python / Go / Ruby / C / JS)

Workflow: 10 agents, ~1.1M tokens. Every Stage-A config was validated with
`alint check` against its clone and every non-trivial A-rule positive-controlled
(R4); Stage B independently re-derived or corrected each record.

| repo | shape | A | B | C | D | cov_today | file-graph | Stage B |
|---|---|--:|--:|--:|--:|--:|--:|---|
| `pallets/flask` | Python web-fw lib | 8 | 0 | 0 | 10 | **100%** | 1 | no change (HQ) |
| `prometheus/prometheus` | Go infra/CNCF | 4 | 0 | 0 | 22 | **100%** | 5 | +1 A |
| `rubocop/rubocop` | Ruby CLI/linter | 5 | 3 | 1 | 13 | 56% | 1 | +1 B |
| `curl/curl` | C systems CLI+lib | 9 | 2 | 2 | 24 | 69% | 8 | +1 A |
| `eslint/eslint` | JS CLI/linter | 5 | 1 | 2 | 12 | 63% | 6 | +1 A |

**Coverage shape.** Pooled 31/42 = **74%** (vs the tokio calibration's 71%),
per-repo range 56-100%. The split is informative: libraries whose bespoke
validation is **cross-file value-coherence + hygiene + an import firewall**
(flask, prometheus) score ~100% of their addressable surface; **linters / systems
projects with bespoke per-line grammars, set-equality, or path constraints**
(rubocop, curl, eslint) carry real B gaps. As with tokio, the bulk of every
repo's enforced surface is D (execution / AST / build) — 81 of 123 behaviors —
that alint orchestrates but does not reimplement.

### Headline: the `file_dependency_graph` gate flips

tokio surfaced **0** file-reference-graph edge sources; batch 1 surfaced **21**,
in **every** repo, spanning all the edge shapes the
[`file_dependency_graph`](./file_dependency_graph.md) design names:

- **Existence edges** (every-X-has-a-Y): eslint rule→doc + rule→test
  (`Makefile.js:848,916`, basename convention).
- **Value / set-equality edges**: curl option↔`docs/curl.1`↔`tool_listhelp.c`
  set-equality (`tests/test1139.pl`); eslint rule→doc-title value
  (`Makefile.js:775` `hasIdInTitle`).
- **Codegen-freshness edges** (generate then `git diff --exit-code`): prometheus
  ×5 (PromQL function sigs/docs, goyacc parser — `Makefile:106`), curl ×1.
- **Import-layering / module-boundary firewalls**: flask `sansio/` cannot import
  the Flask globals (content-regex over imports, `import_gate`); eslint.
- **Intra-file reference / dangling-edge**: rubocop CHANGELOG implicit-link
  resolution (`spec/project_spec.rb:371`).

Edge *sources* vary (content-regex, naming-convention, manifest-declaration,
generated-diff) — which is exactly the case for a **generic** file-graph kind
rather than per-ecosystem rules. **Recommendation: lift `file_dependency_graph`
from "study-gated (0 sources)" to a v0.12 build candidate.** Note many of these
edges are enforced today via bespoke Perl/Ruby/JS (so D as *current* alint kinds)
but are precisely what the new kind would express natively.

### New-kind candidates (B) — ranked by cross-repo demand

1. **`file_dependency_graph` / generic file-reference graph** — 21 sources, all
   5 repos, all edge shapes (above). The dominant signal of the batch.
2. **`every_X_has_Y` with a value-equality predicate on the partner**
   (parameterized cross-file: "every rule file's basename == its doc's `title:`
   frontmatter") — eslint `Makefile.js:775`; overlaps curl set-equality. A
   focused cross-file primitive distinct from today's existence-only
   `every_matching_has`.
3. **`implicit_link_resolves`** — every `[name][]` Markdown reference has a
   co-file `[name]: http…` definition (rubocop `spec/project_spec.rb:371`). An
   intra-file orphan-edge graph; subsumed by (1) if the file-graph kind handles
   intra-file edges.
4. **`changelog_entry_format`** — per-line conjunctive grammar ("every line
   matching `^\* ` must also satisfy Q1..Qn") — rubocop changelog discipline
   (`spec/project_spec.rb:200-284`).
5. **`in_file_line_uniqueness`** — uniqueness of lines matching a regex *within
   one file* (today `unique_by` is path-keyed) — rubocop contributor names.
6. **`path_length_cap`** — `len(path) <= N` / `len(basename) <= M` — curl
   `scripts/spacecheck.pl:111` (64/48 caps), CI-gated.
7. **`max_consecutive_spaces` / `no_repeated_chars`** — forbid a run of ≥N of a
   character in a line — curl `spacecheck.pl:109`.

### alint sharp-edges surfaced (C-tuning — actionable now, with proof)

These are real product defects/relaxations the corpus proved, independent of any
new kind:

- **`no_merge_conflict_markers` false-positives on reST/Markdown setext
  underlines** (a `=======` title underline reads as a conflict marker) — flask
  `docs/*.rst`. Wants a setext-aware skip. *(clear bug, cheap fix.)*
- **`file_is_ascii` needs an `allow:` codepoint exemption** — PROVEN firing on
  curl `lib/mqtt.c`/`.h` ("Björn", U+00F6); curl's own `spacecheck.pl` allows it.
- **`ordered_block` wants a `pattern:`/`select:` line filter** (sort only matched
  lines) — rubocop.
- **`every_matching_has` `select:` needs include/exclude** (negation) — eslint.
- **`import_gate language: js` over-matches JSDoc `@typedef {import(...)}`** —
  eslint; the preset should ignore comment context.
- **`gha-workflow-contents-read` fires on `permissions: {}`** — flask uses the
  *stricter* empty scope; the bundle should treat empty as satisfying.
- **`compliance/apache-2` source-header pattern still too strict** for some
  prometheus headers — a residual of the v0.12 ASF over-fire fix; collect more
  cases before re-touching.
- **`final_newline` should pair with a "no trailing blank line at EOF" option**
  (exactly-one-trailing-newline) — curl.
- **node bundle `node_modules`/`dist` checks want a default `tests/fixtures`
  exclude** — eslint.

### Stage-B value (the adversarial pass earned its keep)

Every repo's Stage B added findings — 4 of 5 changed the counts (+1 A on
prometheus/curl/eslint, +1 B on rubocop) and 12 concrete misses surfaced, e.g.
flask `.editorconfig`, prometheus Go-toolchain-version coherence (`.promu.yml`)
+ Dockerfile variant-label uniqueness, rubocop `.rubocop.yml inherit_from` edge,
curl per-file COPYRIGHT presence + `CURL_DISABLE_*` feature-gate set sync +
typecheck-enum ↔ `libcurl-errors.3` set equality, eslint rule→registry membership
parity. flask's Stage A was strong enough that Stage B moved nothing.

### Artifacts

5 validated draft configs at `/tmp/cs_out/<owner>-<repo>.alint.yml` (flask 13K,
prometheus 7K, rubocop 15K, curl 5.8K, eslint 5.6K). Pending the corpus-storage
decision before integration into the repo.

---

## Batch 2 — 2026-06-01 (scale-up 10: adds PHP / .NET / Elixir / Rust)

Workflow: 20 agents, ~2M tokens. *(The first attempt failed transiently — all 10
agents, including 222-file ripgrep, died without emitting output during the
session-exit window. Clean re-run after two safeguards: agents write `alint check`
JSON to a file and inspect only the summary/failing rules, and both stages are
told they MUST end with the `StructuredOutput` call.)*

| repo | shape | A | B | C | D | cov | file-graph | Stage B |
|---|---|--:|--:|--:|--:|--:|--:|---|
| `pydantic/pydantic` | Python lib | 5 | 0 | 0 | 17 | **100%** | 0 | no change |
| `vitejs/vite` | JS build tool | 1 | 0 | 0 | 10 | **100%** | 0 | −1 D |
| `symfony/symfony` | PHP web-fw mono | 8 | 3 | 1 | 11 | 67% | 3 | no change (HQ) |
| `hashicorp/terraform` | Go infra/ops | 2 | 0 | 1 | 20 | 67% | 3 | no change |
| `phoenixframework/phoenix` | Elixir web-fw | 2 | 0 | 1 | 7 | 67% | 0 | −1 A |
| `dotnet/aspnetcore` | .NET web-fw mono | 2 | 1 | 1 | 11 | 50% | 3 | +1 A |
| `django/django` | Python web-fw | 3 | 1 | 2 | 17 | 50% | 0 | B→C |
| `spring-projects/spring-boot` | JVM web-fw mono | 2 | 3 | 3 | 14 | 25% | 3 | −2 A (kind-misuse) |
| `redis/redis` | C systems/DB | 0 | 1 | 0 | 13 | 0% | 1 | no change |
| `BurntSushi/ripgrep` | Rust CLI | 0 | 1 | 0 | 7 | 0% | 0 | C→B |

Pooled **25/44 = 57%** (vs batch 1's 74%) — deliberately heavy on large
frameworks/monorepos (django, spring-boot, symfony, aspnetcore), a systems DB
(redis), and a solo CLI (ripgrep), where alint's addressable share is smaller:
more codegen-freshness / set-membership / commit-message gaps and a higher D
(execution) floor. Focused libraries still hit 100% (pydantic, vite).
**Cumulative across 16 repos: 61/93 = 66%.**

### `file_dependency_graph` keeps climbing — now 34 sources across 16 repos

Batch 2 added 13 edge sources, broadening the *types*: **codegen-freshness
`git diff --exit-code`** (redis `commands.def` from 442 JSON specs; terraform ×3;
aspnetcore ×3 in `eng/scripts/CodeCheck.ps1`) and **registry / manifest-
declaration** edges (spring-boot ×3 buildSrc registry→code; symfony ×3
`composer.json` `replace`-map → sub-packages). The gate is decisively validated;
the edge-source variety (content-regex, naming-convention, manifest, generated-
diff) keeps arguing for a *generic* kind over per-ecosystem rules.

### New-kind candidates — several VALIDATE already-planned v0.12 workstreams

- **`git_commit_subject_matches` / commit-message regex** — recurs in django +
  spring-boot. Fresh corpus evidence for the planned
  [`git_commit_subject_matches.md`](./git_commit_subject_matches.md) (Django's
  period-suffix + `[A.B.x]` stable-branch prefix; Spring's DCO `Signed-off-by`).
- **`value_set_membership` / cross-file subset** — aspnetcore. Validates
  [`value_set_membership.md`](./value_set_membership.md).
- **`files_equal` (whole-file byte-identity)** — recurs (tokio B1 + symfony).
  Demand accumulating; promote from a tokio singleton to a tracked candidate.
- **`generated_file_fresh` mutating / regen mode** — redis + symfony want a
  codegen-freshness variant whose generator writes in place (today gff is
  stdout-only). Extends an existing kind.
- **`registry_paths_resolve` extensions** — to code symbols (spring-boot
  `.imports`/`.factories`) and `.slnx`/`.slnf` project paths (aspnetcore).
- Singletons: `ordered_values_in_key` (spring-boot), `unique_filename` /
  no-duplicate-basename (aspnetcore), `manifest_implies_content` (symfony).

### alint sharp-edges (C-tuning) — recurring themes firming up

The **ref-pin-vs-SHA-pin** preset (django, recurring from flask) and the
**fixture-exclude** preset (django) now have multiple confirmations. New:
`file_content_forbidden` allowlist + per-subpackage `import_gate` layering preset
(spring-boot), a `php-symfony` header bundle (symfony), per-directory SPDX
value-in-allowlist (terraform), within-file version-pin sync (phoenix).

### Stage-B value

4 of 10 records had counts corrected (spring-boot −2 A as kind-misuses,
ripgrep/django re-bucketed, aspnetcore +1 A); 10 concrete misses surfaced incl.
symfony's `splitsh.json` 183-entry subtree registry, spring-boot's
`CheckSpringConfigurationMetadata` family, aspnetcore's shared-framework
reference-boundary firewall, vite's `patchedDependencies` path resolution.

### Artifacts

10 validated draft configs at `/tmp/cs_out/` (all `config_validated: true`).
