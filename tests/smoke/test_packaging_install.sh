#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
workdir="$(mktemp -d)"

trap 'rm -rf "$workdir"' EXIT

mkdir -p "$workdir/bin" "$workdir/fixtures" "$workdir/logs" "$workdir/tmp"

export TMPDIR="$workdir/tmp"
export FAKE_LOG="$workdir/logs/calls.log"

create_package_fixture() {
    local path="$1"
    : > "$path"
}

write_python_shim() {
    local path="$1"
    shift

    cat > "$path" <<'PY'
#!/usr/bin/env python3
PY
    cat >> "$path" <<'PY'
import os
import pathlib
import shlex
import sys

LOG = pathlib.Path(os.environ["FAKE_LOG"])
LOG.parent.mkdir(parents=True, exist_ok=True)
command_name = pathlib.Path(sys.argv[0]).name
with LOG.open("a", encoding="utf-8") as handle:
    handle.write(shlex.join([command_name, *sys.argv[1:]]) + "\n")
PY
    cat >> "$path" <<'PY'

def package_name(path: str) -> str:
    return "pepper-x"


def package_version(path: str) -> str:
    name = pathlib.Path(path).name
    if "old" in name:
        if name.endswith(".deb"):
            return "1.0.0"
        return "1.0.0-1"
    if name.endswith(".deb"):
        return "1.1.0"
    return "1.1.0-1"


def required_payload_paths(root: pathlib.Path) -> list[pathlib.Path]:
    return [
        root / "usr/bin/pepper-x",
        root / "usr/share/applications/com.obra.PepperX.desktop",
        root / "etc/xdg/autostart/pepper-x-autostart.desktop",
        root / "usr/share/gnome-shell/extensions/pepperx@obra/metadata.json",
        root / "usr/share/gnome-shell/extensions/pepperx@obra/extension.js",
        root / "usr/share/gnome-shell/extensions/pepperx@obra/ipc.js",
        root / "usr/share/gnome-shell/extensions/pepperx@obra/keybindings.js",
        root / "usr/share/gnome-shell/extensions/pepperx@obra/README.md",
    ]


def ensure_payload(root: pathlib.Path) -> None:
    for path in required_payload_paths(root):
        path.parent.mkdir(parents=True, exist_ok=True)
        relative = path.relative_to(root).as_posix()
        if relative.endswith("metadata.json"):
            path.write_text(
                """{
  "uuid": "pepperx@obra",
  "name": "Pepper X",
  "description": "Thin GNOME Shell bridge for the Pepper X app-first shell.",
  "shell-version": ["48"],
  "url": "https://github.com/obra/pepper-x"
}
""",
                encoding="utf-8",
            )
        elif relative.endswith("extension.js"):
            path.write_text(
                "import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';\n"
                "export default class PepperXExtension extends Extension {\n"
                "    enable() {}\n"
                "    disable() {}\n"
                "    showSettings() {}\n"
                "}\n",
                encoding="utf-8",
            )
        elif relative.endswith("ipc.js"):
            path.write_text(
                "export function createPepperXClient() { return null; }\n"
                "export const SERVICE_NAME = 'com.obra.PepperX.Service';\n",
                encoding="utf-8",
            )
        elif relative.endswith("keybindings.js"):
            path.write_text(
                "export class KeybindingRegistry {\n"
                "    registerCleanup() {}\n"
                "    clear() {}\n"
                "}\n",
                encoding="utf-8",
            )
        elif relative.endswith("README.md"):
            path.write_text("# Pepper X GNOME Shell Extension\n", encoding="utf-8")
        else:
            path.write_text("fixture\n", encoding="utf-8")


def clear_payload(root: pathlib.Path) -> None:
    for path in required_payload_paths(root):
        if path.exists():
            path.unlink()


def read_state(path: pathlib.Path) -> str:
    return path.read_text(encoding="utf-8").strip()


def write_state(path: pathlib.Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value + "\n", encoding="utf-8")


def remove_state(path: pathlib.Path) -> None:
    if path.exists():
        path.unlink()


def parse_option(args: list[str], names: tuple[str, ...]) -> tuple[str | None, list[str]]:
    remaining: list[str] = []
    value: str | None = None
    index = 0
    while index < len(args):
        arg = args[index]
        if any(arg == name or arg.startswith(f"{name}=") for name in names):
            if "=" in arg:
                value = arg.split("=", 1)[1]
            else:
                index += 1
                value = args[index]
        else:
            remaining.append(arg)
        index += 1
    return value, remaining


def version_key(value: str) -> tuple:
    parts: list[object] = []
    for part in value.replace("-", ".").split("."):
        if part.isdigit():
            parts.append(int(part))
        else:
            parts.append(part)
    return tuple(parts)
PY
    cat >> "$path" <<'PY'

def main() -> int:
    args = sys.argv[1:]
    if not args:
        raise SystemExit(2)

    command = pathlib.Path(sys.argv[0]).name
    if command == "dpkg-deb":
        if args[0] == "-f":
            pkg = args[1]
            field = args[2]
            if field == "Package":
                print(package_name(pkg))
            elif field == "Version":
                print(package_version(pkg))
            else:
                raise SystemExit(f"unexpected dpkg-deb field: {field}")
            return 0

        if args[0] == "-x":
            root = pathlib.Path(args[2])
            ensure_payload(root)
            return 0

        raise SystemExit(f"unexpected dpkg-deb args: {args!r}")

    if command == "dpkg":
        if args and args[0] == "--compare-versions":
            left, op, right = args[1:4]
            if op == "gt":
                return 0 if version_key(left) > version_key(right) else 1
            if op == "lt":
                return 0 if version_key(left) < version_key(right) else 1
            if op == "eq":
                return 0 if version_key(left) == version_key(right) else 1
            raise SystemExit(f"unexpected dpkg compare op: {op}")

        root, remaining = parse_option(args, ("--root",))
        admindir, remaining = parse_option(remaining, ("--admindir",))
        root_path = pathlib.Path(root or ".")
        admindir_path = pathlib.Path(admindir or root_path / "var/lib/dpkg")

        if "-i" in remaining:
            pkg = remaining[remaining.index("-i") + 1]
            ensure_payload(root_path)
            write_state(admindir_path / "status", "install ok installed")
            write_state(admindir_path / "version", package_version(pkg))
            write_state(admindir_path / "name", package_name(pkg))
            return 0

        if "-r" in remaining:
            clear_payload(root_path)
            remove_state(admindir_path / "status")
            remove_state(admindir_path / "version")
            remove_state(admindir_path / "name")
            return 0

        raise SystemExit(f"unexpected dpkg args: {args!r}")

    if command == "dpkg-query":
        root, remaining = parse_option(args, ("--root",))
        admindir, remaining = parse_option(remaining, ("--admindir",))
        admindir_path = pathlib.Path(admindir or pathlib.Path(root or ".") / "var/lib/dpkg")

        if "-W" not in remaining:
            raise SystemExit(f"unexpected dpkg-query args: {args!r}")

        format_index = remaining.index("-f") if "-f" in remaining else remaining.index("--showformat")
        format_value = remaining[format_index + 1]
        package = remaining[-1]
        version_path = admindir_path / "version"
        status_path = admindir_path / "status"

        if not version_path.exists() or not status_path.exists():
            return 1

        if format_value == "${Status}" or format_value == "${Status}\n":
            print(read_state(status_path))
            return 0

        if format_value == "${Version}" or format_value == "${Version}\n":
            print(read_state(version_path))
            return 0

        raise SystemExit(f"unexpected dpkg-query format for {package}: {format_value!r}")

    if command == "rpm":
        root, remaining = parse_option(args, ("--root",))
        dbpath, remaining = parse_option(remaining, ("--dbpath",))
        root_path = pathlib.Path(root or ".")
        dbpath_path = pathlib.Path(dbpath or root_path / "var/lib/rpm")

        if "-qp" in remaining:
            pkg = remaining[-1]
            if "--queryformat" in remaining:
                fmt = remaining[remaining.index("--queryformat") + 1]
                if fmt == "%{NAME}":
                    print(package_name(pkg))
                    return 0
                if fmt == "%{VERSION}-%{RELEASE}":
                    print(package_version(pkg))
                    return 0
            raise SystemExit(f"unexpected rpm query args: {args!r}")

        if "-q" in remaining:
            state_path = dbpath_path / "version"
            if not state_path.exists():
                return 1
            if "--queryformat" in remaining:
                fmt = remaining[remaining.index("--queryformat") + 1]
                if fmt == "%{NAME}":
                    print("pepper-x")
                    return 0
                if fmt == "%{VERSION}-%{RELEASE}":
                    print(read_state(state_path))
                    return 0
            print("pepper-x")
            return 0

        if "-i" in remaining or "-U" in remaining:
            pkg = remaining[-1]
            ensure_payload(root_path)
            write_state(dbpath_path / "version", package_version(pkg))
            write_state(dbpath_path / "name", package_name(pkg))
            return 0

        if "-e" in remaining:
            clear_payload(root_path)
            remove_state(dbpath_path / "version")
            remove_state(dbpath_path / "name")
            return 0

        raise SystemExit(f"unexpected rpm args: {args!r}")

    raise SystemExit(f"unexpected command name: {command}")


if __name__ == "__main__":
    raise SystemExit(main())
PY

    chmod +x "$path"
}

