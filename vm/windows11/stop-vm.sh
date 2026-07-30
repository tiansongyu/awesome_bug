#!/usr/bin/env bash
set -euo pipefail

vm_name="cockroach-win11"
connection="qemu:///session"

state="$(virsh -c "${connection}" domstate "${vm_name}" 2>/dev/null || true)"
if [[ "${state}" == "running" ]]; then
    virsh -c "${connection}" shutdown "${vm_name}"
else
    echo "VM is not running."
fi
