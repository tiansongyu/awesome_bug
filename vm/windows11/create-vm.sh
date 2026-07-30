#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
vm_name="cockroach-win11"
vm_state_dir="/home/ubuntu/VirtualMachines/cockroach-win11"
windows_iso="/media/ubuntu/Ventoy/Win11_25H2_Chinese_Simplified_x64.iso"
disk_image="${vm_state_dir}/windows11.qcow2"
test_tools_iso="${vm_state_dir}/cockroach-test-tools.iso"
connection="qemu:///session"

for program in virsh virt-install qemu-img genisoimage swtpm; do
    if ! command -v "${program}" >/dev/null 2>&1; then
        echo "Missing required program: ${program}" >&2
        exit 1
    fi
done

if [[ ! -r "${windows_iso}" ]]; then
    echo "Windows ISO is not readable: ${windows_iso}" >&2
    exit 1
fi
if [[ ! -f "${project_dir}/dist/windows-x64/cockroach_overlay.exe" ]]; then
    echo "Build the Windows package first: ./scripts/build-windows.sh" >&2
    exit 1
fi

mkdir -p "${vm_state_dir}"

stage_dir="$(mktemp -d)"
cleanup() {
    rm -rf -- "${stage_dir}"
}
trap cleanup EXIT

install -m 0644 "${project_dir}/vm/windows11/autounattend.xml" \
    "${stage_dir}/Autounattend.xml"
cp -a "${project_dir}/dist/windows-x64/." "${stage_dir}/"
genisoimage -quiet -J -R -V COCKROACH_TEST \
    -o "${test_tools_iso}.new" "${stage_dir}"
mv -f -- "${test_tools_iso}.new" "${test_tools_iso}"

if virsh -c "${connection}" dominfo "${vm_name}" >/dev/null 2>&1; then
    echo "VM ${vm_name} already exists."
    echo "Run: ${project_dir}/vm/windows11/start-vm.sh"
    exit 0
fi

if [[ ! -f "${disk_image}" ]]; then
    qemu-img create -f qcow2 -o lazy_refcounts=on \
        "${disk_image}" 100G
fi

virt-install \
    --connect "${connection}" \
    --name "${vm_name}" \
    --description "Windows 11 cockroach desktop-pet test VM" \
    --memory 12288 \
    --vcpus 8 \
    --cpu host-passthrough \
    --machine q35 \
    --features smm.state=on,hyperv.relaxed.state=on,hyperv.vapic.state=on,hyperv.spinlocks.state=on,hyperv.spinlocks.retries=8191 \
    --clock offset=localtime,hypervclock_present=yes \
    --boot loader=/usr/share/OVMF/OVMF_CODE_4M.secboot.fd,loader.readonly=yes,loader.type=pflash,nvram.template=/usr/share/OVMF/OVMF_VARS_4M.ms.fd \
    --tpm backend.type=emulator,backend.version=2.0,model=tpm-crb \
    --disk "path=${disk_image},format=qcow2,bus=sata,cache=writeback,discard=unmap" \
    --cdrom "${windows_iso}" \
    --disk "path=${test_tools_iso},device=cdrom,bus=sata,readonly=on" \
    --network user,model=e1000e \
    --graphics spice,listen=none,clipboard.copypaste=yes \
    --video qxl \
    --controller usb,model=qemu-xhci \
    --input tablet,bus=usb \
    --channel spicevmc \
    --sound ich9 \
    --osinfo win11 \
    --noautoconsole

sleep 1
virsh -c "${connection}" send-key "${vm_name}" KEY_ENTER || true

echo "VM created and Windows setup started."
echo "Open its display with: ${project_dir}/vm/windows11/open-console.sh"
