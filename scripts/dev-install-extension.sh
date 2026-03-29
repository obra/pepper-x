#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
extension_root="${PEPPERX_EXTENSION_ROOT:-${repo_root}/gnome-extension}"
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
import ast
import json
import pathlib
import re
import sys

extension_root = pathlib.Path(sys.argv[1])
metadata = json.loads((extension_root / "metadata.json").read_text())

expected_uuid = "pepperx@obra"
expected_shell_versions = ["48"]

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

for module_name in ("panelMenu", "popupMenu"):
    if re.search(
        rf"import\s+[A-Za-z_]\w*\s+from\s+'resource:///org/gnome/shell/ui/{module_name}\.js'",
        extension_source,
    ):
        raise SystemExit(
            f"extension.js must use a namespace import for {module_name}.js"
        )

if "createPepperXClient" not in ipc_source:
    raise SystemExit("ipc.js must export a Pepper X D-Bus client builder")

if "com.obra.PepperX" not in ipc_source:
    raise SystemExit("ipc.js must target the Pepper X D-Bus service")

if "KeybindingRegistry" not in keybindings_source:
    raise SystemExit("keybindings.js must export a keybinding registry")

for method_name in ("registerCleanup", "clear"):
    if not re.search(rf"\b{method_name}\s*\(", keybindings_source):
        raise SystemExit(f"keybindings.js must define {method_name}()")

def parse_enabled_extensions(raw: str) -> list[str]:
    normalized = raw.strip()

    if normalized.startswith("@as "):
        normalized = normalized[4:]

    return list(ast.literal_eval(normalized))

if parse_enabled_extensions("@as []") != []:
    raise SystemExit("enabled-extensions parser must handle empty GVariant arrays")

if parse_enabled_extensions("['pepperx@obra']") != ["pepperx@obra"]:
    raise SystemExit("enabled-extensions parser must preserve existing extension UUIDs")
PY
}

check_extension

extension_known_to_shell() {
    if ! command -v gnome-extensions >/dev/null 2>&1; then
        return 1
    fi

    gnome-extensions info "${extension_uuid}" >/dev/null 2>&1
}

queue_first_install() {
    if ! command -v gsettings >/dev/null 2>&1; then
        echo "Pepper X extension copied to ${install_root}. Restart GNOME Shell once, then enable ${extension_uuid} manually." >&2
        return
    fi

    python3 - "${extension_uuid}" <<'PY'
import ast
import subprocess
import sys

uuid = sys.argv[1]

def parse_enabled_extensions(raw: str) -> list[str]:
    normalized = raw.strip()

    if normalized.startswith("@as "):
        normalized = normalized[4:]

    return list(ast.literal_eval(normalized))

current = subprocess.check_output(
    ["gsettings", "get", "org.gnome.shell", "enabled-extensions"],
    text=True,
).strip()
enabled = parse_enabled_extensions(current)

if uuid not in enabled:
    enabled.append(uuid)
    subprocess.run(
        [
            "gsettings",
            "set",
            "org.gnome.shell",
            "enabled-extensions",
            str(enabled),
        ],
        check=True,
    )
PY

    echo "Pepper X extension copied to ${install_root}. Restart the GNOME session once to finish the first install." >&2
}

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

if ! extension_known_to_shell; then
    queue_first_install
    exit 0
fi

gnome-extensions disable "${extension_uuid}" >/dev/null 2>&1 || true
gnome-extensions enable "${extension_uuid}"
