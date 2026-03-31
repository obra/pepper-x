#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "${repo_root}"

if ! command -v cargo >/dev/null 2>&1 && [[ -f "${HOME}/.cargo/env" ]]; then
    . "${HOME}/.cargo/env"
fi

if [[ -z "${PEPPERX_PARAKEET_MODEL_DIR:-}" ]]; then
    echo "PEPPERX_PARAKEET_MODEL_DIR must point at a Parakeet model bundle" >&2
    exit 1
fi

if [[ -z "${PEPPERX_STATE_ROOT:-}" ]]; then
    echo "PEPPERX_STATE_ROOT must point at a writable state directory" >&2
    exit 1
fi

if [[ ! -d "${PEPPERX_STATE_ROOT}" || ! -w "${PEPPERX_STATE_ROOT}" ]]; then
    echo "PEPPERX_STATE_ROOT must already exist and be writable: ${PEPPERX_STATE_ROOT}" >&2
    exit 1
fi

if [[ -z "${XDG_RUNTIME_DIR:-}" ]]; then
    echo "XDG_RUNTIME_DIR must be set inside a real user session" >&2
    exit 1
fi

if ! command -v gdbus >/dev/null 2>&1; then
    echo "gdbus is required to verify the GNOME session bus" >&2
    exit 1
fi

if ! gdbus call \
    --session \
    --dest org.gnome.Shell \
    --object-path /org/gnome/Shell \
    --method org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1
then
    echo "Pepper X live recording smoke must run inside a GNOME Shell session" >&2
    exit 1
fi

stop_delay_seconds="${PEPPERX_LIVE_RECORDING_STOP_DELAY_SECONDS:-2}"
log_path="${PEPPERX_STATE_ROOT}/transcript-log.jsonl"
expected_insertion_target_class="${PEPPERX_EXPECT_INSERTION_TARGET_CLASS:-}"
expected_insertion_backend="${PEPPERX_EXPECT_INSERTION_BACKEND:-}"

echo "Pepper X live recording smoke: speak now; stopping in ${stop_delay_seconds}s" >&2

transcript_output="$(
    {
        sleep "${stop_delay_seconds}"
        printf '\n'
    } | PEPPERX_PARAKEET_MODEL_DIR="${PEPPERX_PARAKEET_MODEL_DIR}" \
        PEPPERX_STATE_ROOT="${PEPPERX_STATE_ROOT}" \
        cargo run -p pepper-x-app --quiet -- --record-and-transcribe
)"

if [[ -z "${transcript_output//[[:space:]]/}" ]]; then
    echo "Pepper X live recording CLI did not emit a transcript" >&2
    exit 1
fi

if [[ ! -f "${log_path}" ]]; then
    echo "Pepper X did not write a transcript log to ${log_path}" >&2
    exit 1
fi

python3 - \
    "${log_path}" \
    "${transcript_output}" \
    "${expected_insertion_target_class}" \
    "${expected_insertion_backend}" <<'PY'
import json
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
stdout_transcript = sys.argv[2].strip()
expected_target_class = sys.argv[3].strip()
expected_backend = sys.argv[4].strip()
lines = [line for line in log_path.read_text().splitlines() if line.strip()]
if not lines:
    raise SystemExit(f"Transcript log is empty: {log_path}")

entry = json.loads(lines[-1])
source_wav_path = Path(entry["source_wav_path"])
if "live-recording-" not in source_wav_path.name:
    raise SystemExit(
        "Transcript log entry did not archive a live recording: "
        f"{source_wav_path}"
    )

if not entry.get("transcript_text", "").strip():
    raise SystemExit(f"Transcript log entry is missing transcript_text: {entry}")

if entry.get("backend_name") != "sherpa-onnx":
    raise SystemExit(f"Unexpected backend_name in transcript log: {entry}")

if stdout_transcript != entry["transcript_text"]:
    raise SystemExit(
        "Pepper X live recording stdout must match the archived transcript: "
        f"{stdout_transcript!r} != {entry['transcript_text']!r}"
    )

if expected_target_class or expected_backend:
    insertion = entry.get("insertion")
    if not insertion:
        raise SystemExit(
            "Pepper X live recording entry is missing insertion diagnostics "
            "for the requested target assertions"
        )

    if expected_target_class and insertion.get("target_class") != expected_target_class:
        raise SystemExit(
            "Pepper X live recording archived the wrong target class: "
            f"{insertion.get('target_class')!r} != {expected_target_class!r}"
        )

    if expected_backend and insertion.get("backend_name") != expected_backend:
        raise SystemExit(
            "Pepper X live recording archived the wrong insertion backend: "
            f"{insertion.get('backend_name')!r} != {expected_backend!r}"
        )
PY
