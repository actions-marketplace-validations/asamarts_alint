#!/usr/bin/env bash
# Print the commit a perf gate compares HEAD against (empty: nothing to compare).
#   - pull request: PR_BASE_SHA, the PR's base;
#   - push: PUSH_BEFORE_SHA, the tip before the push, so a direct push to main
#     is gated on exactly what it changed. After a force-push the old tip is
#     not an ancestor of HEAD; compare against where the histories meet;
#   - otherwise: the merge base with origin/main.
# Sourced by det-perf-gate.sh and micro-det-gate.sh so both gates agree.
perf_gate_base() {
  local before="${PUSH_BEFORE_SHA:-}"
  if [[ -n "${PR_BASE_SHA:-}" ]]; then
    printf '%s\n' "$PR_BASE_SHA"
  elif [[ -n "$before" && "$before" != 0000000000000000000000000000000000000000 ]] \
    && git cat-file -e "${before}^{commit}" 2>/dev/null; then
    if git merge-base --is-ancestor "$before" HEAD 2>/dev/null; then
      printf '%s\n' "$before"
    else
      git merge-base "$before" HEAD 2>/dev/null || true
    fi
  else
    git merge-base HEAD origin/main 2>/dev/null || true
  fi
}
