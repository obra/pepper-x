#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"

usage() {
    echo "usage: $0 OLD.deb NEW.deb" >&2
    exit 2
}

die() {
    echo "verify-upgrade-ubuntu: $*" >&2
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

assert_package_status() {
    local root="$1"
    local admindir="$2"
    local package_name="$3"
    local expected_status="$4"

    local status
    status="$(
        dpkg-query \
            --root="$root" \
            --admindir="$admindir" \
            -W -f '${Status}' \
            "$package_name"
    )"

    [[ "$status" == "$expected_status" ]] || die "unexpected package status for $package_name: $status"
}

assert_package_version() {
    local root="$1"
    local admindir="$2"
    local package_name="$3"
    local expected_version="$4"

    local version
    version="$(
        dpkg-query \
            --root="$root" \
            --admindir="$admindir" \
            -W -f '${Version}' \
            "$package_name"
    )"

    [[ "$version" == "$expected_version" ]] || die "unexpected package version for $package_name: $version"
}

install_package() {
    local root="$1"
    local admindir="$2"
    local pkg="$3"

    dpkg \
        --force-not-root \
        --force-depends \
        --root="$root" \
        --admindir="$admindir" \
        -i "$pkg" \
        >/dev/null
}

remove_package() {
    local root="$1"
    local admindir="$2"
    local package_name="$3"

    dpkg \
        --force-not-root \
        --root="$root" \
        --admindir="$admindir" \
        -r "$package_name" \
        >/dev/null
}

[[ $# -eq 2 ]] || usage

old_pkg="$1"
new_pkg="$2"

for pkg in "$old_pkg" "$new_pkg"; do
    [[ -f "$pkg" ]] || die "missing package: $pkg"
done

require_command dpkg-deb
require_command dpkg
require_command dpkg-query

old_name="$(dpkg-deb -f "$old_pkg" Package)"
new_name="$(dpkg-deb -f "$new_pkg" Package)"
[[ "$old_name" == "$new_name" ]] || die "package names differ: $old_name vs $new_name"

old_version="$(dpkg-deb -f "$old_pkg" Version)"
new_version="$(dpkg-deb -f "$new_pkg" Version)"
if ! dpkg --compare-versions "$new_version" gt "$old_version"; then
    die "new version must be greater than old version: $old_version -> $new_version"
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

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

root="$tmpdir/root"
admindir="$root/var/lib/dpkg"
mkdir -p "$admindir/updates" "$admindir/info"

install_package "$root" "$admindir" "$old_pkg"
assert_package_status "$root" "$admindir" "$old_name" "install ok installed"
assert_package_version "$root" "$admindir" "$old_name" "$old_version"

for path in "${required_paths[@]}"; do
    assert_exists "$root/$path"
done
"${script_dir}/verify-extension-install.sh" "$root"

install_package "$root" "$admindir" "$new_pkg"
assert_package_status "$root" "$admindir" "$new_name" "install ok installed"
assert_package_version "$root" "$admindir" "$new_name" "$new_version"

for path in "${required_paths[@]}"; do
    assert_exists "$root/$path"
done
"${script_dir}/verify-extension-install.sh" "$root"

remove_package "$root" "$admindir" "$new_name"

for path in "${required_paths[@]}"; do
    assert_missing "$root/$path"
done

if dpkg-query \
    --root="$root" \
    --admindir="$admindir" \
    -W -f '${Status}' \
    "$new_name" >/dev/null 2>&1; then
    die "package should have been removed: $new_name"
fi

echo "Ubuntu upgrade check passed: $old_name $old_version -> $new_version"
