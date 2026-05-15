#!/usr/bin/env bash
# Run every shell-script test harness under ci/scripts/. Each
# harness is a `test-*.sh` that exits 0 on success / non-zero on
# failure. New harnesses just need to follow the `test-*.sh`
# naming convention to be picked up here.
#
# Split out from `ci/scripts/test.sh` so a shell-test failure
# (e.g. a homebrew-formula golden drift) doesn't fail the Rust
# `Test` CI job, which otherwise looks like a Rust regression in
# the workflow summary.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

failed=0
checked=0
for harness in ci/scripts/test-*.sh; do
  [[ -f "$harness" ]] || continue
  echo "==> Running $harness"
  if "$harness"; then
    checked=$((checked + 1))
  else
    echo "[shell-tests] FAILED: $harness" >&2
    failed=$((failed + 1))
  fi
done

total=$((checked + failed))
if [[ "$failed" -ne 0 ]]; then
  echo "[shell-tests] $failed of $total harnesses failed" >&2
  exit 1
fi

if [[ "$total" -eq 0 ]]; then
  echo "[shell-tests] no test-*.sh harnesses found under ci/scripts/" >&2
  exit 0
fi

echo "[shell-tests] OK — $checked harness(es) passed"
