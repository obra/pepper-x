#!/usr/bin/env bash

set -euo pipefail

usage() {
    echo "usage: $0 OLD.deb NEW.deb" >&2
    exit 2
}

die() {
    echo "verify-upgrade-ubuntu: $*" >&2
    exit 1
}

[[ $# -eq 2 ]] || usage

old_pkg="$1"
new_pkg="$2"

for pkg in "$old_pkg" "$new_pkg"; do
    [[ -f "$pkg" ]] || die "missing package: $pkg"
done

command -v dpkg-deb >/dev/null 2>&1 || die "dpkg-deb is required"
command -v dpkg >/dev/null 2>&1 || die "dpkg is required"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

old_root="$tmpdir/old"
new_root="$tmpdir/new"
mkdir -p "$old_root" "$new_root"

dpkg-deb -x "$old_pkg" "$old_root"
dpkg-deb -x "$new_pkg" "$new_root"

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
    "usr/share/applications/com.obra.PepperX.desktop"
    "etc/xdg/autostart/pepper-x-autostart.desktop"
)

for path in "${required_paths[@]}"; do
    [[ -e "$old_root/$path" ]] || die "old package is missing $path"
    [[ -e "$new_root/$path" ]] || die "new package is missing $path"
done

echo "Ubuntu upgrade check passed: $old_name $old_version -> $new_version"
