#!/usr/bin/env bash
# Relation-layer perf gate: HEAD against the frozen v0.18.2 reference
# (ADR-0023 decision V21-6, ADR-0019 DEC-13), on the deterministic scenarios
# S1-S3.
#
#   1. Instructions. The reference's release binary and HEAD's are each run by
#      HEAD's det_check harness (same generated trees, toolchain, Valgrind and
#      gungraun runner; only the alint binary differs). Any S1-S3 cell more
#      than +3% Ir over the reference fails. Nothing is committed as a
#      baseline, so neither the host nor the toolchain can drift it.
#   2. Syscalls. read/stat-family counts for the alint process (spawned
#      children excluded, see syscall-counts.py) on the 10k trees. A family
#      more than 32 calls over the reference fails: fixed per-run probes differ
#      by a few calls, a per-file regression by thousands.
#
# S4 is not compared: it extends bundled rulesets that arrived in v0.19, so the
# reference binary cannot load it. A breach exits 1; so does a gate that cannot
# run, labelled as a tooling failure. Run by ci.yml's `perf-gate-reference` job
# for relation-layer changes (detect-changes.sh `relation`).
set -uo pipefail
cd "$(git rev-parse --show-toplevel)" || exit 1

REFERENCE="${PERF_REFERENCE:-v0.18.2}"
IR_LIMIT="${PERF_REFERENCE_IR_LIMIT:-3}"
SYSCALL_ALLOWANCE="${PERF_REFERENCE_SYSCALL_ALLOWANCE:-32}"
FILTER='det_check::check_grp::*'
SCENARIOS=(s1 s2 s3)

summary() { printf '%s\n' "$@" >> "${GITHUB_STEP_SUMMARY:-/dev/null}"; }
tooling_failure() {
  echo "::error title=Reference perf gate::$1 (tooling failure, not a perf signal); the gate did not run"
  summary "### Reference perf gate (${REFERENCE}): did not run" "" "Tooling failure: $1. See the job log."
  exit 1
}

work="$(mktemp -d)"
ref_tree="$work/reference"
head_bin="$work/alint-head"
cleanup() {
  [[ -f "$head_bin" ]] && cp "$head_bin" target/release/alint 2>/dev/null
  git worktree remove --force "$ref_tree" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

GUNGRAUN_VERSION="$(awk '/^name = "gungraun"$/{f=1; next} f && /^version = /{gsub(/"/,"",$3); print $3; exit}' Cargo.lock)"
[[ -n "$GUNGRAUN_VERSION" ]] || tooling_failure "could not read the gungraun version from Cargo.lock"
for tool in valgrind strace; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    { sudo apt-get update -qq && sudo apt-get install -y -qq "$tool"; } \
      || tooling_failure "could not install $tool"
  fi
done
if [[ "$(gungraun-runner --version 2>/dev/null | awk '{print $NF}')" != "$GUNGRAUN_VERSION" ]]; then
  cargo install gungraun-runner --version "$GUNGRAUN_VERSION" --locked \
    || tooling_failure "could not install gungraun-runner v${GUNGRAUN_VERSION}"
fi

echo "==> [1/4] build the ${REFERENCE} release binary"
git rev-parse --verify -q "${REFERENCE}^{commit}" >/dev/null \
  || tooling_failure "${REFERENCE} is not in this checkout (fetch tags)"
git worktree add -q --detach "$ref_tree" "${REFERENCE}^{commit}" \
  || tooling_failure "could not check out ${REFERENCE}"
(cd "$ref_tree" && cargo build --release -p alint --locked) \
  || tooling_failure "the ${REFERENCE} release build failed"
ref_bin="$ref_tree/target/release/alint"

echo "==> [2/4] build HEAD's release binary"
cargo build --release -p alint --locked || tooling_failure "HEAD's release build failed"
cp target/release/alint "$head_bin"

