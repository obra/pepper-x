#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "$0")/.."
if ! command -v cargo >/dev/null 2>&1 && [[ -f "${HOME}/.cargo/env" ]]; then
    . "${HOME}/.cargo/env"
fi
cargo run -p pepper-x-app -- "$@"
