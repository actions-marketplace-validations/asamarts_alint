#!/usr/bin/env bash
# Validate every examples/*/.alint.yml against alint's schema +
# config parser. Catches schema regressions in pinned case-study
# configs that would otherwise only surface when a contributor
# tried to copy-paste one as a starting point, or when alint.org's
# /examples/ gallery linked to a stale config.
#
# 30 case-study configs as of v0.9.22; the loop discovers them so
# the count is self-updating.
#
# Exit codes:
#   0  every example config parses + validates
#   1  one or more configs failed validation
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

echo "==> Building release binary"
cargo build --release --locked -p alint

BIN="$REPO_ROOT/target/release/alint"

failed=0
checked=0
for cfg in examples/*/.alint.yml; do
  [[ -f "$cfg" ]] || continue
  if "$BIN" validate-config --config "$cfg" > /dev/null 2>&1; then
    checked=$((checked + 1))
  else
    # Re-run with output visible so the failure is debuggable from
    # the CI log without needing to re-run locally.
    echo "[examples-validate] FAILED: $cfg" >&2
    "$BIN" validate-config --config "$cfg" >&2 || true
    failed=$((failed + 1))
  fi
done

total=$((checked + failed))
if [[ "$failed" -ne 0 ]]; then
  echo "[examples-validate] $failed of $total example configs failed validation" >&2
  exit 1
fi

echo "[examples-validate] OK — $checked example configs validated"