echo "==> [3/4] instructions: ${REFERENCE} then HEAD through HEAD's det_check"
cp "$ref_bin" target/release/alint
cargo bench -p alint-bench --bench det_check --locked -- --save-baseline=reference "$FILTER" \
  || tooling_failure "the det_check run of the ${REFERENCE} binary failed"
cp "$head_bin" target/release/alint
bench_log="$work/bench.log"
cargo bench -p alint-bench --bench det_check --locked -- --baseline=reference \
  --callgrind-limits="ir=${IR_LIMIT}%" "$FILTER" 2>&1 | tee "$bench_log"
bench_rc="${PIPESTATUS[0]}"
grep -q 'Gungraun result:' "$bench_log" || tooling_failure "the HEAD det_check run did not complete"

ir_rows="$(sed 's/\x1b\[[0-9;]*m//g' "$bench_log" | awk '
  /^det_check::/ { split($2, id, ":"); name = id[1]; next }
  name != "" && $1 == "Instructions:" {
    split($2, v, "|"); delta = $3; gsub(/[()]/, "", delta)
    printf "| `%s` | %s | %s | %s |\n", name, v[2], v[1], delta; name = ""
  }')"
[[ -n "$ir_rows" ]] || tooling_failure "no S1-S3 instruction counts in the det_check output"

echo "==> [4/4] syscalls on the 10k trees"
trace_set="$(python3 ci/scripts/syscall-counts.py --syscalls)"
syscall_rows=""
syscall_breach=false
for scenario in "${SCENARIOS[@]}"; do
  tree="target/det-trees/${scenario}-10000"
  [[ -f "$tree/.alint.yml" ]] || tooling_failure "det_check left no tree at $tree"
  for side in reference head; do
    bin="$ref_bin"
    [[ "$side" == head ]] && bin="$head_bin"
    (cd "$tree" && strace -f -qq -o "$work/$scenario-$side.trace" -e trace="$trace_set" \
      "$bin" check --format json --config .alint.yml >/dev/null 2>&1)
    python3 ci/scripts/syscall-counts.py "$work/$scenario-$side.trace" > "$work/$scenario-$side.counts" \
      || tooling_failure "could not count syscalls for $scenario ($side)"
  done
  while read -r family ref_count; do
    head_count="$(awk -v f="$family" '$1 == f {print $2}' "$work/$scenario-head.counts")"
    extra=$((head_count - ref_count))
    mark=""
    if (( extra > SYSCALL_ALLOWANCE )); then
      mark=" (over)"
      syscall_breach=true
    fi
    syscall_rows+="| ${scenario}_10k | ${family} | ${ref_count} | ${head_count} | ${extra}${mark} |"$'\n'
  done < "$work/$scenario-reference.counts"
done

verdict="pass"
[[ "$bench_rc" -ne 0 ]] && verdict="instruction regression"
if [[ "$syscall_breach" == true ]]; then
  [[ "$verdict" == pass ]] && verdict="syscall regression" || verdict="$verdict and syscall regression"
fi
summary "### Reference perf gate (${REFERENCE}): ${verdict}" "" \
  "S1-S3, HEAD against the ${REFERENCE} binary, both measured by HEAD's harness." "" \
  "| Bench | ${REFERENCE} | HEAD | Delta (limit +${IR_LIMIT}%) |" "|---|---:|---:|---:|" "$ir_rows" "" \
  "| Tree | Family | ${REFERENCE} | HEAD | Extra (allowance ${SYSCALL_ALLOWANCE}) |" "|---|---|---:|---:|---:|" \
  "$syscall_rows"
printf '%s\n' "$syscall_rows"
if [[ "$verdict" != pass ]]; then
  echo "::error title=Reference perf gate::${verdict} against ${REFERENCE} (Ir limit +${IR_LIMIT}%, syscall allowance ${SYSCALL_ALLOWANCE})"
  exit 1
fi
echo "==> reference perf gate: PASS"
