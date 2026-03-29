#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
test_path="atspi::accessible_insert_live::accessible_insert_live_terminal_round_trip"

if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]]; then
    echo "Run this helper inside the live GNOME session or export that session's DBUS_SESSION_BUS_ADDRESS first" >&2
    exit 1
fi

if [[ -z "${XDG_RUNTIME_DIR:-}" ]]; then
    echo "XDG_RUNTIME_DIR must point at the live GNOME session runtime directory" >&2
    exit 1
fi

if [[ "${XDG_SESSION_TYPE:-}" != "wayland" ]]; then
    echo "Pepper X terminal insertion smoke requires a GNOME Wayland session" >&2
    exit 1
fi

if ! command -v gnome-terminal >/dev/null 2>&1; then
    echo "gnome-terminal is required for the terminal insertion smoke" >&2
    exit 1
fi

smoke_dir="$(mktemp -d)"
smoke_file="${smoke_dir}/terminal-marker.txt"
marker="pepperx-terminal-$(date +%s%N)"
escaped_marker="$(printf "%s" "${marker}" | sed "s/'/'\"'\"'/g")"
escaped_file="$(printf "%s" "${smoke_file}" | sed "s/'/'\"'\"'/g")"

trap 'rm -rf "${smoke_dir}"' EXIT

export PEPPERX_TERMINAL_SMOKE_FILE="${smoke_file}"
export PEPPERX_TERMINAL_EXPECTED_MARKER="${marker}"
export PEPPERX_TERMINAL_INSERT_TEXT="printf '%s' '${escaped_marker}' > '${escaped_file}'"$'\n'

echo "Focus a GNOME Terminal shell prompt before running this helper." >&2

log_file="$(mktemp)"
trap 'rm -rf "${smoke_dir}"; rm -f "${log_file}"' EXIT

(
    cd "${repo_root}"
    cargo test -p pepperx-platform-gnome "${test_path}" -- --ignored --exact --nocapture
) 2>&1 | tee "${log_file}"

if ! grep -q "running 1 test" "${log_file}"; then
    echo "Pepper X terminal insertion smoke did not run ${test_path}" >&2
    exit 1
fi

if ! grep -q "test ${test_path} ... ok" "${log_file}"; then
    echo "Pepper X terminal insertion smoke failed" >&2
    exit 1
fi
