#!/usr/bin/env bash

set -euo pipefail

usage() {
    echo "usage: $0 OLD.rpm NEW.rpm" >&2
    exit 2
}

die() {
    echo "verify-upgrade-fedora: $*" >&2
    exit 1
}

[[ $# -eq 2 ]] || usage

old_pkg="$1"
new_pkg="$2"

for pkg in "$old_pkg" "$new_pkg"; do
    [[ -f "$pkg" ]] || die "missing package: $pkg"
done

command -v rpm >/dev/null 2>&1 || die "rpm is required"
command -v rpm2cpio >/dev/null 2>&1 || die "rpm2cpio is required"
command -v cpio >/dev/null 2>&1 || die "cpio is required"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

old_root="$tmpdir/old"
new_root="$tmpdir/new"
mkdir -p "$old_root" "$new_root"

extract_rpm() {
    local pkg="$1"
    local dest="$2"
    (cd "$dest" && rpm2cpio "$pkg" | cpio -idmu --quiet)
}

extract_rpm "$old_pkg" "$old_root"
extract_rpm "$new_pkg" "$new_root"

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
    "usr/share/applications/com.obra.PepperX.desktop"
    "etc/xdg/autostart/pepper-x-autostart.desktop"
)

for path in "${required_paths[@]}"; do
    [[ -e "$old_root/$path" ]] || die "old package is missing $path"
    [[ -e "$new_root/$path" ]] || die "new package is missing $path"
done

echo "Fedora upgrade check passed: $old_name $old_version -> $new_version"
