#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "${repo_root}"

service_name="${PEPPERX_DBUS_SERVICE:-com.obra.PepperX.Service}"
object_path="${PEPPERX_DBUS_OBJECT_PATH:-/com/obra/PepperX}"
interface_name="${PEPPERX_DBUS_INTERFACE:-com.obra.PepperX}"

if ! command -v cargo >/dev/null 2>&1 && [[ -f "${HOME}/.cargo/env" ]]; then
    . "${HOME}/.cargo/env"
fi

if ! cargo build -p pepper-x-app >/tmp/pepperx-build.log 2>&1; then
    cat /tmp/pepperx-build.log >&2
    exit 1
fi

dbus-run-session -- bash -s <<EOF
set -euo pipefail

cd "${repo_root}"
PEPPERX_HEADLESS=1 ./scripts/dev-run-app.sh >/tmp/pepperx-app.log 2>&1 &
app_pid=\$!

cleanup() {
    kill "\${app_pid}" >/dev/null 2>&1 || true
    wait "\${app_pid}" 2>/dev/null || true
}

trap cleanup EXIT

for _ in \$(seq 1 50); do
    if gdbus introspect --session --dest "${service_name}" --object-path "${object_path}" >/dev/null 2>&1; then
        break
    fi

    sleep 0.2
done

if ! gdbus introspect --session --dest "${service_name}" --object-path "${object_path}" >/dev/null 2>&1; then
    echo "Pepper X D-Bus service is not reachable: ${service_name} ${object_path}" >&2
    cat /tmp/pepperx-app.log >&2 || true
    exit 1
fi

reply="\$(gdbus call --session --dest "${service_name}" --object-path "${object_path}" --method "${interface_name}.Ping")"
if [[ "\${reply}" != *"pong"* ]]; then
    echo "Pepper X Ping returned an unexpected reply: \${reply}" >&2
    exit 1
fi

capabilities="\$(gdbus call --session --dest "${service_name}" --object-path "${object_path}" --method "${interface_name}.GetCapabilities")"
if [[ "\${capabilities}" != *"false"* ]]; then
    echo "Pepper X capabilities unexpectedly reported modifier-only support in headless mode: \${capabilities}" >&2
    exit 1
fi
EOF
