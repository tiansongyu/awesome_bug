#!/usr/bin/env bash
set -euo pipefail

exec virt-viewer --connect qemu:///session --attach cockroach-win11
