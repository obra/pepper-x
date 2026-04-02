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

fixture_path="${repo_root}/tests/fixtures/loop1-hello.wav"
log_path="${PEPPERX_STATE_ROOT}/transcript-log.jsonl"

transcript_output="$(
    PEPPERX_PARAKEET_MODEL_DIR="${PEPPERX_PARAKEET_MODEL_DIR}" \
    PEPPERX_STATE_ROOT="${PEPPERX_STATE_ROOT}" \
    cargo run -p pepper-x-app --quiet -- --transcribe-wav "${fixture_path}"
)"

if [[ -z "${transcript_output//[[:space:]]/}" ]]; then
    echo "Pepper X CLI did not emit a transcript" >&2
    exit 1
fi

if [[ ! -f "${log_path}" ]]; then
    echo "Pepper X did not write a transcript log to ${log_path}" >&2
    exit 1
fi

python3 - "${log_path}" "${fixture_path}" "${transcript_output}" <<'PY'
import json
import sys
from pathlib import Path

log_path = Path(sys.argv[1])
fixture_path = Path(sys.argv[2]).resolve()
stdout_transcript = sys.argv[3].strip()
lines = [line for line in log_path.read_text().splitlines() if line.strip()]
if not lines:
    raise SystemExit(f"Transcript log is empty: {log_path}")

entry = json.loads(lines[-1])
if not entry.get("transcript_text", "").strip():
    raise SystemExit(f"Transcript log entry is missing transcript_text: {entry}")

source_wav_path = Path(entry["source_wav_path"])
if source_wav_path != fixture_path:
    raise SystemExit(
        "Transcript log entry used the wrong WAV path: "
        f"{source_wav_path} != {fixture_path}"
    )

if entry.get("backend_name") != "parakeet-rs":
    raise SystemExit(f"Unexpected backend_name in transcript log: {entry}")

if not entry.get("model_name", "").strip():
    raise SystemExit(f"Transcript log entry is missing model_name: {entry}")

if stdout_transcript != entry["transcript_text"]:
    raise SystemExit(
        "Pepper X CLI stdout must match the archived transcript: "
        f"{stdout_transcript!r} != {entry['transcript_text']!r}"
    )
PY
