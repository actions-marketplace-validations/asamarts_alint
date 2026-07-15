#!/usr/bin/env bash
set -euo pipefail

CONTAINER_NAME="${CONTAINER_NAME:-alint-runner}"

# Remove the reboot-persistence unit first, so systemd doesn't try to recreate
# the container we're about to tear down. The unit has no ExecStop, so disabling
# it never stops a running container on its own.
echo "==> Removing systemd user unit"
systemctl --user disable --now "container-${CONTAINER_NAME}.service" 2>/dev/null || true
rm -f "${HOME}/.config/systemd/user/container-${CONTAINER_NAME}.service"
systemctl --user daemon-reload 2>/dev/null || true

echo "==> Stopping runner container: ${CONTAINER_NAME}"
podman stop -t 30 "${CONTAINER_NAME}" 2>/dev/null || true

echo "==> Removing container"
podman rm "${CONTAINER_NAME}" 2>/dev/null || true

if [[ "${1:-}" == "--purge" ]]; then
    echo "==> Purging cache volumes"
    podman volume rm alint-runner-cargo-cache 2>/dev/null || true
    podman volume rm alint-runner-cargo-target 2>/dev/null || true
    echo "==> Volumes purged"
fi

echo "==> Teardown complete"
