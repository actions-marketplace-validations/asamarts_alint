#!/usr/bin/env bash
# Pin the outcome handling of ci/scripts/reference-perf-gate.sh and the child
# exclusion of ci/scripts/syscall-counts.py:
#   - a clean comparison passes and reports both tables;
#   - an S1-S3 instruction breach and a syscall family over the allowance each
#     exit 1 with a labelled verdict, and a count within the allowance passes;
#   - a gate that cannot run (missing reference tag, a det_check run with no
#     "Gungraun result:") is a labelled tooling failure, never a pass;
#   - HEAD's binary is back in target/release/alint afterwards, and the
#     reference worktree is gone;
#   - syscall-counts.py skips a spawned child's calls and resumed halves.
# Behavioural: the gate runs in a scratch git repo against stubbed `cargo`,
# `strace`, `gungraun-runner` and `valgrind`. The real Valgrind and strace runs
# are exercised by the `perf-gate-reference` CI job itself.
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
  sed 's/^/      | /' "$sandbox/out" "$sandbox/summary" >&2 2>/dev/null || true
  fail=$((fail + 1))
}

echo "==> syscall-counts.py: the traced process and its threads only"
cat > "$sandbox/trace.txt" <<'TRACE'
100 execve("/bin/alint", ["alint", "check"], 0x7ffc /* 3 vars */) = 0
100 openat(AT_FDCWD, ".alint.yml", O_RDONLY|O_CLOEXEC) = 3
100 read(3, "version: 1\n", 8192) = 11
101 newfstatat(AT_FDCWD, "a.rs", {st_mode=S_IFREG|0644, st_size=10, ...}, 0) = 0
101 read(4,  <unfinished ...>
100 statx(AT_FDCWD, "b.rs", AT_STATX_SYNC_AS_STAT, STATX_ALL, {...}) = 0
101 <... read resumed>"fn a() {}\n", 8192) = 10
100 getdents64(5, 0x5581 /* 4 entries */, 32768) = 112
102 execve("/usr/bin/git", ["git", "rev-parse"], 0x7ffc /* 3 vars */) = 0
102 read(3, "ref: refs/heads/main\n", 4096) = 21
102 openat(AT_FDCWD, ".git/HEAD", O_RDONLY) = 3
101 +++ exited with 0 +++
TRACE
counts="$(python3 "$REPO_ROOT/ci/scripts/syscall-counts.py" "$sandbox/trace.txt")"
if [[ "$counts" == $'read 2\nstat 2\nopen 1\ndents 1' ]]; then
  ok "threads count, the git child and resumed halves do not"
else
  bad "syscall counts: $counts"
fi
if python3 "$REPO_ROOT/ci/scripts/syscall-counts.py" --syscalls | grep -q 'execve' \
  && python3 "$REPO_ROOT/ci/scripts/syscall-counts.py" --syscalls | grep -q 'statx'; then
  ok "the strace set traces execve and every counted call"
else
  bad "trace set"
fi

mkdir -p "$sandbox/bin"
# cargo: `build` writes a binary that reports which tree built it; `bench`
# materializes the trees and replays the gungraun output the test chose.
cat > "$sandbox/bin/cargo" <<'STUB'
#!/usr/bin/env bash
case "$1" in
  install) exit 0 ;;
  build)
    mkdir -p target/release
    printf '#!/usr/bin/env bash\necho "%s"\n' "$PWD" > target/release/alint
    chmod +x target/release/alint
    exit 0 ;;
  bench)
    for s in s1 s2 s3; do
      mkdir -p "target/det-trees/$s-10000"
      echo 'version: 1' > "target/det-trees/$s-10000/.alint.yml"
    done
    case " $* " in
      *" --save-baseline=reference "*) exit "${STUB_REF_RC:-0}" ;;
      *" --baseline=reference "*) cat "${STUB_HEAD_OUTPUT:?}"; exit "${STUB_HEAD_RC:-0}" ;;
    esac ;;
esac
echo "unexpected cargo $*" >&2
exit 99
STUB
# strace: write a trace for whichever binary it runs; HEAD's gets
# STUB_HEAD_EXTRA_STATS more stat calls.
cat > "$sandbox/bin/strace" <<'STUB'
#!/usr/bin/env bash
out=""; bin=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -o) out="$2"; shift 2 ;;
    -e) shift 2 ;;
    -*) shift ;;
    *) bin="$1"; break ;;
  esac
done
extra=0
[[ "$bin" == */reference/* ]] || extra="${STUB_HEAD_EXTRA_STATS:-0}"
{
  echo '7 execve("alint", ["alint"], 0x1 /* 1 vars */) = 0'
  for _ in $(seq 1 $((100 + extra))); do echo '7 newfstatat(AT_FDCWD, "f", {...}, 0) = 0'; done
  for _ in $(seq 1 50); do echo '7 read(3, "x", 8192) = 1'; done
} > "$out"
STUB
cat > "$sandbox/bin/gungraun-runner" <<'STUB'
#!/usr/bin/env bash
echo "gungraun-runner 0.20.0"
STUB
printf '#!/usr/bin/env bash\nexit 0\n' > "$sandbox/bin/valgrind"
chmod +x "$sandbox/bin/"*

cat > "$sandbox/gungraun-ok.txt" <<'OUT'
det_check::check_grp::check s1_10k:(materialize("s1", S1, 10_000)) -> crates/alint-be...
  Instructions:                      396279941|431761845            (-8.21793%) [-1.08954x]
