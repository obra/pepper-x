#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
extension_root="${repo_root}/gnome-extension"
extension_uuid="pepperx@obra"
install_root="${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/${extension_uuid}"

require_file() {
    local path="$1"

    if [[ ! -f "${path}" ]]; then
        echo "Missing required extension file: ${path}" >&2
        exit 1
    fi
}

check_extension() {
    require_file "${extension_root}/metadata.json"
    require_file "${extension_root}/extension.js"
    require_file "${extension_root}/ipc.js"
    require_file "${extension_root}/keybindings.js"
    require_file "${extension_root}/README.md"

    python3 - "${extension_root}" <<'PY'
import json
import pathlib
import re
import sys

extension_root = pathlib.Path(sys.argv[1])
metadata = json.loads((extension_root / "metadata.json").read_text())

expected_uuid = "pepperx@obra"
expected_shell_versions = ["46", "47", "48"]

if metadata.get("uuid") != expected_uuid:
    raise SystemExit(f"metadata.json uuid must be {expected_uuid!r}")

if metadata.get("shell-version") != expected_shell_versions:
    raise SystemExit(
        "metadata.json shell-version must be "
        f"{expected_shell_versions!r}"
    )

if metadata.get("name") != "Pepper X":
    raise SystemExit("metadata.json name must be 'Pepper X'")

extension_source = (extension_root / "extension.js").read_text()
ipc_source = (extension_root / "ipc.js").read_text()
keybindings_source = (extension_root / "keybindings.js").read_text()

if not re.search(r"export\s+default\s+class\s+\w+\s+extends\s+Extension", extension_source):
    raise SystemExit("extension.js must export the GNOME Shell extension entrypoint")

for method_name in ("enable", "disable"):
    if not re.search(rf"\b{method_name}\s*\(", extension_source):
        raise SystemExit(f"extension.js must define {method_name}()")

if "showSettings" not in extension_source:
    raise SystemExit("extension.js must expose a settings action")

if "createPepperXClient" not in ipc_source:
    raise SystemExit("ipc.js must export a Pepper X D-Bus client builder")

if "com.obra.PepperX" not in ipc_source:
    raise SystemExit("ipc.js must target the Pepper X D-Bus service")

if "KeybindingRegistry" not in keybindings_source:
    raise SystemExit("keybindings.js must export a keybinding registry")

for method_name in ("registerCleanup", "clear"):
    if not re.search(rf"\b{method_name}\s*\(", keybindings_source):
        raise SystemExit(f"keybindings.js must define {method_name}()")
PY
}

check_extension

if [[ "${1:-}" == "--check" ]]; then
    exit 0
fi

if ! command -v gnome-extensions >/dev/null 2>&1; then
    echo "gnome-extensions is required to install the Pepper X extension" >&2
    exit 1
fi

mkdir -p "${install_root}"
cp \
    "${extension_root}/metadata.json" \
    "${extension_root}/extension.js" \
    "${extension_root}/ipc.js" \
    "${extension_root}/keybindings.js" \
    "${install_root}/"

gnome-extensions disable "${extension_uuid}" >/dev/null 2>&1 || true
gnome-extensions enable "${extension_uuid}"
