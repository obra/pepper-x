#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
mode="${1:-}"

usage() {
    echo "Usage: $0 <text-editor|browser-textarea>" >&2
    exit 1
}

browser_command() {
    local candidate

    for candidate in firefox chromium chromium-browser google-chrome brave-browser microsoft-edge vivaldi; do
        if command -v "${candidate}" >/dev/null 2>&1; then
            echo "${candidate}"
            return 0
        fi
    done

    return 1
}

case "${mode}" in
    text-editor)
        test_path="atspi::accessible_insert_live::accessible_insert_live_text_editor_round_trip"
        focus_hint="Focus a GNOME Text Editor document before running this helper."
        if ! command -v gnome-text-editor >/dev/null 2>&1; then
            echo "gnome-text-editor is required for the accessible insertion smoke" >&2
            exit 1
        fi
        ;;
    browser-textarea)
        test_path="atspi::accessible_insert_live::accessible_insert_live_browser_textarea_round_trip"
        if ! browser_binary="$(browser_command)"; then
            echo "A browser executable is required for the browser-textarea smoke" >&2
            exit 1
        fi
        focus_hint="Focus a textarea in ${browser_binary} before running this helper."
        ;;
    *)
        usage
        ;;
esac

if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]]; then
    echo "Run this helper inside the live GNOME session or export that session's DBUS_SESSION_BUS_ADDRESS first" >&2
    exit 1
fi

if [[ -z "${XDG_RUNTIME_DIR:-}" ]]; then
    echo "XDG_RUNTIME_DIR must point at the live GNOME session runtime directory" >&2
    exit 1
fi

if [[ "${XDG_SESSION_TYPE:-}" != "wayland" ]]; then
    echo "Pepper X accessible insertion smoke requires a GNOME Wayland session" >&2
    exit 1
fi

echo "${focus_hint}" >&2

log_file="$(mktemp)"
trap 'rm -f "${log_file}"' EXIT

(
    cd "${repo_root}"
    cargo test -p pepperx-platform-gnome "${test_path}" -- --ignored --exact --nocapture
) 2>&1 | tee "${log_file}"

if ! grep -q "running 1 test" "${log_file}"; then
    echo "Pepper X accessible insertion smoke did not run ${test_path}" >&2
    exit 1
fi

if ! grep -q "test ${test_path} ... ok" "${log_file}"; then
    echo "Pepper X accessible insertion smoke failed" >&2
    exit 1
fi