det_check::check_grp::check s3_10k:(materialize("s3", S3, 10_000)) -> crates/alint-be...
  Instructions:                    22374765762|22383599179          (-0.03946%) [-1.00039x]
Gungraun result: Ok. 2 without regressions; 0 regressed; 2 benchmarks finished in 120s
OUT
cat > "$sandbox/gungraun-regressed.txt" <<'OUT'
det_check::check_grp::check s2_10k:(materialize("s2", S2, 10_000)) -> crates/alint-be...
  Instructions:                     1947714782|1872799790           (+4.00014%) [+1.04000x]
Gungraun result: Regressed. 0 without regressions; 1 regressed; 1 benchmarks finished in 60s
OUT
printf 'error: could not compile `alint-bench`\n' > "$sandbox/bench-error.txt"

repo="$sandbox/repo"
mkdir -p "$repo/ci/scripts"
cp "$REPO_ROOT/ci/scripts/reference-perf-gate.sh" "$REPO_ROOT/ci/scripts/syscall-counts.py" "$repo/ci/scripts/"
printf 'target/\n' > "$repo/.gitignore"
git -C "$repo" init -q -b main
git -C "$repo" config user.email test@example.com
git -C "$repo" config user.name Test
git -C "$repo" config commit.gpgsign false
printf '[[package]]\nname = "gungraun"\nversion = "0.20.0"\n' > "$repo/Cargo.lock"
git -C "$repo" add -A
git -C "$repo" commit -q -m reference
git -C "$repo" tag v0.18.2
echo head > "$repo/head.txt"
git -C "$repo" add -A
git -C "$repo" commit -q -m head

run_gate() {
  : > "$sandbox/summary"
  set +e
  (cd "$repo" && env PATH="$sandbox/bin:$PATH" GITHUB_STEP_SUMMARY="$sandbox/summary" \
    STUB_HEAD_OUTPUT="$sandbox/gungraun-ok.txt" "$@" \
    bash ci/scripts/reference-perf-gate.sh) > "$sandbox/out" 2>&1
  rc=$?
  set -e
}
has() { grep -qF -- "$2" "$sandbox/$1"; }
# HEAD's binary is restored and the reference worktree removed.
clean() {
  [[ "$(bash "$repo/target/release/alint")" == "$repo" ]] \
    && [[ "$(git -C "$repo" worktree list | wc -l)" -eq 1 ]]
}

echo "==> pass: exit 0, both tables in the summary"
run_gate
if [[ $rc -eq 0 ]] && has out 'reference perf gate: PASS' \
  && has summary '### Reference perf gate (v0.18.2): pass' \
  && has summary '| `s1_10k` | 431761845 | 396279941 | -8.21793% |' \
  && has summary '| s2_10k | stat | 100 | 100 | 0 |' && clean; then
  ok "a clean comparison passes and restores HEAD's binary"
else
  bad "pass case (rc=$rc)"
fi

echo "==> instruction breach: exit 1"
run_gate STUB_HEAD_RC=3 STUB_HEAD_OUTPUT="$sandbox/gungraun-regressed.txt"
if [[ $rc -eq 1 ]] && has summary '### Reference perf gate (v0.18.2): instruction regression' \
  && has out '::error title=Reference perf gate::instruction regression' && clean; then
  ok "an S1-S3 cell over +3% fails"
else
  bad "instruction breach (rc=$rc)"
fi

echo "==> syscall growth over the allowance: exit 1; within it: pass"
run_gate STUB_HEAD_EXTRA_STATS=40
if [[ $rc -eq 1 ]] && has summary 'syscall regression' && has summary '| s1_10k | stat | 100 | 140 | 40 (over) |'; then
  ok "40 extra stat calls fail"
else
  bad "syscall breach (rc=$rc)"
fi
run_gate STUB_HEAD_EXTRA_STATS=32
if [[ $rc -eq 0 ]] && has summary '| s3_10k | stat | 100 | 132 | 32 |'; then
  ok "32 extra stat calls are within the allowance"
else
  bad "syscall allowance (rc=$rc)"
fi

echo "==> tooling failures: exit 1, labelled"
run_gate PERF_REFERENCE=v9.9.9
if [[ $rc -eq 1 ]] && has out 'v9.9.9 is not in this checkout (fetch tags) (tooling failure' \
  && has summary 'did not run'; then
  ok "a missing reference tag fails as tooling"
else
  bad "missing reference (rc=$rc)"
fi
run_gate STUB_HEAD_RC=101 STUB_HEAD_OUTPUT="$sandbox/bench-error.txt"
if [[ $rc -eq 1 ]] && has out 'the HEAD det_check run did not complete (tooling failure' && clean; then
  ok "a HEAD bench with no comparison fails as tooling and still restores HEAD's binary"
else
  bad "HEAD bench tooling failure (rc=$rc)"
fi
run_gate STUB_REF_RC=1
if [[ $rc -eq 1 ]] && has out 'det_check run of the v0.18.2 binary failed (tooling failure' && clean; then
  ok "a failed reference bench fails as tooling"
else
  bad "reference bench tooling failure (rc=$rc)"
fi

echo ""
echo "reference-perf-gate: ${pass} passed, ${fail} failed"
[[ $fail -eq 0 ]]
