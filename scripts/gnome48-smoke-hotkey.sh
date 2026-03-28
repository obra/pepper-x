#!/usr/bin/env bash

set -euo pipefail

service_name="${PEPPERX_DBUS_SERVICE:-com.obra.PepperX.Service}"
object_path="${PEPPERX_DBUS_OBJECT_PATH:-/com/obra/PepperX}"
interface_name="${PEPPERX_DBUS_INTERFACE:-com.obra.PepperX}"

if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]]; then
    echo "Run this helper inside the live GNOME session or export that session's DBUS_SESSION_BUS_ADDRESS first" >&2
    exit 1
fi

capabilities="$(
    gdbus call \
        --session \
        --dest "${service_name}" \
        --object-path "${object_path}" \
        --method "${interface_name}.GetCapabilities"
)"

if [[ "${capabilities}" != *"(true,"* && "${capabilities}" != *"(true, true,"* ]]; then
    echo "Pepper X modifier-only capture is not available in the live GNOME session: ${capabilities}" >&2
    exit 1
fi

cat <<EOF
Pepper X live GNOME 48 smoke prerequisites passed.

- Capabilities: ${capabilities}

Next step:
- Press and release the configured Control key on a physical keyboard.
- Confirm the app log shows:
  [Pepper X] modifier-only start
  [Pepper X] modifier-only stop

Note:
- The modifier watcher is in-process via the app-owned AT-SPI backend; there is no separate Pepper X keyboard monitor D-Bus name to inspect.
- Remote injectors such as QEMU send-key and VNC/noVNC are not authoritative for this stack.
EOF
