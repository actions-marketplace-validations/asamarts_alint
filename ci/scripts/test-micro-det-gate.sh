#!/usr/bin/env bash
# Pin the outcome handling of ci/scripts/micro-det-gate.sh:
#   - it compares HEAD against the same base as det-perf-gate.sh (a PR's base,
#     or a push's previous tip) and asks for the advisory `pr` subset;
#   - advisory regressions warn and exit 0; a clean run exits 0 quietly;
#   - an `xtask micro-det` that fails is a labelled tooling failure (exit 1),
#     never a silent pass;
#   - no distinct base is a clean skip that builds nothing.
# Behavioural: runs in a scratch git repo against a stubbed `cargo` (which
# writes the `--out-json` rows) and `valgrind`.
#
# Stub bodies are single-quoted on purpose: they expand when the stub runs.
# shellcheck disable=SC2016
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() {
  echo "  FAIL: $1" >&2
  sed 's/^/      | /' "$sandbox/out" "$sandbox/summary" "$sandbox/log" >&2 2>/dev/null || true
  fail=$((fail + 1))
}

mkdir -p "$sandbox/bin"
cat > "$sandbox/bin/cargo" <<'STUB'
#!/usr/bin/env bash
echo "cargo $*" >> "$STUB_LOG"
out=""
prev=""
for arg in "$@"; do
  [[ "$prev" == "--out-json" ]] && out="$arg"
  prev="$arg"
done
if [[ -n "$out" ]]; then
  cp "${STUB_ROWS:?}" "$out"
fi
exit "${STUB_RC:-0}"
STUB
printf '#!/usr/bin/env bash\nexit 0\n' > "$sandbox/bin/valgrind"
chmod +x "$sandbox/bin/"*

printf '[\n  {\n    "id": "rule_engine/1000",\n    "verdict": "ok"\n  }\n]\n' > "$sandbox/rows-ok.json"
printf '[\n  {\n    "id": "cross_file/pair/1000",\n    "verdict": "regression"\n  }\n]\n' \
  > "$sandbox/rows-regressed.json"

repo="$sandbox/repo"
mkdir -p "$repo/ci/scripts"
cp "$REPO_ROOT/ci/scripts/micro-det-gate.sh" "$REPO_ROOT/ci/scripts/perf-gate-base.sh" \
  "$repo/ci/scripts/"
git -C "$repo" init -q -b main
git -C "$repo" config user.email test@example.com
git -C "$repo" config user.name Test
git -C "$repo" config commit.gpgsign false
echo base > "$repo/base.txt"
git -C "$repo" add -A
git -C "$repo" commit -q -m base
base_sha=$(git -C "$repo" rev-parse HEAD)
echo pr > "$repo/pr.txt"
git -C "$repo" add -A
git -C "$repo" commit -q -m pr
head_sha=$(git -C "$repo" rev-parse HEAD)

run_gate() {
  : > "$sandbox/summary"
  : > "$sandbox/log"
  set +e
  (cd "$repo" && env PATH="$sandbox/bin:$PATH" STUB_LOG="$sandbox/log" \
    GITHUB_STEP_SUMMARY="$sandbox/summary" PR_BASE_SHA="$base_sha" \
    RUNNER_TEMP="$sandbox" STUB_ROWS="$sandbox/rows-ok.json" "$@" \
    bash ci/scripts/micro-det-gate.sh) > "$sandbox/out" 2>&1
  rc=$?
  set -e
}
has() { grep -qF -- "$2" "$sandbox/$1"; }

echo "==> clean run: exit 0, pr subset, advisory, base and head passed through"
run_gate
if [[ $rc -eq 0 ]] && has log "micro-det --base $base_sha --head $head_sha --suite pr --advisory" \
  && has log "--workdir $sandbox/micro-det" && ! has out '::warning'; then
  ok "a clean comparison passes quietly"
else
  bad "clean run (rc=$rc)"
fi

echo "==> advisory regression: exit 0 with a warning"
run_gate STUB_ROWS="$sandbox/rows-regressed.json"
if [[ $rc -eq 0 ]] && has out '::warning title=Deterministic micro gate::1 micro bench(es)'; then
  ok "a regression warns but does not fail"
else
  bad "advisory regression (rc=$rc)"
fi

echo "==> xtask fails: tooling failure, exit 1"
run_gate STUB_RC=1
if [[ $rc -eq 1 ]] && has out 'xtask micro-det did not complete (tooling failure' \
  && has summary '### Deterministic micro benches: did not run'; then
  ok "a failed micro-det run fails as tooling"
else
  bad "tooling failure (rc=$rc)"
fi

echo "==> push: base is the previous tip"
run_gate PR_BASE_SHA= PUSH_BEFORE_SHA="$base_sha"
if [[ $rc -eq 0 ]] && has log "--base $base_sha --head $head_sha"; then
  ok "a push compares against the tip before it"
else
  bad "push base (rc=$rc)"
fi

echo "==> no distinct base: clean skip"
run_gate PR_BASE_SHA="$head_sha"
if [[ $rc -eq 0 ]] && has out 'no distinct base' && [[ ! -s "$sandbox/log" ]]; then
  ok "no distinct base skips without building"
else
  bad "no distinct base (rc=$rc)"
fi

echo "[micro-det-gate] $pass passed, $fail failed"
[[ $fail -eq 0 ]]
