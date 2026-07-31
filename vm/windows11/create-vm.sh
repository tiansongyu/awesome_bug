#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
vm_name="${VM_NAME:-cockroach-win11}"
vm_state_dir="${VM_STATE_DIR:-${HOME}/VirtualMachines/${vm_name}}"
windows_iso="${WINDOWS_ISO:-}"
disk_image="${vm_state_dir}/windows11.qcow2"
test_tools_iso="${vm_state_dir}/cockroach-test-tools.iso"
package_archive="${project_dir}/dist/cockroach-overlay-windows-x64.zip"
package_checksum="${package_archive}.sha256"
connection="${LIBVIRT_URI:-qemu:///session}"
allow_reinstall="${ALLOW_REINSTALL:-0}"

if [[ "${allow_reinstall}" != "0" && "${allow_reinstall}" != "1" ]]; then
    echo "ALLOW_REINSTALL must be 0 or 1." >&2
    exit 1
fi

for program in virsh virt-install qemu-img genisoimage sha256sum swtpm unzip; do
    if ! command -v "${program}" >/dev/null 2>&1; then
        echo "Missing required program: ${program}" >&2
        exit 1
    fi
done

if [[ -z "${windows_iso}" || ! -r "${windows_iso}" ]]; then
    echo "Set WINDOWS_ISO to a readable Windows 11 x64 ISO." >&2
    echo "Windows ISO is not readable: ${windows_iso}" >&2
    exit 1
fi
if [[ ! -f "${package_archive}" || ! -f "${package_checksum}" ]]; then
    echo "Build the Windows package first: ./scripts/build-windows.sh" >&2
    exit 1
fi
(
    cd -- "${project_dir}/dist"
    sha256sum --check -- "$(basename -- "${package_checksum}")"
)

mkdir -p "${vm_state_dir}"

stage_dir="$(mktemp -d)"
cleanup() {
    rm -rf -- "${stage_dir}"
}
trap cleanup EXIT

install -m 0644 "${project_dir}/vm/windows11/autounattend.xml" \
    "${stage_dir}/Autounattend.xml"
unzip -q "${package_archive}" -d "${stage_dir}/package"
payload_dir="${stage_dir}/package/windows-x64"
for required in cockroach_overlay.exe cockroach_swarm_20.exe SDL2.dll bugs; do
    if [[ ! -e "${payload_dir}/${required}" ]]; then
        echo "Windows package is incomplete: ${required}" >&2
        exit 1
    fi
done
cp -a "${payload_dir}/." "${stage_dir}/"
rm -rf -- "${stage_dir}/package"
genisoimage -quiet -J -R -V COCKROACH_TEST \
    -o "${test_tools_iso}.new" "${stage_dir}"
mv -f -- "${test_tools_iso}.new" "${test_tools_iso}"

if virsh -c "${connection}" dominfo "${vm_name}" >/dev/null 2>&1; then
    echo "VM ${vm_name} already exists."
    echo "Run: ${project_dir}/vm/windows11/start-vm.sh"
    exit 0
fi

if [[ -e "${disk_image}" || -L "${disk_image}" ]]; then
    if [[ ! -f "${disk_image}" || -L "${disk_image}" ]]; then
        echo "Refusing to use a non-regular VM disk: ${disk_image}" >&2
        exit 1
    fi
    if [[ "${allow_reinstall}" != "1" ]]; then
        echo "Refusing to reuse the existing VM disk: ${disk_image}" >&2
        echo "Windows Setup is configured to erase its installation disk." >&2
        echo "To intentionally reinstall, rerun with ALLOW_REINSTALL=1." >&2
        exit 1
    fi
    echo "WARNING: reinstalling Windows may erase the existing disk:"
    echo "  ${disk_image}"
else
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
