#!/usr/bin/env bash
# Resolve the binary release selected by the composite Action.
#
# Arguments:
#   1. Explicit `version:` input (empty when omitted)
#   2. github.action_ref (exact tag, branch, or commit SHA; empty for `uses: ./`)
#   3. Release version baked into this action.yml commit
#
# Keep this function side-effect-free: action.yml sources it, and the shell test
# exercises the same production implementation.

_is_alint_release_tag() {
  [[ "$1" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]
}

resolve_alint_action_version() {
  local requested_version="${1:-}"
  local action_ref="${2:-}"
  local baked_version="${3:-}"

  if [[ -n "$requested_version" ]]; then
    if [[ "$requested_version" != "latest" ]] && ! _is_alint_release_tag "$requested_version"; then
      echo "alint action: invalid version '$requested_version' (expected 'latest' or a release tag)" >&2
      return 2
    fi
    printf '%s\n' "$requested_version"
  elif _is_alint_release_tag "$action_ref"; then
    printf '%s\n' "$action_ref"
  elif [[ "$action_ref" =~ ^[0-9a-fA-F]{40}$ ]]; then
    if ! _is_alint_release_tag "$baked_version"; then
      echo "alint action: commit-SHA ref has no valid baked release version" >&2
      return 2
    fi
    printf '%s\n' "$baked_version"
  else
    # Preserve the historical moving-ref behaviour for branches (`main`, `v0`)
    # and local `uses: ./` action tests.
    printf '%s\n' latest
  fi
}
