#!/usr/bin/env bash
set -euo pipefail

# Stable Ubuntu-to-Windows cross-build entry point.
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec "${script_dir}/build-windows-gnu.sh" "$@"
