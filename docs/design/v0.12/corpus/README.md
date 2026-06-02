# v0.12 case-study corpus — draft configs

Agent-drafted `.alint.yml` configs produced by the
[100-repo case study](../case_study_100_repos.md), one per repo. Each was
written by a Stage-A analyst agent against a depth-1 clone pinned to the SHA in
[`case_study_repos.md`](../case_study_repos.md), and **validated with
`alint check` against that clone** (every `config_validated: true`) — see the
per-batch findings and coverage metrics in
[`case_study_log.md`](../case_study_log.md).

These are **research artifacts, not shipped examples.** They express each repo's
*addressable* (A-bucket) enforced-validation surface to ground the coverage
numbers and the new-kind / tuning backlog. The polished, audited example configs
live under `examples/` (the calibration repo, `examples/tokio-rs-tokio/`, is the
reference). This directory is `ignore:`d by the repo's own `.alint.yml` — these
are other repos' configs, so dogfooding alint over them is meaningless.

The configs are pinned to the study's clone SHAs; re-running them targets those
trees, not current upstream.

## Contents

- **Batch 1:** `pallets-flask`, `prometheus-prometheus`, `rubocop-rubocop`,
  `curl-curl`, `eslint-eslint`.
- **Batch 2:** `django-django`, `pydantic-pydantic`,
  `spring-projects-spring-boot`, `symfony-symfony`, `redis-redis`,
  `hashicorp-terraform`, `vitejs-vite`, `BurntSushi-ripgrep`,
  `dotnet-aspnetcore`, `phoenixframework-phoenix`.
- **Batch 3:** `rails-rails`, `laravel-framework`, `gradle-gradle`,
  `JetBrains-kotlin`, `postgres-postgres`, `openssl-openssl`,
  `pandas-dev-pandas`, `numpy-numpy`, `swiftlang-swift`, `jgm-pandoc`,
  `gohugoio-hugo`, `grafana-grafana`, `sveltejs-svelte`, `serde-rs-serde`,
  `ansible-ansible`, `elastic-elasticsearch`, `neovim-neovim`,
  `composer-composer`, `fastapi-fastapi`, `NixOS-nix`.
- **Batch 4:** `kubernetes-kubernetes`, `golang-go`, `rust-lang-rust`,
  `python-cpython`, `flutter-flutter`, `apache-kafka`, `apache-spark`,
  `llvm-llvm-project`, `dotnet-roslyn`, `facebook-react`, `microsoft-vscode`,
  `discourse-discourse`, `elixir-lang-elixir`, `PostgREST-postgrest`,
  `systemd-systemd`, `ClickHouse-ClickHouse`, `astral-sh-ruff`,
  `scikit-learn-scikit-learn`, `square-okhttp`, `vim-vim`.
- **Batch 5:** `apache-airflow`, `apache-arrow`, `angular-angular`, `nodejs-node`,
  `pytorch-pytorch`, `tensorflow-tensorflow`, `denoland-deno`, `bazelbuild-bazel`,
  `protocolbuffers-protobuf`, `prettier-prettier`, `helm-helm`, `istio-istio`,
  `dotnet-runtime`, `microsoft-TypeScript`, `pnpm-pnpm`, `mastodon-mastodon`,
  `NixOS-nixpkgs`, `valkey-io-valkey`, `quarkusio-quarkus`, `AvaloniaUI-Avalonia`.
- **Batch 6:** `git-git`, `google-guava`, `netty-netty`,
  `junit-team-junit-framework`, `psf-black`, `pytest-dev-pytest`, `python-mypy`,
  `python-poetry-poetry`, `sqlalchemy-sqlalchemy`, `Textualize-rich`,
  `encode-httpx`, `ruby-ruby`, `jekyll-jekyll`, `Homebrew-brew`,
  `fastlane-fastlane`, `phpstan-phpstan`, `guzzle-guzzle`, `vapor-vapor`,
  `Alamofire-Alamofire`, `signalapp-Signal-Android`.

Later batches append here as they pass their checkpoint.
