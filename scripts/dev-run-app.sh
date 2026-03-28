#!/usr/bin/env bash

set -euo pipefail

cd "$(dirname "$0")/.."
cargo run -p pepper-x-app -- "$@"
