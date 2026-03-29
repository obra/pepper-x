#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "${repo_root}"

if ! command -v cargo >/dev/null 2>&1 && [[ -f "${HOME}/.cargo/env" ]]; then
    . "${HOME}/.cargo/env"
fi

cargo test -p pepperx-cleanup cleanup_ocr_ -- --nocapture
cargo test -p pepperx-platform-gnome context_ -- --nocapture
cargo test -p pepper-x-app cleanup_ocr_ -- --nocapture
