#!/usr/bin/env bash
# Headless Neovim smoke for the alint LSP integration.
#
# Boots Neovim 0.11+ with editors/nvim on the runtimepath (so its
# `lsp/alint.lua` config is discoverable), enables the `alint` server,
# opens a fixture file that trips a rule, and asserts an alint-sourced
# diagnostic appears (see smoke.lua).
#
# Requires: nvim 0.11+ (override the binary with $NVIM_BIN) and the
# `alint` binary on PATH. The lsp config hardcodes `cmd = {"alint","lsp"}`,
# so $ALINT_TEST_BINARY (if set) is symlinked onto PATH as `alint`.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
nvim_rtp="$(cd "$here/.." && pwd)" # editors/nvim (holds lsp/alint.lua)
nvim_bin="${NVIM_BIN:-nvim}"

bindir=""
if [[ -n "${ALINT_TEST_BINARY:-}" ]]; then
  bindir="$(mktemp -d)"
  ln -s "$ALINT_TEST_BINARY" "$bindir/alint"
  export PATH="$bindir:$PATH"
fi
command -v alint >/dev/null || {
  echo "nvim smoke FAIL: \`alint\` not on PATH (set \$ALINT_TEST_BINARY)" >&2
  exit 1
}

ws="$(mktemp -d)"
cleanup() { rm -rf "$ws" "$bindir"; }
trap cleanup EXIT

# A rule on **/*.py (python is in alint.lua's filetypes list, so the
# server attaches) and a fixture file that trips it.
cat >"$ws/.alint.yml" <<'YAML'
version: 1
rules:
  - id: no-todo
    kind: file_content_forbidden
    paths: "**/*.py"
    pattern: "TODO"
    level: error
YAML
printf 'x = 1  # TODO\n' >"$ws/bad.py"

cd "$ws"
set +e
"$nvim_bin" --clean --headless \
  --cmd "set rtp+=$nvim_rtp" \
  -c "lua vim.lsp.enable('alint')" \
  -c "edit $ws/bad.py" \
  -c "luafile $here/smoke.lua"
rc=$?
set -e
exit "$rc"
