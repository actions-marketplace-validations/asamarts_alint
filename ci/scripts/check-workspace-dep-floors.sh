#!/usr/bin/env bash
# Verify workspace.dependencies path-entry versions are <=
# workspace.package.version.
#
# Cargo.toml's [workspace.dependencies] block pins each internal crate
# (alint-core, alint-dsl, ...) with a path + version. The version
# acts as an "API-compat floor" — the lowest version at which the
# crate's public API was stable. External consumers depending on (e.g.)
# alint-core via crates.io get any 0.9.x where x >= floor. Floors only
# move forward when an inter-crate API actually breaks; this is
# usually rare relative to patch-release cadence. See RELEASING.md for
# the bump policy.
#
# What this check catches:
#   - A floor pin accidentally exceeds workspace.package.version (e.g.
#     hand-editing Cargo.toml and bumping it too far). The result
#     would be an unsatisfiable dependency at publish time.
#   - A new path-having entry added without a version field at all.
#
# What this check does NOT do:
#   - Require lockstep between floor and workspace version. They are
#     deliberately decoupled.
#   - Prompt for floor bumps when a public API changes. That is a
#     human judgement call.
#
# Exit codes:
#   0  all floor pins <= workspace.package.version
#   1  drift detected (over-pinned floor, or missing version field)
#   2  could not parse Cargo.toml
#
# Usage:
#   bash ci/scripts/check-workspace-dep-floors.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

WORKSPACE_VER=$(awk -F'"' '
  /^\[workspace\.package\]/ { f=1 }
  f && /^version =/ { print $2; exit }
' Cargo.toml)

if [[ -z "${WORKSPACE_VER:-}" ]]; then
  echo "[workspace-dep-floors] could not read [workspace.package].version" >&2
  exit 2
fi

# Lines like:
#   alint-core = { path = "crates/alint-core", version = "0.9.8" }
#
# The /\{.*path/ filter is on the curly-braced inline-table form,
# avoiding false matches against string-form deps whose NAME happens
# to contain `path` (e.g. `serde_json_path = "0.7"`).
entries=$(awk '
  /^\[workspace\.dependencies\]/ { f=1; next }
  /^\[/ { f=0 }
  f && !/^[[:space:]]*#/ && /\{.*path[[:space:]]*=/ { print }
' Cargo.toml)

failed=0
count=0
while IFS= read -r line; do
  [[ -z "$line" ]] && continue
  count=$((count + 1))
  name="${line%%=*}"
  name="${name// /}"
  if [[ ! "$line" =~ version\ =\ \"([0-9]+\.[0-9]+\.[0-9]+)\" ]]; then
    echo "[workspace-dep-floors] $name: missing version field (or non-X.Y.Z shape)" >&2
    failed=1
    continue
  fi
  pin="${BASH_REMATCH[1]}"

  # sort -V is version-aware; head -1 gives the smaller. If the
  # smaller is the pin, pin <= workspace. If they're equal, both
  # are "smaller" candidates; head -1 still works.
  smaller=$(printf "%s\n%s\n" "$pin" "$WORKSPACE_VER" | sort -V | head -1)
  if [[ "$smaller" != "$pin" ]]; then
    echo "[workspace-dep-floors] $name: pin $pin > workspace $WORKSPACE_VER" >&2
    failed=1
  fi
done <<< "$entries"

if [[ "$count" -eq 0 ]]; then
  echo "[workspace-dep-floors] no path-having entries in [workspace.dependencies]" >&2
  exit 1
fi

if [[ "$failed" -ne 0 ]]; then
  echo "" >&2
  echo "Fix: ensure every path-having entry in [workspace.dependencies] pins" >&2
  echo "  version <= $WORKSPACE_VER (the workspace.package.version)." >&2
  echo "  Bump a floor only when the relevant crate's public API broke." >&2
  exit 1
fi

echo "[workspace-dep-floors] OK — all $count path-having pins <= $WORKSPACE_VER"
