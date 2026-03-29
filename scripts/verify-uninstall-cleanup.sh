#!/usr/bin/env bash

set -euo pipefail

usage() {
    echo "usage: $0 [PACKAGE.(deb|rpm)]" >&2
    exit 2
}

die() {
    echo "verify-uninstall-cleanup: $*" >&2
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

assert_dpkg_missing() {
    local root="$1"
    local admindir="$2"
    local package_name="$3"

    if dpkg-query --root="$root" --admindir="$admindir" -W -f '${Status}' "$package_name" >/dev/null 2>&1; then
        die "package should have been removed: $package_name"
    fi
}

assert_rpm_missing() {
    local root="$1"
    local dbpath="$2"
    local package_name="$3"

    if rpm --root="$root" --dbpath "$dbpath" -q "$package_name" >/dev/null 2>&1; then
        die "package should have been removed: $package_name"
    fi
}

verify_deb_uninstall() {
    local pkg="$1"
    require_command dpkg
    require_command dpkg-deb
    require_command dpkg-query

    local package_name
    package_name="$(dpkg-deb -f "$pkg" Package)"
    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN

    local root="$tmpdir/root"
    local admindir="$root/var/lib/dpkg"
    mkdir -p "$admindir/updates" "$admindir/info"

    dpkg --force-not-root --force-depends --root="$root" --admindir="$admindir" -i "$pkg" >/dev/null

    for path in \
        usr/bin/pepper-x \
        usr/share/applications/com.obra.PepperX.desktop \
        etc/xdg/autostart/pepper-x-autostart.desktop
    do
        assert_exists "$root/$path"
    done

    dpkg --force-not-root --root="$root" --admindir="$admindir" -r "$package_name" >/dev/null
    assert_dpkg_missing "$root" "$admindir" "$package_name"

    for path in \
        usr/bin/pepper-x \
        usr/share/applications/com.obra.PepperX.desktop \
        etc/xdg/autostart/pepper-x-autostart.desktop
    do
        assert_missing "$root/$path"
    done
}

verify_rpm_uninstall() {
    local pkg="$1"
    require_command rpm

    local package_name
    package_name="$(rpm -qp --queryformat '%{NAME}' "$pkg")"
    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN

    local root="$tmpdir/root"
    local dbpath="$root/var/lib/rpm"
    mkdir -p "$dbpath"

    rpm --root="$root" --dbpath "$dbpath" --nodeps -i "$pkg" >/dev/null

    for path in \
        usr/bin/pepper-x \
        usr/share/applications/com.obra.PepperX.desktop \
        etc/xdg/autostart/pepper-x-autostart.desktop
    do
        assert_exists "$root/$path"
    done

    rpm --root="$root" --dbpath "$dbpath" -e "$package_name" >/dev/null
    assert_rpm_missing "$root" "$dbpath" "$package_name"

    for path in \
        usr/bin/pepper-x \
        usr/share/applications/com.obra.PepperX.desktop \
        etc/xdg/autostart/pepper-x-autostart.desktop
    do
        assert_missing "$root/$path"
    done
}

verify_host_is_clean() {
    if command -v dpkg-query >/dev/null 2>&1; then
        if dpkg-query -W -f '${Status}' pepper-x 2>/dev/null | grep -q "install ok installed"; then
            die "pepper-x is still installed according to dpkg"
        fi
    fi

    if command -v rpm >/dev/null 2>&1; then
        if rpm -q pepper-x >/dev/null 2>&1; then
            die "pepper-x is still installed according to rpm"
        fi
    fi

    for path in \
        /usr/share/applications/com.obra.PepperX.desktop \
        /etc/xdg/autostart/pepper-x-autostart.desktop
    do
        assert_missing "$path"
    done
}

if [[ $# -eq 0 ]]; then
    verify_host_is_clean
elif [[ $# -eq 1 ]]; then
    package_path="$1"
    [[ -f "$package_path" ]] || die "missing package: $package_path"

    case "$package_path" in
        *.deb) verify_deb_uninstall "$package_path" ;;
        *.rpm) verify_rpm_uninstall "$package_path" ;;
        *) die "unsupported package format: $package_path" ;;
    esac
else
    usage
fi

echo "Uninstall cleanup check passed"
