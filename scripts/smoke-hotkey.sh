#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

python3 - "${repo_root}" <<'PY'
import pathlib
import sys

repo_root = pathlib.Path(sys.argv[1])
extension_source = (repo_root / "gnome-extension" / "extension.js").read_text()
keybindings_source = (repo_root / "gnome-extension" / "keybindings.js").read_text()
manual_source = (repo_root / "tests" / "smoke" / "test_modifier_only_hotkey.md").read_text()

required_keybinding_markers = [
    "registerModifierHold",
    "captured-event",
    "KEY_PRESS",
    "KEY_RELEASE",
]

for marker in required_keybinding_markers:
    if marker not in keybindings_source:
        raise SystemExit(f"Missing modifier-only keybinding marker: {marker}")

required_extension_markers = [
    "startRecording(",
    "stopRecording(",
    "modifier-only",
    "start sent",
    "stop sent",
    "App unavailable",
    "duplicate request ignored",
]

for marker in required_extension_markers:
    if marker not in extension_source:
        raise SystemExit(f"Missing modifier-only extension marker: {marker}")

required_manual_markers = [
    "modifier-only press triggers StartRecording",
    "release triggers StopRecording",
    "repeated use stays stable",
    "disabling the extension removes the behavior cleanly",
]

for marker in required_manual_markers:
    if marker not in manual_source:
        raise SystemExit(f"Missing manual smoke expectation: {marker}")
PY
