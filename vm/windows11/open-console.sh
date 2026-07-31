#!/usr/bin/env bash
set -euo pipefail

vm_name="${VM_NAME:-cockroach-win11}"
connection="${LIBVIRT_URI:-qemu:///session}"

exec virt-viewer --connect "${connection}" --attach "${vm_name}"
