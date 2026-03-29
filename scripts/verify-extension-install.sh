#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
extension_uuid="pepperx@obra"

cd "${repo_root}"

if [[ $# -gt 1 ]]; then
    echo "usage: $0 [package-root]" >&2
    exit 2
fi

if [[ $# -eq 1 ]]; then
    export PEPPERX_EXTENSION_ROOT="${1%/}/usr/share/gnome-shell/extensions/${extension_uuid}"
fi

./scripts/dev-install-extension.sh --check
