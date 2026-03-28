#!/usr/bin/env bash

set -euo pipefail

service_name="${PEPPERX_DBUS_SERVICE:-com.obra.PepperX}"
object_path="${PEPPERX_DBUS_OBJECT_PATH:-/com/obra/PepperX}"

if gdbus introspect --session --dest "${service_name}" --object-path "${object_path}" >/dev/null 2>&1; then
    exit 0
fi

echo "Pepper X D-Bus service is not reachable: ${service_name} ${object_path}" >&2
exit 1
