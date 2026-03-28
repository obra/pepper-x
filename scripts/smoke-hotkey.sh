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
integration_source = (repo_root / "docs" / "architecture" / "gnome-integration.md").read_text()
live_helper_source = (repo_root / "scripts" / "gnome48-smoke-hotkey.sh").read_text()

for forbidden_marker in [
    "registerModifierHold",
    "_startModifierOnlyRecording",
    "_stopModifierOnlyRecording",
    "startRecording(",
    "stopRecording(",
]:
    if forbidden_marker in extension_source:
        raise SystemExit(
            f"Extension still owns modifier-only hotkey behavior: {forbidden_marker}"
        )

for forbidden_marker in [
    "registerModifierHold",
    "grab_accelerator",
    "accelerator-activated",
    "allowKeybinding",
    "global.get_pointer()[2]",
]:
    if forbidden_marker in keybindings_source:
        raise SystemExit(
            f"Keybinding module still owns modifier-only hotkey behavior: {forbidden_marker}"
        )

for required_marker in [
    "showSettings",
    "Ping",
    "getCapabilities",
]:
    if required_marker not in extension_source:
        raise SystemExit(f"Missing thin-extension marker: {required_marker}")

required_manual_markers = [
    "modifier-only press triggers StartRecording",
    "release triggers StopRecording",
    "repeated use stays stable",
]

for marker in required_manual_markers:
    if marker not in manual_source:
        raise SystemExit(f"Missing manual smoke expectation: {marker}")

if "disabling the extension removes the behavior cleanly" in manual_source:
    raise SystemExit("Manual smoke doc still assumes extension-owned modifier capture")

if "use `StartRecording` and `StopRecording` for hold-to-talk signaling" in integration_source:
    raise SystemExit("GNOME integration doc still assigns hold-to-talk to the extension")

for forbidden_marker in [
    "PEPPERX_KEYBOARD_MONITOR_NAME",
    "KeyboardMonitor",
]:
    if forbidden_marker in live_helper_source:
        raise SystemExit(
            f"Live GNOME 48 helper still assumes a fictitious Pepper X keyboard monitor bus name: {forbidden_marker}"
        )
PY
