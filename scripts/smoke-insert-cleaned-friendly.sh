#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "${repo_root}"

if ! command -v cargo >/dev/null 2>&1 && [[ -f "${HOME}/.cargo/env" ]]; then
    . "${HOME}/.cargo/env"
fi

if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]]; then
    echo "Run this helper inside the live GNOME session or export that session's DBUS_SESSION_BUS_ADDRESS first" >&2
    exit 1
fi

if [[ -z "${XDG_RUNTIME_DIR:-}" ]]; then
    echo "XDG_RUNTIME_DIR must point at the live GNOME session runtime directory" >&2
    exit 1
fi

if [[ "${XDG_SESSION_TYPE:-}" != "wayland" ]]; then
    echo "Pepper X cleaned insertion smoke requires a GNOME Wayland session" >&2
    exit 1
fi

if ! command -v gnome-text-editor >/dev/null 2>&1; then
    echo "gnome-text-editor is required for the cleaned insertion smoke" >&2
    exit 1
fi

if [[ -z "${PEPPERX_PARAKEET_MODEL_DIR:-}" ]]; then
    echo "PEPPERX_PARAKEET_MODEL_DIR must point at a Parakeet model bundle" >&2
    exit 1
fi

if [[ -z "${PEPPERX_CLEANUP_MODEL_PATH:-}" ]]; then
    echo "PEPPERX_CLEANUP_MODEL_PATH must point at a GGUF cleanup model" >&2
    exit 1
fi

echo "Focus a GNOME Text Editor document before running this helper." >&2

state_root="$(mktemp -d)"
trap 'rm -rf "${state_root}"' EXIT

fixture_path="${repo_root}/tests/fixtures/loop1-hello.wav"
log_path="${state_root}/transcript-log.jsonl"

cleanup_output="$(
    PEPPERX_PARAKEET_MODEL_DIR="${PEPPERX_PARAKEET_MODEL_DIR}" \
    PEPPERX_CLEANUP_MODEL_PATH="${PEPPERX_CLEANUP_MODEL_PATH}" \
    PEPPERX_STATE_ROOT="${state_root}" \
    cargo run -p pepper-x-app --quiet -- --transcribe-wav-and-cleanup-and-insert-friendly "${fixture_path}"
)"

if [[ -z "${cleanup_output//[[:space:]]/}" ]]; then
    echo "Pepper X cleaned insertion CLI did not emit cleaned text" >&2
    exit 1
fi

if [[ ! -f "${log_path}" ]]; then
    echo "Pepper X did not write a transcript log to ${log_path}" >&2
    exit 1
fi

python3 - "${log_path}" "${fixture_path}" "${cleanup_output}" <<'PY'
import json
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
fixture_path = Path(sys.argv[2]).resolve()
stdout_cleanup = sys.argv[3].strip()
lines = [line for line in log_path.read_text().splitlines() if line.strip()]
if not lines:
    raise SystemExit(f"Transcript log is empty: {log_path}")

entry = json.loads(lines[-1])
if Path(entry["source_wav_path"]) != fixture_path:
    raise SystemExit(
        "Transcript log entry used the wrong WAV path: "
        f"{entry['source_wav_path']} != {fixture_path}"
    )

if not entry.get("transcript_text", "").strip():
    raise SystemExit(f"Transcript log entry is missing transcript_text: {entry}")

cleanup = entry.get("cleanup")
if not cleanup:
    raise SystemExit(f"Transcript log entry is missing cleanup diagnostics: {entry}")

if cleanup.get("succeeded") is not True:
    raise SystemExit(f"Cleanup diagnostics did not report success: {cleanup}")

if not cleanup.get("cleaned_text", "").strip():
    raise SystemExit(f"Cleanup diagnostics are missing cleaned_text: {cleanup}")

if stdout_cleanup != cleanup["cleaned_text"]:
    raise SystemExit(
        "Pepper X cleaned insertion stdout must match the archived cleaned transcript: "
        f"{stdout_cleanup!r} != {cleanup['cleaned_text']!r}"
    )

insertion = entry.get("insertion")
if not insertion:
    raise SystemExit(f"Transcript log entry is missing insertion diagnostics: {entry}")

if insertion.get("succeeded") is not True:
    raise SystemExit(f"Insertion diagnostics did not report success: {insertion}")

if not insertion.get("backend_name", "").strip():
    raise SystemExit(f"Insertion diagnostics are missing backend_name: {insertion}")

if insertion.get("target_application_name") != "Text Editor":
    raise SystemExit(
        "Pepper X cleaned insertion targeted the wrong application: "
        f"{insertion.get('target_application_name')!r}"
    )
PY

echo "Pepper X archived cleaned text and reported successful friendly insertion." >&2
