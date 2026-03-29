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

if [[ -z "${PEPPERX_CLEANUP_MODEL_PATH:-}" ]]; then
    echo "PEPPERX_CLEANUP_MODEL_PATH must point at a GGUF cleanup model" >&2
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

export PEPPERX_DISABLE_CONTEXT_CAPTURE="${PEPPERX_DISABLE_CONTEXT_CAPTURE:-1}"

fixture_path="${repo_root}/tests/fixtures/loop1-hello.wav"
log_path="${PEPPERX_STATE_ROOT}/transcript-log.jsonl"

cleanup_output="$(
    PEPPERX_PARAKEET_MODEL_DIR="${PEPPERX_PARAKEET_MODEL_DIR}" \
    PEPPERX_CLEANUP_MODEL_PATH="${PEPPERX_CLEANUP_MODEL_PATH}" \
    PEPPERX_STATE_ROOT="${PEPPERX_STATE_ROOT}" \
    PEPPERX_DISABLE_CONTEXT_CAPTURE="${PEPPERX_DISABLE_CONTEXT_CAPTURE}" \
    cargo run -p pepper-x-app --quiet -- --transcribe-wav-and-cleanup "${fixture_path}"
)"

if [[ -z "${cleanup_output//[[:space:]]/}" ]]; then
    echo "Pepper X cleanup CLI did not emit cleaned text" >&2
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

if cleanup.get("backend_name") != "llama.cpp":
    raise SystemExit(f"Unexpected cleanup backend: {cleanup}")

if not cleanup.get("model_name", "").strip():
    raise SystemExit(f"Cleanup diagnostics are missing model_name: {cleanup}")

if cleanup.get("succeeded") is not True:
    raise SystemExit(f"Cleanup diagnostics did not report success: {cleanup}")

if not cleanup.get("cleaned_text", "").strip():
    raise SystemExit(f"Cleanup diagnostics are missing cleaned_text: {cleanup}")

if stdout_cleanup != cleanup["cleaned_text"]:
    raise SystemExit(
        "Pepper X cleanup CLI stdout must match the archived cleaned transcript: "
        f"{stdout_cleanup!r} != {cleanup['cleaned_text']!r}"
    )
PY
