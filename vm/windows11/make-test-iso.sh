#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd -- "${script_dir}/../.." && pwd)"
package_archive="${project_dir}/dist/cockroach-overlay-windows-x64.zip"
package_checksum="${package_archive}.sha256"
output="${1:-${project_dir}/dist/rust-bug-windows11-test.iso}"

for program in genisoimage install mktemp sha256sum unzip; do
    if ! command -v "${program}" >/dev/null 2>&1; then
        echo "Missing required command: ${program}" >&2
        exit 1
    fi
done

for required in "${package_archive}" "${package_checksum}"; do
    if [[ ! -f "${required}" ]]; then
        echo "Required test payload is missing: ${required}" >&2
        exit 1
    fi
done
(
    cd -- "${project_dir}/dist"
    sha256sum --check -- "$(basename -- "${package_checksum}")"
)

stage="$(mktemp -d "${TMPDIR:-/tmp}/rust-bug-vm-iso.XXXXXXXX")"
trap 'rm -rf -- "${stage}"' EXIT

unzip -q "${package_archive}" -d "${stage}/package"
payload_dir="${stage}/package/windows-x64"
for required in cockroach_overlay.exe cockroach_swarm_20.exe SDL2.dll bugs; do
    if [[ ! -e "${payload_dir}/${required}" ]]; then
        echo "Windows package is incomplete: ${required}" >&2
        exit 1
    fi
done
cp -a "${payload_dir}/." "${stage}/"
rm -rf -- "${stage}/package"
install -m 0644 "${script_dir}/run-rust-smoke.ps1" "${stage}/"
install -m 0644 "${script_dir}/run-rust-smoke.cmd" "${stage}/"
install -m 0644 "${script_dir}/run-interaction-probe.ps1" "${stage}/"
install -m 0644 "${script_dir}/run-interaction-probe.cmd" "${stage}/"
install -m 0644 "${script_dir}/run-bait-trace.ps1" "${stage}/"
install -m 0644 "${script_dir}/run-bait-trace.cmd" "${stage}/"
install -m 0644 "${script_dir}/run-single-live.cmd" "${stage}/"
install -m 0644 "${script_dir}/run-swarm-live.cmd" "${stage}/"

mkdir -p -- "$(dirname -- "${output}")"
genisoimage -quiet -J -R -V RUSTBUG -o "${output}" "${stage}"
echo "Windows 11 test ISO: ${output}"
