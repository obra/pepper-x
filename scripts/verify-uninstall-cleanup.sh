#!/usr/bin/env bash

set -euo pipefail

die() {
    echo "verify-uninstall-cleanup: $*" >&2
    exit 1
}

if command -v dpkg-query >/dev/null 2>&1; then
    if dpkg-query -W -f='${Status}' pepper-x 2>/dev/null | grep -q "install ok installed"; then
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
    [[ ! -e "$path" ]] || die "uninstall should remove $path"
done

echo "Uninstall cleanup check passed"
