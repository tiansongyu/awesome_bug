#!/usr/bin/env bash
set -euo pipefail

vm_name="cockroach-win11"
connection="qemu:///session"

state="$(virsh -c "${connection}" domstate "${vm_name}" 2>/dev/null || true)"
if [[ "${state}" != "running" ]]; then
    virsh -c "${connection}" start "${vm_name}"
fi

exec virt-viewer --connect "${connection}" --attach "${vm_name}"
