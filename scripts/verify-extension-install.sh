#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

cd "${repo_root}"
./scripts/dev-install-extension.sh --check
