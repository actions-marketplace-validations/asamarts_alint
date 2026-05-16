#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

# cargo-deny may not be on the runner. It is on the self-hosted CI
# runner, but NOT on GitHub-hosted ubuntu-latest (used by
# release.yml preflight). Install on demand, pinned with --locked,
# so the gate is portable across both. Swatinem/rust-cache persists
# ~/.cargo/bin so the install only pays once per cache window.
if ! command -v cargo-deny >/dev/null 2>&1; then
    echo "==> cargo-deny not found; installing (cargo install --locked)"
    # Clear RUSTFLAGS for the tool build only: CI sets `-D warnings`
    # for *alint's* code, but applying it while compiling a
    # third-party tool would fail the install on any upstream
    # warning under the current toolchain.
    RUSTFLAGS= cargo install cargo-deny --locked
fi

echo "==> Running cargo deny check licenses bans sources (blocking)"
# Licenses is the v0.9.22-audit gap (no deny.toml meant the license
# gate allowlisted nothing). bans (no `*` version reqs) + sources
# (crates.io only) are cheap supply-chain gates. A violation here
# fails CI and blocks a release. Policy lives in deny.toml.
cargo deny check licenses bans sources

echo "==> Running cargo deny check advisories (advisory-only)"
# Mirrors ci/scripts/audit.sh: RustSec advisories are surfaced but
# must not block the pipeline (cargo audit is the primary surface;
# this is a secondary view over the same DB). Revisit if/when a
# blocking advisory policy is adopted.
cargo deny check advisories || {
    echo "==> WARNING: cargo deny advisories found issues (see above)"
    echo "==> Advisory-only; not failing the pipeline (see audit.sh)"
}
