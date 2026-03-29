#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"

usage() {
    echo "usage: $0 OLD.rpm NEW.rpm" >&2
    exit 2
}

die() {
    echo "verify-upgrade-fedora: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "$1 is required"
}

assert_exists() {
    local path="$1"

    [[ -e "$path" ]] || die "missing expected path: $path"
}

assert_missing() {
    local path="$1"

    [[ ! -e "$path" ]] || die "unexpected leftover path: $path"
}

assert_package_installed() {
    local root="$1"
    local dbpath="$2"
    local package_name="$3"

    rpm --root="$root" --dbpath "$dbpath" -q "$package_name" >/dev/null 2>&1 \
        || die "package is not installed: $package_name"
}

assert_package_missing() {
    local root="$1"
    local dbpath="$2"
    local package_name="$3"

    if rpm --root="$root" --dbpath "$dbpath" -q "$package_name" >/dev/null 2>&1; then
        die "package should have been removed: $package_name"
    fi
}

assert_package_version() {
    local root="$1"
    local dbpath="$2"
    local package_name="$3"
    local expected_version="$4"

    local version
    version="$(
        rpm --root="$root" --dbpath "$dbpath" -q --queryformat '%{VERSION}-%{RELEASE}' "$package_name"
    )"

    [[ "$version" == "$expected_version" ]] || die "unexpected package version for $package_name: $version"
}

install_package() {
    local root="$1"
    local dbpath="$2"
    local pkg="$3"

    rpm --root="$root" --dbpath "$dbpath" --nodeps -i "$pkg" >/dev/null
}

upgrade_package() {
    local root="$1"
    local dbpath="$2"
    local pkg="$3"

    rpm --root="$root" --dbpath "$dbpath" --nodeps -U "$pkg" >/dev/null
}

remove_package() {
    local root="$1"
    local dbpath="$2"
    local package_name="$3"

    rpm --root="$root" --dbpath "$dbpath" -e "$package_name" >/dev/null
}

[[ $# -eq 2 ]] || usage

old_pkg="$1"
new_pkg="$2"

for pkg in "$old_pkg" "$new_pkg"; do
    [[ -f "$pkg" ]] || die "missing package: $pkg"
done

require_command rpm

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

old_name="$(rpm -qp --queryformat '%{NAME}' "$old_pkg")"
new_name="$(rpm -qp --queryformat '%{NAME}' "$new_pkg")"
[[ "$old_name" == "$new_name" ]] || die "package names differ: $old_name vs $new_name"

old_version="$(rpm -qp --queryformat '%{VERSION}-%{RELEASE}' "$old_pkg")"
new_version="$(rpm -qp --queryformat '%{VERSION}-%{RELEASE}' "$new_pkg")"
if [[ "$old_version" == "$new_version" ]]; then
    die "new package version must differ from old version: $old_version"
fi

required_paths=(
    "usr/bin/pepper-x"
    "usr/libexec/pepper-x/pepperx-uinput-helper"
    "usr/share/applications/com.obra.PepperX.desktop"
    "etc/xdg/autostart/pepper-x-autostart.desktop"
    "usr/share/gnome-shell/extensions/pepperx@obra/metadata.json"
    "usr/share/gnome-shell/extensions/pepperx@obra/extension.js"
    "usr/share/gnome-shell/extensions/pepperx@obra/ipc.js"
    "usr/share/gnome-shell/extensions/pepperx@obra/keybindings.js"
    "usr/share/gnome-shell/extensions/pepperx@obra/README.md"
)

root="$tmpdir/root"
dbpath="$root/var/lib/rpm"
mkdir -p "$dbpath"

install_package "$root" "$dbpath" "$old_pkg"
assert_package_installed "$root" "$dbpath" "$old_name"
assert_package_version "$root" "$dbpath" "$old_name" "$old_version"

for path in "${required_paths[@]}"; do
    assert_exists "$root/$path"
done
"${script_dir}/verify-extension-install.sh" "$root"

upgrade_package "$root" "$dbpath" "$new_pkg"
assert_package_installed "$root" "$dbpath" "$new_name"
assert_package_version "$root" "$dbpath" "$new_name" "$new_version"

for path in "${required_paths[@]}"; do
    assert_exists "$root/$path"
done
"${script_dir}/verify-extension-install.sh" "$root"

remove_package "$root" "$dbpath" "$new_name"
assert_package_missing "$root" "$dbpath" "$new_name"

for path in "${required_paths[@]}"; do
    assert_missing "$root/$path"
done

echo "Fedora upgrade check passed: $old_name $old_version -> $new_version"
