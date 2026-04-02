#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "${repo_root}"

if ! command -v cargo >/dev/null 2>&1 && [[ -f "${HOME}/.cargo/env" ]]; then
    . "${HOME}/.cargo/env"
fi

asr_model_id="nemotron-speech-streaming-en-0.6b"
alternate_asr_model_id="nemo-parakeet-tdt-0.6b-v3-int8"
cleanup_model_id="qwen3.5-2b-q4_k_m.gguf"
alternate_cleanup_model_id="qwen3.5-0.8b-q4_k_m.gguf"
offline_mode=0

if [[ "${1:-}" == "--offline" ]]; then
    offline_mode=1
fi

if [[ -z "${PEPPERX_STATE_ROOT:-}" ]]; then
    echo "PEPPERX_STATE_ROOT must point at a writable state directory" >&2
    exit 1
fi

if [[ -z "${XDG_CACHE_HOME:-}" ]]; then
    echo "XDG_CACHE_HOME must point at a writable cache root" >&2
    exit 1
fi

mkdir -p "${PEPPERX_STATE_ROOT}" "${XDG_CACHE_HOME}"

run_cli() {
    PEPPERX_STATE_ROOT="${PEPPERX_STATE_ROOT}" \
    XDG_CACHE_HOME="${XDG_CACHE_HOME}" \
    cargo run -p pepper-x-app --quiet -- "$@"
}

assert_contains() {
    local haystack="$1"
    local needle="$2"

    if [[ "${haystack}" != *"${needle}"* ]]; then
        echo "Expected output to contain: ${needle}" >&2
        exit 1
    fi
}

status_before="$(run_cli --list-models)"
assert_contains "${status_before}" "Model cache: ${XDG_CACHE_HOME}/pepper-x/models"
assert_contains "${status_before}" "Default ASR model: ${asr_model_id}"
assert_contains "${status_before}" "Default cleanup model: ${cleanup_model_id}"
assert_contains "${status_before}" "Cleanup prompt profile: ordinary-dictation"
assert_contains "${status_before}" "- ${alternate_asr_model_id} [asr]"
assert_contains "${status_before}" "- ${alternate_cleanup_model_id} [cleanup]"

run_cli --set-default-asr-model "${asr_model_id}" >/dev/null
run_cli --set-default-cleanup-model "${cleanup_model_id}" >/dev/null
run_cli --set-cleanup-prompt-profile ordinary-dictation >/dev/null

python3 - "${PEPPERX_STATE_ROOT}/settings.json" "${asr_model_id}" "${cleanup_model_id}" <<'PY'
import json
import sys
from pathlib import Path

settings_path = Path(sys.argv[1])
asr_model_id = sys.argv[2]
cleanup_model_id = sys.argv[3]

settings = json.loads(settings_path.read_text())
if settings["preferred_asr_model"] != asr_model_id:
    raise SystemExit(f"preferred_asr_model mismatch: {settings}")
if settings["preferred_cleanup_model"] != cleanup_model_id:
    raise SystemExit(f"preferred_cleanup_model mismatch: {settings}")
if settings["cleanup_prompt_profile"] != "ordinary-dictation":
    raise SystemExit(f"cleanup_prompt_profile mismatch: {settings}")
PY

if [[ "${offline_mode}" -eq 0 ]]; then
    echo "Pepper X model smoke: bootstrapping ${asr_model_id}" >&2
    run_cli --bootstrap-model "${asr_model_id}" >/dev/null

    echo "Pepper X model smoke: bootstrapping ${cleanup_model_id} (large download)" >&2
    run_cli --bootstrap-model "${cleanup_model_id}" >/dev/null
fi

status_after="$(run_cli --list-models)"
assert_contains "${status_after}" "Default ASR model: ${asr_model_id}"
assert_contains "${status_after}" "Default cleanup model: ${cleanup_model_id}"
assert_contains "${status_after}" "- ${alternate_asr_model_id} [asr]"
assert_contains "${status_after}" "- ${alternate_cleanup_model_id} [cleanup]"

if [[ "${offline_mode}" -eq 0 ]]; then
    assert_contains "${status_after}" "- ${asr_model_id} [asr] ready"
    assert_contains "${status_after}" "- ${cleanup_model_id} [cleanup] ready"
fi