create_package_fixture "$workdir/fixtures/pepper-x-old.deb"
create_package_fixture "$workdir/fixtures/pepper-x-new.deb"
create_package_fixture "$workdir/fixtures/pepper-x-old.rpm"
create_package_fixture "$workdir/fixtures/pepper-x-new.rpm"

write_python_shim "$workdir/bin/dpkg-deb"
write_python_shim "$workdir/bin/dpkg"
write_python_shim "$workdir/bin/dpkg-query"
write_python_shim "$workdir/bin/rpm"

run_with_shims() {
    PATH="$workdir/bin:$PATH" \
    bash "$@"
}

run_with_shims "$repo_root/scripts/verify-upgrade-ubuntu.sh" \
    "$workdir/fixtures/pepper-x-old.deb" \
    "$workdir/fixtures/pepper-x-new.deb"

run_with_shims "$repo_root/scripts/verify-upgrade-fedora.sh" \
    "$workdir/fixtures/pepper-x-old.rpm" \
    "$workdir/fixtures/pepper-x-new.rpm"

run_with_shims "$repo_root/scripts/verify-uninstall-cleanup.sh" \
    "$workdir/fixtures/pepper-x-old.deb"

run_with_shims "$repo_root/scripts/verify-uninstall-cleanup.sh" \
    "$workdir/fixtures/pepper-x-old.rpm"

python3 - "$FAKE_LOG" <<'PY'
from pathlib import Path
import sys

log = Path(sys.argv[1]).read_text()

required_fragments = [
    "dpkg ",
    "dpkg --force-not-root --force-depends",
    "dpkg -r pepper-x",
    "rpm ",
    "rpm --root",
    "rpm -e pepper-x",
    "--nodeps -i",
    "--nodeps -U",
]

for fragment in required_fragments:
    if fragment not in log:
        raise SystemExit(f"missing lifecycle command fragment: {fragment}")
PY

if find "$workdir/tmp" -type f | grep -Eq '(usr/bin/pepper-x|com.obra.PepperX.desktop|pepper-x-autostart.desktop|pepperx@obra/(metadata.json|extension.js|ipc.js|keybindings.js|README.md))'; then
    echo "packaging lifecycle smoke left packaged files behind in the temp root" >&2
    exit 1
fi

echo "Packaged install, upgrade, and uninstall helpers exercised real temp-root lifecycle commands."
