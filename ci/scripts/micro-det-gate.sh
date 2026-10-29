#!/usr/bin/env bash
# Deterministic micro-bench gate, per-PR / per-push tier (advisory).
#
# `xtask micro-det` compiles the curated low-noise subset of the criterion micro
# suite at HEAD and at the base (perf-gate-base.sh, the same base the
# det-perf-gate.sh uses), runs each benchmark under Callgrind and compares
# instructions per iteration. Load-immune, so it runs on any executor. A
# regression past +3% annotates the run but never fails it; the full suite
# gates releases in bench-record.yml. A gate that could not run fails the job,
# labelled as tooling, so a blind gate never looks like a pass.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)" || exit 1

summary() { printf '%s\n' "$@" >> "${GITHUB_STEP_SUMMARY:-/dev/null}"; }
tooling_failure() {
  echo "::error title=Deterministic micro gate::$1 (tooling failure, not a perf signal); the gate did not run"
  summary "### Deterministic micro benches: did not run" "" "Tooling failure: $1. See the job log."
  exit 1
}

# shellcheck source=ci/scripts/perf-gate-base.sh
source "$(dirname "$0")/perf-gate-base.sh"
BASE_SHA="$(perf_gate_base)"
HEAD_SHA="$(git rev-parse HEAD)"
if [[ -z "$BASE_SHA" || "$BASE_SHA" == "$HEAD_SHA" ]]; then
  echo "==> no distinct base to compare against — skipping"
  exit 0
fi

if ! command -v valgrind >/dev/null 2>&1; then
  if ! { sudo apt-get update -qq && sudo apt-get install -y -qq valgrind; }; then
    tooling_failure "could not install valgrind"
  fi
fi

rows="$(mktemp)"
trap 'rm -f "$rows"' EXIT
workdir="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/micro-det"
# Reuse the job's cached target dir for dependencies (micro-det cleans only the
# workspace's own packages between refs).
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
echo "==> micro-det: ${BASE_SHA:0:12} -> ${HEAD_SHA:0:12} (pr subset)"
if ! cargo run -q -p xtask -- micro-det \
  --base "$BASE_SHA" --head "$HEAD_SHA" --suite pr --advisory \
  --workdir "$workdir" --out-json "$rows" \
  --summary "${GITHUB_STEP_SUMMARY:-/dev/null}"; then
  tooling_failure "xtask micro-det did not complete"
fi
regressions="$(grep -cE '"verdict": "(regression|anchor_regression)"' "$rows" || true)"
if [[ "$regressions" -gt 0 ]]; then
  echo "::warning title=Deterministic micro gate::${regressions} micro bench(es) grew more than +3% in instructions (ADVISORY). See the job summary."
fi
echo "==> deterministic micro gate: done (${regressions} advisory regression(s))"
