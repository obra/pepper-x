#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"

python3 - "${repo_root}" <<'PY'
from configparser import ConfigParser
from pathlib import Path
import sys

repo_root = Path(sys.argv[1])
deb_root = repo_root / "packaging" / "deb"
control = (deb_root / "control").read_text()
spec = (repo_root / "packaging" / "rpm" / "pepper-x.spec").read_text()

required_control_markers = [
    "Depends: ${misc:Depends}, ${shlibs:Depends}",
    "libadwaita-1-0",
    "libatspi2.0-0",
    "libgtk-4-1",
    "pipewire",
    "tesseract-ocr",
]

for marker in required_control_markers:
    if marker not in control:
        raise SystemExit(f"Debian control is missing required marker: {marker}")

required_spec_markers = [
    "Requires:",
    "at-spi2-core",
    "gtk4",
    "libadwaita",
    "pipewire",
    "tesseract",
    "install -Dpm0644 packaging/deb/pepper-x.desktop",
    "install -Dpm0644 packaging/deb/pepper-x-autostart.desktop",
]

for marker in required_spec_markers:
    if marker not in spec:
        raise SystemExit(f"RPM spec is missing required marker: {marker}")

parser = ConfigParser(interpolation=None)
parser.optionxform = str
parser.read(deb_root / "pepper-x.desktop")
desktop = dict(parser["Desktop Entry"])
parser.read(deb_root / "pepper-x-autostart.desktop")
autostart = dict(parser["Desktop Entry"])

for field in ("Type", "Version", "Exec", "Icon", "Terminal"):
    if desktop[field] != autostart[field]:
        raise SystemExit(
            f"Desktop/autostart launch metadata diverged for {field}: "
            f"{desktop[field]!r} != {autostart[field]!r}"
        )
PY

"${repo_root}/scripts/verify-extension-install.sh"
