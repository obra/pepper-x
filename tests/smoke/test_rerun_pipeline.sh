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
history_root="${PEPPERX_STATE_ROOT}/history"

PEPPERX_PARAKEET_MODEL_DIR="${PEPPERX_PARAKEET_MODEL_DIR}" \
PEPPERX_CLEANUP_MODEL_PATH="${PEPPERX_CLEANUP_MODEL_PATH}" \
PEPPERX_STATE_ROOT="${PEPPERX_STATE_ROOT}" \
PEPPERX_DISABLE_CONTEXT_CAPTURE="${PEPPERX_DISABLE_CONTEXT_CAPTURE}" \
cargo run -p pepper-x-app --quiet -- --set-cleanup-prompt-profile ordinary-dictation >/dev/null

PEPPERX_PARAKEET_MODEL_DIR="${PEPPERX_PARAKEET_MODEL_DIR}" \
PEPPERX_CLEANUP_MODEL_PATH="${PEPPERX_CLEANUP_MODEL_PATH}" \
PEPPERX_STATE_ROOT="${PEPPERX_STATE_ROOT}" \
PEPPERX_DISABLE_CONTEXT_CAPTURE="${PEPPERX_DISABLE_CONTEXT_CAPTURE}" \
cargo run -p pepper-x-app --quiet -- --transcribe-wav-and-cleanup "${fixture_path}" >/dev/null

parent_run_id="$(
    python3 - "${history_root}" <<'PY'
import json
import sys
from pathlib import Path

history_root = Path(sys.argv[1])
metadata_paths = sorted(history_root.glob("*/run.json"))
if not metadata_paths:
    raise SystemExit("Pepper X rerun smoke did not archive an initial run")

latest = max(
    (json.loads(path.read_text()) for path in metadata_paths),
    key=lambda payload: (payload["archived_at_ms"], payload["run_id"]),
)
print(latest["run_id"])
PY
)"

rerun_args=(
    --rerun-archived-run
    "${parent_run_id}"
    --cleanup-prompt-profile
    literal-dictation
)

if [[ -n "${PEPPERX_RERUN_ASR_MODEL:-}" ]]; then
    rerun_args+=(--asr-model "${PEPPERX_RERUN_ASR_MODEL}")
fi

if [[ -n "${PEPPERX_RERUN_CLEANUP_MODEL:-}" ]]; then
    rerun_args+=(--cleanup-model "${PEPPERX_RERUN_CLEANUP_MODEL}")
fi

rerun_output="$(
    PEPPERX_PARAKEET_MODEL_DIR="${PEPPERX_PARAKEET_MODEL_DIR}" \
    PEPPERX_CLEANUP_MODEL_PATH="${PEPPERX_CLEANUP_MODEL_PATH}" \
    PEPPERX_STATE_ROOT="${PEPPERX_STATE_ROOT}" \
    PEPPERX_DISABLE_CONTEXT_CAPTURE="${PEPPERX_DISABLE_CONTEXT_CAPTURE}" \
    cargo run -p pepper-x-app --quiet -- "${rerun_args[@]}"
)"

if [[ -z "${rerun_output//[[:space:]]/}" ]]; then
    echo "Pepper X rerun CLI did not emit rerun text" >&2
    exit 1
fi

expected_rerun_asr_model="${PEPPERX_RERUN_ASR_MODEL:-}"
expected_rerun_cleanup_model="${PEPPERX_RERUN_CLEANUP_MODEL:-}"

python3 - "${history_root}" "${parent_run_id}" "${rerun_output}" "${expected_rerun_asr_model}" "${expected_rerun_cleanup_model}" <<'PY'
import json
import sys
from pathlib import Path

history_root = Path(sys.argv[1])
parent_run_id = sys.argv[2]
stdout_rerun = sys.argv[3].strip()
expected_asr_model = sys.argv[4] or None
expected_cleanup_model = sys.argv[5] or None

metadata_payloads = [
    json.loads(path.read_text())
    for path in history_root.glob("*/run.json")
]
if len(metadata_payloads) < 2:
    raise SystemExit("Pepper X rerun smoke expected at least two archived runs")

metadata_payloads.sort(key=lambda payload: (payload["archived_at_ms"], payload["run_id"]), reverse=True)
child = metadata_payloads[0]
parent = next(
    (payload for payload in metadata_payloads if payload["run_id"] == parent_run_id),
    None,
)
if parent is None:
    raise SystemExit(f"Parent run {parent_run_id} is missing from archive metadata")

if child["run_id"] == parent_run_id:
    raise SystemExit("Rerun smoke did not archive a new child run")

if child.get("parent_run_id") != parent_run_id:
    raise SystemExit(
        f"Rerun smoke expected parent_run_id={parent_run_id!r}, got {child.get('parent_run_id')!r}"
    )

if parent.get("parent_run_id") is not None:
    raise SystemExit(f"Parent run should remain unlinked: {parent}")

if parent.get("prompt_profile") != "ordinary-dictation":
    raise SystemExit(f"Parent prompt profile changed unexpectedly: {parent}")

if child.get("prompt_profile") != "literal-dictation":
    raise SystemExit(f"Child prompt profile did not capture the rerun override: {child}")

if expected_asr_model is None:
    expected_asr_model = parent["entry"]["model_name"]
if child["entry"]["model_name"] != expected_asr_model:
    raise SystemExit(
        "Child rerun did not capture the expected ASR model override: "
        f"{child['entry']['model_name']!r} != {expected_asr_model!r}"
    )

parent_archived_wav = Path(parent["archived_source_wav_path"])
child_entry_wav = Path(child["entry"]["source_wav_path"])
if child_entry_wav != parent_archived_wav:
    raise SystemExit(
        "Rerun should transcribe the archived parent WAV: "
        f"{child_entry_wav} != {parent_archived_wav}"
    )

child_archived_wav = Path(child["archived_source_wav_path"])
if not child_archived_wav.is_file():
    raise SystemExit(f"Child archived source WAV is missing: {child_archived_wav}")

if child_archived_wav.read_bytes() != parent_archived_wav.read_bytes():
    raise SystemExit("Child archived source WAV does not match the parent archived source WAV")

cleanup = child["entry"].get("cleanup")
if not cleanup or cleanup.get("succeeded") is not True:
    raise SystemExit(f"Child rerun is missing successful cleanup diagnostics: {child}")

expected_cleanup_model = expected_cleanup_model or parent["entry"]["cleanup"]["model_name"]
if cleanup.get("model_name") != expected_cleanup_model:
    raise SystemExit(
        "Child rerun did not capture the expected cleanup model override: "
        f"{cleanup.get('model_name')!r} != {expected_cleanup_model!r}"
    )

if stdout_rerun != cleanup.get("cleaned_text", "").strip():
    raise SystemExit(
        "Pepper X rerun CLI stdout must match the archived cleaned transcript: "
        f"{stdout_rerun!r} != {cleanup.get('cleaned_text')!r}"
    )
PY
