#!/usr/bin/env bash
set -euo pipefail

vm_name="${VM_NAME:-cockroach-win11}"
connection="${LIBVIRT_URI:-qemu:///session}"

state="$(virsh -c "${connection}" domstate "${vm_name}" 2>/dev/null || true)"
if [[ "${state}" == "running" ]]; then
    virsh -c "${connection}" shutdown "${vm_name}"
else
    echo "VM is not running."
fi
