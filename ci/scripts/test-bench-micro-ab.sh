#!/usr/bin/env bash
# Pin the same-session micro A/B of .github/workflows/bench-record.yml: the
# `micro baseline (previous release)` step benches the previous tag into the
# shared criterion dir as baseline `prev`, then `micro cargo bench` benches the
# new ref against it.
#
# The two refs must never share a cargo target dir. Between releases they carry
# the same package versions, so cargo gives their workspace packages the same
# artifact names; in one target dir the new ref's build judges the previous
# tag's artifacts fresh and benchmarks the previous tag again, and the A/B
# reports no change (bench-record run 39882001798 did exactly that).
#
# Both steps' real run: scripts are extracted (ci/scripts/workflow_step.py) and
# run with recording stubs for git and cargo; the test asserts the target dir
# each cargo run resolves to, the criterion dir they share, the cleanup, and
# that `micro cargo bench` compares only when the baseline step says it can.
#
# Stub bodies are single-quoted on purpose: they expand when the stub runs.
# shellcheck disable=SC2016
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"
WF=.github/workflows/bench-record.yml
EXTRACT="$REPO_ROOT/ci/scripts/workflow_step.py"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL: $1" >&2; fail=$((fail + 1)); }

mkdir -p "$sandbox/bin"
stub() { printf '#!/usr/bin/env bash\n%s\n' "$2" > "$sandbox/bin/$1"; chmod +x "$sandbox/bin/$1"; }
stub sudo 'exit 0'
stub sync 'exit 0'
# `git worktree add --detach <dir> <ref>` checks out a minimal alint-bench.
stub git '
if [[ "$1 $2" == "worktree add" ]]; then
  dir="${@: -2:1}"
  mkdir -p "$dir/crates/alint-bench/benches"
  printf "[features]\nfs-benches = []\n" > "$dir/crates/alint-bench/Cargo.toml"
  : > "$dir/crates/alint-bench/benches/glob_match.rs"
  : > "$dir/crates/alint-bench/benches/det_engine.rs"
  exit 0
fi
if [[ "$1 $2 $3" == "worktree remove --force" ]]; then rm -rf "$4"; exit 0; fi
exit 1'
# cargo records the target dir it resolves to (CARGO_TARGET_DIR, else the
# workspace target/ of its cwd) and the criterion dir, and leaves a build there.
stub cargo '
target="${CARGO_TARGET_DIR:-$PWD/target}"
criterion="${CRITERION_HOME:-$target/criterion}"
mkdir -p "$target/release" "$criterion/glob_match/new"
[[ " $* " == *" --save-baseline prev "* ]] && mkdir -p "$criterion/glob_match/prev"
printf "target=%s\tcriterion=%s\targs=%s\n" "$(realpath -m "$target")" "$(realpath -m "$criterion")" "$*" >> "$STUB_LOG"'

ws="$sandbox/ws"
tmp="$sandbox/runner-temp"
mkdir -p "$ws/crates/alint-bench/benches" "$tmp"
: > "$ws/crates/alint-bench/benches/glob_match.rs"
: > "$ws/crates/alint-bench/benches/det_engine.rs"

# run_step <step name or id> [VAR=value...]: sets $rc; cargo runs go to $sandbox/log
run_step() {
  local name=$1
  shift
  python3 "$EXTRACT" "$WF" bench "$name" run > "$sandbox/step.sh"
  : > "$sandbox/gh_output"
  rc=0
  (cd "$ws" && env -u CARGO_TARGET_DIR -u CRITERION_HOME PATH="$sandbox/bin:$PATH" STUB_LOG="$sandbox/log" GITHUB_WORKSPACE="$ws" \
     RUNNER_TEMP="$tmp" GITHUB_OUTPUT="$sandbox/gh_output" GITHUB_STEP_SUMMARY=/dev/null "$@" \
     bash --noprofile --norc -eo pipefail "$sandbox/step.sh") > "$sandbox/out" 2>&1 || rc=$?
}
field() { sed -n "$1p" "$sandbox/log" | tr '\t' '\n' | sed -n "s/^$2=//p"; }
check() {
  local label=$1
  shift
  if "$@"; then ok "$label"; else
    bad "$label (exit $rc)"
    sed 's/^/      | /' "$sandbox/out" "$sandbox/log" >&2
  fi
}

# The baseline step, then the new ref's step with its compare flag.
: > "$sandbox/log"
run_step micro_prev PREV_TAG=v0.20.0
prev_rc=$rc
pruned=no
[[ -d "$ws/target/criterion/glob_match/prev" && ! -e "$ws/target/criterion/glob_match/new" ]] && pruned=yes
compare="$(sed -n 's/^compare=//p' "$sandbox/gh_output")"
run_step 'micro cargo bench' PREV_COMPARE="$compare"
head_rc=$rc

check "both steps succeed" test "$prev_rc" -eq 0 -a "$head_rc" -eq 0
check "the baseline step and the new ref each run cargo once" test "$(wc -l < "$sandbox/log")" -eq 2
prev_target="$(field 1 target)"
head_target="$(field 2 target)"
inside() { [[ "$1/" == "$2/"* ]]; }
check "the refs build in different target dirs, neither inside the other ($prev_target, $head_target)" \
  eval '[[ -n "$prev_target" && -n "$head_target" ]] && ! inside "$prev_target" "$head_target" && ! inside "$head_target" "$prev_target"'
check "the baseline lands in the new ref's criterion dir" \
  test "$(field 1 criterion)" = "$(field 2 criterion)" -a "$(field 2 criterion)" = "$(realpath -m "$ws/target/criterion")"
check "the baseline is saved as prev, without det_* benches" \
  eval '[[ "$(field 1 args)" == "bench -p alint-bench --bench glob_match --features fs-benches -- --save-baseline prev" ]]'
check "the new ref compares against prev" eval '[[ "$compare" == 1 && "$(field 2 args)" == *"-- --baseline-lenient prev" ]]'
check "only prev/ survives the baseline step" test "$pruned" = yes
check "the previous tag's worktree and target dir are removed" \
  eval '[[ -z "$(find "$tmp" -mindepth 1 -maxdepth 1)" ]] && [[ ! -e "$prev_target" ]]'

# No previous release: no baseline, no comparison.
rm -rf "$ws/target"
: > "$sandbox/log"
run_step micro_prev PREV_TAG=
check "without a previous release the baseline step runs no cargo" \
  eval '[[ "$rc" -eq 0 && ! -s "$sandbox/log" && ! -s "$sandbox/gh_output" ]]'
run_step 'micro cargo bench' PREV_COMPARE=
check "and the new ref benches without --baseline-lenient" eval '[[ "$rc" -eq 0 && "$(field 1 args)" != *baseline* ]]'

# Wiring: the compare flag comes from the baseline step's output.
want='${{ steps.micro_prev.outputs.compare }}'
got="$(python3 "$EXTRACT" "$WF" bench 'micro cargo bench' env | sed -n 's/^PREV_COMPARE\t//p')"
check "micro cargo bench reads PREV_COMPARE from steps.micro_prev" test "$got" = "$want"

echo "[bench-micro-ab] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
