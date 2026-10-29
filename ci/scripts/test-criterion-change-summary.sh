#!/usr/bin/env bash
# Pin ci/scripts/criterion-change-summary.py, which renders bench-record's
# same-session criterion A/B for the bench PR: rows come from each
# benchmark's change/estimates.json, largest median change first, and a
# change is "significant" only when its 95% CI excludes zero.
# Expected rows contain literal Markdown backticks, not expansions.
# shellcheck disable=SC2016
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL: $1" >&2; sed 's/^/      | /' "$sandbox/out" >&2; fail=$((fail + 1)); }

# bench <dir> <full_id> <median> <lo> <hi>
bench() {
  mkdir -p "$sandbox/crit/$1/new" "$sandbox/crit/$1/change"
  printf '{"full_id": "%s"}\n' "$2" > "$sandbox/crit/$1/new/benchmark.json"
  printf '{"mean": {"point_estimate": %s, "confidence_interval": {"lower_bound": %s, "upper_bound": %s}},
 "median": {"point_estimate": %s, "confidence_interval": {"lower_bound": %s, "upper_bound": %s}}}\n' \
    "$3" "$4" "$5" "$3" "$4" "$5" > "$sandbox/crit/$1/change/estimates.json"
}
bench cross_file_pair/1000 cross_file/pair/1000 0.12 0.10 0.14
bench walker/1000 walker/1000 -0.01 -0.03 0.01
bench rule_engine/1000 rule_engine/1000 -0.05 -0.07 -0.03
mkdir -p "$sandbox/crit/new_only/new"

summary() { python3 "$REPO_ROOT/ci/scripts/criterion-change-summary.py" "$@" > "$sandbox/out"; }

echo "==> rows, order and significance"
summary "$sandbox/crit"
if grep -qF '3 benchmarks compared in one session; 2 with a significant median change' "$sandbox/out" \
  && [[ "$(grep -c '^| `' "$sandbox/out")" -eq 3 ]] \
  && head -n 5 "$sandbox/out" | tail -n 1 | grep -qF '| `cross_file/pair/1000` | +12.00% | [+10.00%, +14.00%] | significant |' \
  && grep -qF '| `walker/1000` | -1.00% | [-3.00%, +1.00%] |  |' "$sandbox/out" \
  && grep -qF '| `rule_engine/1000` | -5.00% | [-7.00%, -3.00%] | significant |' "$sandbox/out"; then
  ok "largest change first; CI straddling zero is not significant"
else
  bad "rows"
fi

echo "==> --top limits the table"
summary "$sandbox/crit" --top 1
if [[ "$(grep -c '^| `' "$sandbox/out")" -eq 1 ]] && grep -qF '2 more benchmark(s) not shown.' "$sandbox/out"; then
  ok "--top truncates with a count"
else
  bad "--top"
fi

echo "==> no comparison"
mkdir -p "$sandbox/empty"
summary "$sandbox/empty"
if grep -qF 'No same-session comparison' "$sandbox/out"; then
  ok "an empty run says so"
else
  bad "empty"
fi

echo "[criterion-change-summary] $pass passed, $fail failed"
[[ $fail -eq 0 ]]
