# v0.12 — 100+ repo case study: the pinned corpus (Step 0)

Status: **Approved 2026-06-01 — the committed Step-0 frame.** The Step-0
deliverable of [`case_study_100_repos.md`](./case_study_100_repos.md):
the stratified, pinned frame the study runs against. The selection is
locked; **no repo is cloned or linted until the study itself is approved.**

## What this is

**111 repositories** — the existing 30-repo baseline re-pinned, plus 81
additions chosen to fill the strata the 30-repo corpus was thin on
(Python, JVM, Ruby, PHP, systems/C-C++, and mid-size single-language
libraries). Every repo is pinned to its default-branch HEAD commit as of
the pin date, so configs and coverage numbers stay reproducible against a
fixed tree — the 30-repo pass cloned moving upstream with no pin, which
this fixes.

## Provenance + method

- **Pin date: 2026-06-01.** Each row's `Pinned HEAD` is the default-branch
  HEAD commit SHA at that date, resolved **read-only** via the GitHub
  GraphQL API (no clones). Stage A clones target this SHA.
- **Evidence-backed, not asserted.** All 111 were resolved against the API
  and screened on the four bars this list is curated for:
  - **Active** — every repo's last push is within the ~3 months before the
    pin date (**0 stale**).
  - **Popular / important** — **46 repos ≥ 50k stars; all ≥ ~7k** (floor:
    `junit-team/junit-framework`); 97 of 111 are ≥ 10k.
  - **High-quality / live** — **0 archived, 0 forks.** The 18 repos whose
    license reads `NONE`/`NOASSERTION` are flagship projects on *custom*
    licenses GitHub's detector can't map to an SPDX id (PostgreSQL License,
    the curl license, PSF, the LLVM exception, Ruby's license, …) —
    legitimate, and itself a licensing-surface data-point for the study,
    not a defect.
- **Renames canonicalised at pin time:** `apple/swift` →
  `swiftlang/swift`, `junit-team/junit5` → `junit-team/junit-framework`,
  `vercel/turbo` → `vercel/turborepo` (the last a baseline repo whose
  `examples/` directory name is now stale).

## Diversity (the selection's whole point)

The baseline-30 skewed to big-tech JS/TS + Rust monorepos and carried
**zero** Ruby/PHP/Swift/Elixir/Haskell and few Python/JVM/systems repos.
The 81 additions re-balance toward those high-alint-fit strata. Ecosystem
split, baseline (B) vs added (N):

| Ecosystem | Baseline | Added | Total |
|---|--:|--:|--:|
| Python | 4 | 15 | **19** |
| JS/TS | 8 | 7 | **15** |
| C/C++ | 2 | 13 | **15** |
| JVM | 2 | 11 | **13** |
| Go | 4 | 9 | **13** |
| Rust | 7 | 3 | **10** |
| Ruby | 0 | 8 | **8** |
| PHP | 0 | 5 | **5** |
| .NET | 1 | 3 | **4** |
| Swift | 0 | 3 | **3** |
| Haskell | 0 | 2 | **2** |
| Elixir | 0 | 2 | **2** |
| Nix | 1 | 0 | **1** |
| Dart | 1 | 0 | **1** |

**Size tier:** 18 hyperscale-monorepo · 65 large · **28 mid-size
single-language lib** (deliberately over-weighted — highest alint fit,
lowest 30-repo representation).
**Governance:** 39 community · 24 single-vendor · 20 foundation · 8 Google
· 5 Microsoft · 5 CNCF · 4 ASF · 4 solo-maintainer · 2 Meta.
**Domain:** 20 web-fw · 19 lib/SDK · 19 CLI · 10 systems/DB · 10
lang/compiler · 8 infra/ops · 8 data/ML · 5 lang/build · 3 each
runtime / editor / docs-site / app.
**Build system:** 26 distinct — go, cargo, gradle, cmake, make, bundler,
tox, pnpm, npm, meson, composer, yarn, maven, dotnet, bazel, autotools,
swiftpm, poetry, nox, mix, hatch, cabal, nix, gn, breeze, bootstrap.

## The corpus

`T` = tier (B re-pinned baseline / N new). `size`/`build`/`gov`/`domain`
are **pre-clone editorial estimates** — Stage A records each repo's actual
validation surface; treat them as the stratification frame, not ground
truth. Sorted by ecosystem, then tier, then name.

| # | Repo | T | Eco | Size | Build | Gov | Domain | Stars | Pushed | License | Pinned HEAD |
|--:|---|:-:|---|---|---|---|---|--:|---|---|---|
| 1 | `apache/arrow` | B | C/C++ | large | cmake | ASF | data/ML | 16805 | 2026-06-01 | Apache-2.0 | `e2b44378ed` |
| 2 | `protocolbuffers/protobuf` | B | C/C++ | mono | bazel/cmake | Google | lib/SDK | 71298 | 2026-06-01 | NOASSERTION | `5e7e16f032` |
| 3 | `ClickHouse/ClickHouse` | N | C/C++ | large | cmake | vendor | systems/DB | 47737 | 2026-06-01 | Apache-2.0 | `044a9bb554` |
| 4 | `curl/curl` | N | C/C++ | large | autotools/cmake | community | CLI | 42034 | 2026-06-01 | NOASSERTION | `ba600296d2` |
| 5 | `duckdb/duckdb` | N | C/C++ | large | cmake | vendor | systems/DB | 38544 | 2026-06-01 | MIT | `0a8a19486d` |
| 6 | `git/git` | N | C/C++ | large | make | community | CLI | 61246 | 2026-05-31 | NOASSERTION | `1666c12652` |
| 7 | `llvm/llvm-project` | N | C/C++ | mono | cmake | foundation | lang/compiler | 38594 | 2026-06-01 | NOASSERTION | `d7ee01c0fa` |
| 8 | `neovim/neovim` | N | C/C++ | large | cmake | community | editor | 100060 | 2026-06-01 | NOASSERTION | `fb5aad1d07` |
| 9 | `NixOS/nix` | N | C/C++ | large | meson/nix | foundation | lang/build | 16992 | 2026-06-01 | LGPL-2.1 | `d212b92f1e` |
| 10 | `openssl/openssl` | N | C/C++ | large | make/perl | foundation | lib/SDK | 30246 | 2026-06-01 | Apache-2.0 | `0a396bdd1c` |
| 11 | `postgres/postgres` | N | C/C++ | large | make/meson | community | systems/DB | 21058 | 2026-06-01 | NOASSERTION | `4b0bf0788b` |
| 12 | `redis/redis` | N | C/C++ | large | make | vendor | systems/DB | 74647 | 2026-06-01 | NOASSERTION | `230c651c89` |
| 13 | `systemd/systemd` | N | C/C++ | large | meson | vendor | systems/DB | 16344 | 2026-06-01 | GPL-2.0 | `414fc6e3b2` |
| 14 | `valkey-io/valkey` | N | C/C++ | large | cmake/make | foundation | systems/DB | 25983 | 2026-06-01 | NOASSERTION | `c15ecf1dd8` |
| 15 | `vim/vim` | N | C/C++ | large | autotools/make | community | editor | 40428 | 2026-06-01 | Vim | `fd30a736cc` |
| 16 | `flutter/flutter` | B | Dart | mono | gn/custom | Google | web-fw | 176607 | 2026-06-01 | BSD-3-Clause | `2fc403d857` |
| 17 | `elixir-lang/elixir` | N | Elixir | large | mix/make | community | lang/compiler | 26441 | 2026-06-01 | Apache-2.0 | `55da97b9a3` |
| 18 | `phoenixframework/phoenix` | N | Elixir | large | mix | community | web-fw | 22995 | 2026-06-01 | MIT | `e1e7912418` |
| 19 | `golang/go` | B | Go | large | make/go | Google | lang/compiler | 134173 | 2026-06-01 | BSD-3-Clause | `33001a6225` |
| 20 | `helm/helm` | B | Go | large | go/make | CNCF | infra/ops | 29833 | 2026-05-30 | Apache-2.0 | `59b57c5c31` |
| 21 | `istio/istio` | B | Go | large | go/make | CNCF | infra/ops | 38202 | 2026-06-01 | Apache-2.0 | `5112958faa` |
| 22 | `kubernetes/kubernetes` | B | Go | mono | bazel/make | CNCF | infra/ops | 122597 | 2026-06-01 | Apache-2.0 | `14f9f7e9e5` |
| 23 | `cli/cli` | N | Go | large | go | vendor | CLI | 44675 | 2026-06-01 | MIT | `e6fa2faf9e` |
| 24 | `etcd-io/etcd` | N | Go | large | go | CNCF | systems/DB | 51756 | 2026-06-01 | Apache-2.0 | `9ed485d3e3` |
| 25 | `gin-gonic/gin` | N | Go | mid | go | community | web-fw | 88583 | 2026-05-09 | MIT | `5f4f964325` |
| 26 | `gohugoio/hugo` | N | Go | large | go | community | docs/site | 88338 | 2026-06-01 | Apache-2.0 | `b01ecd4cd4` |
| 27 | `grafana/grafana` | N | Go | large | go/make | vendor | infra/ops | 74077 | 2026-06-01 | AGPL-3.0 | `a54b2e1e9a` |
| 28 | `hashicorp/terraform` | N | Go | large | go | vendor | infra/ops | 48509 | 2026-06-01 | NOASSERTION | `581ffab924` |
| 29 | `junegunn/fzf` | N | Go | mid | go/make | solo | CLI | 80678 | 2026-05-31 | MIT | `7d647c70c2` |
| 30 | `prometheus/prometheus` | N | Go | large | go/make | CNCF | infra/ops | 64230 | 2026-06-01 | Apache-2.0 | `c0b4c5ef18` |
| 31 | `spf13/cobra` | N | Go | mid | go | community | lib/SDK | 44042 | 2026-04-25 | Apache-2.0 | `ad460ea8f2` |
| 32 | `jgm/pandoc` | N | Haskell | large | cabal/stack | solo | CLI | 44497 | 2026-06-01 | GPL-2.0 | `9038439481` |
| 33 | `PostgREST/postgrest` | N | Haskell | mid | cabal/nix | community | systems/DB | 27191 | 2026-06-01 | MIT | `1d6e0bd35f` |
| 34 | `angular/angular` | B | JS/TS | mono | npm/nx | Google | web-fw | 100133 | 2026-06-01 | MIT | `3cc9e2b7a9` |
| 35 | `facebook/react` | B | JS/TS | large | yarn | Meta | web-fw | 245367 | 2026-06-01 | MIT | `557e28fae7` |
| 36 | `microsoft/TypeScript` | B | JS/TS | large | npm/gulp | MS | lang/compiler | 109046 | 2026-06-01 | Apache-2.0 | `f3d3968058` |
| 37 | `microsoft/vscode` | B | JS/TS | large | yarn | MS | editor | 185652 | 2026-06-01 | MIT | `07cee71da6` |
| 38 | `nodejs/node` | B | JS/TS | mono | make/gyp | foundation | lang/runtime | 117453 | 2026-06-01 | NOASSERTION | `7af433b68e` |
| 39 | `pnpm/pnpm` | B | JS/TS | large | pnpm | vendor | CLI | 35318 | 2026-06-01 | MIT | `ae6e07705d` |
| 40 | `prettier/prettier` | B | JS/TS | large | yarn | foundation | CLI | 51888 | 2026-06-01 | MIT | `a5e7b7c3bd` |
| 41 | `vercel/next.js` | B | JS/TS | mono | pnpm/cargo | vendor | web-fw | 139632 | 2026-06-01 | MIT | `bc384860d5` |
| 42 | `axios/axios` | N | JS/TS | mid | npm | community | lib/SDK | 109086 | 2026-06-01 | MIT | `4306df21e8` |
| 43 | `eslint/eslint` | N | JS/TS | large | npm | foundation | CLI | 27263 | 2026-06-01 | MIT | `f99b47a679` |
| 44 | `expressjs/express` | N | JS/TS | mid | npm | foundation | web-fw | 69070 | 2026-05-17 | MIT | `dae209ae65` |
| 45 | `facebook/docusaurus` | N | JS/TS | large | yarn | Meta | docs/site | 65067 | 2026-06-01 | MIT | `183fc6f1e3` |
| 46 | `sveltejs/svelte` | N | JS/TS | large | pnpm | community | web-fw | 86671 | 2026-06-01 | MIT | `5b8db1be35` |
| 47 | `vitejs/vite` | N | JS/TS | large | pnpm | foundation | lang/build | 80913 | 2026-06-01 | MIT | `f94df87ff0` |
| 48 | `withastro/astro` | N | JS/TS | large | pnpm | community | web-fw | 59733 | 2026-06-01 | NOASSERTION | `fa7a26410e` |
| 49 | `apache/spark` | B | JVM | large | maven/sbt | ASF | data/ML | 43374 | 2026-06-01 | Apache-2.0 | `27187d6cba` |
| 50 | `bazelbuild/bazel` | B | JVM | mono | bazel | Google | lang/build | 25450 | 2026-06-01 | Apache-2.0 | `bda3d1c2b0` |
| 51 | `apache/kafka` | N | JVM | large | gradle | ASF | systems/DB | 32696 | 2026-06-01 | Apache-2.0 | `115d6d3185` |
| 52 | `elastic/elasticsearch` | N | JVM | large | gradle | vendor | systems/DB | 76784 | 2026-06-01 | NOASSERTION | `79aea6d637` |
| 53 | `google/guava` | N | JVM | mid | maven | Google | lib/SDK | 51480 | 2026-06-01 | Apache-2.0 | `a8e9533b49` |
| 54 | `gradle/gradle` | N | JVM | large | gradle | foundation | lang/build | 18586 | 2026-06-01 | Apache-2.0 | `b7f4312ca8` |
| 55 | `JetBrains/kotlin` | N | JVM | mono | gradle | vendor | lang/compiler | 52783 | 2026-06-01 | NONE | `155ca706b5` |
| 56 | `junit-team/junit-framework` | N | JVM | mid | gradle | community | lib/SDK | 7028 | 2026-06-01 | EPL-2.0 | `9a7a266fdd` |
| 57 | `netty/netty` | N | JVM | large | maven | community | lib/SDK | 34969 | 2026-06-01 | Apache-2.0 | `e067b6e337` |
| 58 | `quarkusio/quarkus` | N | JVM | large | maven | vendor | web-fw | 15697 | 2026-06-01 | Apache-2.0 | `c8c6babb39` |
| 59 | `signalapp/Signal-Android` | N | JVM | large | gradle | vendor | app | 28868 | 2026-06-01 | AGPL-3.0 | `ae2477356b` |
| 60 | `spring-projects/spring-boot` | N | JVM | large | gradle | vendor | web-fw | 80736 | 2026-05-31 | Apache-2.0 | `bcecf922c6` |
| 61 | `square/okhttp` | N | JVM | mid | gradle | vendor | lib/SDK | 46970 | 2026-05-30 | Apache-2.0 | `0c5a45b117` |
| 62 | `dotnet/runtime` | B | .NET | mono | dotnet/cmake | MS | lang/runtime | 17918 | 2026-06-01 | MIT | `0dfd787f3c` |
| 63 | `AvaloniaUI/Avalonia` | N | .NET | large | dotnet | community | lib/SDK | 30898 | 2026-06-01 | MIT | `5ad8b75427` |
| 64 | `dotnet/aspnetcore` | N | .NET | large | dotnet | MS | web-fw | 37953 | 2026-06-01 | MIT | `423a8c2d50` |
| 65 | `dotnet/roslyn` | N | .NET | large | dotnet | MS | lang/compiler | 20451 | 2026-06-01 | MIT | `4dad8409c1` |
| 66 | `NixOS/nixpkgs` | B | Nix | mono | nix | foundation | infra/ops | 24947 | 2026-06-01 | MIT | `d9a85a232f` |
| 67 | `composer/composer` | N | PHP | mid | composer | community | CLI | 29443 | 2026-05-29 | MIT | `7b3147f083` |
| 68 | `guzzle/guzzle` | N | PHP | mid | composer | community | lib/SDK | 23448 | 2026-06-01 | MIT | `3137dc55ae` |
| 69 | `laravel/framework` | N | PHP | large | composer | community | web-fw | 34740 | 2026-06-01 | MIT | `42b494e1ed` |
| 70 | `phpstan/phpstan` | N | PHP | mid | composer | solo | CLI | 13979 | 2026-06-01 | MIT | `e336b2b9dd` |
| 71 | `symfony/symfony` | N | PHP | mono | composer | community | web-fw | 31058 | 2026-05-31 | MIT | `9f30c2bb7f` |
| 72 | `apache/airflow` | B | Python | large | breeze/nox | ASF | data/ML | 45656 | 2026-06-01 | Apache-2.0 | `33363d5418` |
| 73 | `python/cpython` | B | Python | large | make/autoconf | foundation | lang/compiler | 72940 | 2026-06-01 | NOASSERTION | `ce073ec4cc` |
| 74 | `pytorch/pytorch` | B | Python | mono | cmake/setuptools | foundation | data/ML | 100319 | 2026-06-01 | NOASSERTION | `7d2d4404be` |
| 75 | `tensorflow/tensorflow` | B | Python | mono | bazel | Google | data/ML | 195356 | 2026-06-01 | Apache-2.0 | `803e4ccb97` |
| 76 | `ansible/ansible` | N | Python | large | nox | vendor | infra/ops | 68739 | 2026-05-29 | GPL-3.0 | `1d398ae8af` |
| 77 | `django/django` | N | Python | large | tox | foundation | web-fw | 87611 | 2026-06-01 | BSD-3-Clause | `9383fae0d5` |
| 78 | `encode/httpx` | N | Python | mid | nox/hatch | community | lib/SDK | 15279 | 2026-03-29 | BSD-3-Clause | `b5addb64f0` |
| 79 | `fastapi/fastapi` | N | Python | large | hatch | community | web-fw | 98757 | 2026-06-01 | MIT | `3d2aace42f` |
| 80 | `numpy/numpy` | N | Python | large | meson | foundation | data/ML | 32129 | 2026-06-01 | NOASSERTION | `55460a7e71` |
| 81 | `pallets/flask` | N | Python | mid | tox/hatch | community | web-fw | 71598 | 2026-05-31 | BSD-3-Clause | `36e4a824f3` |
| 82 | `pandas-dev/pandas` | N | Python | large | meson | foundation | data/ML | 48886 | 2026-06-01 | BSD-3-Clause | `3146d10d9a` |
| 83 | `psf/black` | N | Python | mid | tox/hatch | foundation | CLI | 41525 | 2026-06-01 | MIT | `d246367ab4` |
| 84 | `pydantic/pydantic` | N | Python | mid | hatch | vendor | lib/SDK | 27910 | 2026-06-01 | MIT | `cf50ed2ac3` |
| 85 | `pytest-dev/pytest` | N | Python | mid | tox | community | lib/SDK | 13883 | 2026-06-01 | MIT | `0a465c8a71` |
| 86 | `python/mypy` | N | Python | large | tox | community | CLI | 20452 | 2026-06-01 | NOASSERTION | `4c8f9944ea` |
| 87 | `python-poetry/poetry` | N | Python | mid | poetry/nox | community | CLI | 34272 | 2026-06-01 | MIT | `aab3194f20` |
| 88 | `scikit-learn/scikit-learn` | N | Python | large | meson | foundation | data/ML | 66219 | 2026-06-01 | BSD-3-Clause | `65ab405b99` |
| 89 | `sqlalchemy/sqlalchemy` | N | Python | mid | tox | community | lib/SDK | 11873 | 2026-05-30 | MIT | `4fb459aaf0` |
| 90 | `Textualize/rich` | N | Python | mid | poetry | vendor | lib/SDK | 56514 | 2026-04-12 | MIT | `46cebbb032` |
| 91 | `discourse/discourse` | N | Ruby | large | bundler/rake | vendor | app | 47144 | 2026-06-01 | GPL-2.0 | `85fc647f5f` |
| 92 | `fastlane/fastlane` | N | Ruby | large | bundler | Google | CLI | 41604 | 2026-06-01 | MIT | `c30e449d26` |
| 93 | `Homebrew/brew` | N | Ruby | large | bundler/rake | community | CLI | 48253 | 2026-06-01 | BSD-2-Clause | `43039a4500` |
| 94 | `jekyll/jekyll` | N | Ruby | mid | bundler/rake | community | docs/site | 51463 | 2026-04-22 | MIT | `202df57131` |
| 95 | `mastodon/mastodon` | N | Ruby | large | bundler/yarn | community | app | 49983 | 2026-06-01 | AGPL-3.0 | `facb552c9c` |
| 96 | `rails/rails` | N | Ruby | mono | bundler/rake | foundation | web-fw | 58475 | 2026-06-01 | MIT | `6c75e6d566` |
| 97 | `rubocop/rubocop` | N | Ruby | mid | bundler/rake | community | CLI | 12876 | 2026-06-01 | MIT | `43942e2600` |
| 98 | `ruby/ruby` | N | Ruby | large | autotools/make | community | lang/compiler | 23581 | 2026-06-01 | NOASSERTION | `ad14452e3b` |
| 99 | `astral-sh/ruff` | B | Rust | large | cargo | vendor | CLI | 47764 | 2026-06-01 | MIT | `11723aeb5b` |
| 100 | `astral-sh/uv` | B | Rust | large | cargo | vendor | CLI | 85839 | 2026-06-01 | Apache-2.0 | `cfe5277bc4` |
| 101 | `clap-rs/clap` | B | Rust | mid | cargo | community | lib/SDK | 16425 | 2026-06-01 | Apache-2.0 | `8387c812c4` |
| 102 | `denoland/deno` | B | Rust | large | cargo | vendor | lang/runtime | 106929 | 2026-06-01 | MIT | `351d8fbfee` |
| 103 | `rust-lang/rust` | B | Rust | mono | bootstrap/cargo | foundation | lang/compiler | 113296 | 2026-06-01 | Apache-2.0 | `c0bb140a37` |
| 104 | `tokio-rs/tokio` | B | Rust | mid | cargo | community | lib/SDK | 32167 | 2026-05-29 | MIT | `32312ae0d6` |
| 105 | `vercel/turborepo` | B | Rust | mono | cargo/pnpm | vendor | lang/build | 30478 | 2026-06-01 | MIT | `468a9ddd2e` |
| 106 | `BurntSushi/ripgrep` | N | Rust | mid | cargo | solo | CLI | 64507 | 2026-06-01 | Unlicense | `4857d6fa67` |
| 107 | `serde-rs/serde` | N | Rust | mid | cargo | community | lib/SDK | 10599 | 2026-03-06 | Apache-2.0 | `fa7da4a935` |
| 108 | `tokio-rs/axum` | N | Rust | mid | cargo | community | web-fw | 26105 | 2026-06-01 | MIT | `4a72e063b9` |
| 109 | `Alamofire/Alamofire` | N | Swift | mid | swiftpm | community | lib/SDK | 42390 | 2026-05-25 | MIT | `7595cbcf59` |
| 110 | `swiftlang/swift` | N | Swift | mono | cmake | vendor | lang/compiler | 70019 | 2026-06-01 | Apache-2.0 | `09dfb9b25b` |
| 111 | `vapor/vapor` | N | Swift | large | swiftpm | community | web-fw | 26092 | 2026-04-20 | MIT | `712b0919a3` |

## Open items (for review before Step 0 is committed)

- **Count = 111** (protocol asks "100+"). The mid-size-lib tier (28) is the
  most compressible if a smaller corpus is wanted; conversely the thin
  tail (Nix 1, Dart 1, Elixir 2, Haskell 2) could grow.
- **Storage** (carried from the protocol's open questions): 111 configs
  in-repo under `examples/` vs a separate corpus repo — `cargo package`
  size + `examples-validate` runtime implications.
- **Pin-refresh policy:** do the committed configs freeze at these pins, or
  track upstream over time?
- **Per-stratum target counts** are not yet locked — this draft optimises
  for spread + the mid-lib over-weighting, not fixed per-cell quotas.

## Reproducing the pins

Pins were captured with a read-only GraphQL sweep (`defaultBranchRef.target.oid`
per repo) on 2026-06-01; the generator scripts live in the v0.12 working
notes. Re-running resolves *current* HEADs — to reproduce the study, clone
the SHAs in the table, not a fresh sweep.
