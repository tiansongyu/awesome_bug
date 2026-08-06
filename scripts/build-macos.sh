#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd -- "${script_dir}/.." && pwd)"
cd "${repository_root}"

case "$(uname -m)" in
    arm64) package_arch="arm64" ;;
    x86_64) package_arch="x64" ;;
    *)
        echo "unsupported macOS architecture: $(uname -m)" >&2
        exit 1
        ;;
esac

source "${HOME}/.cargo/env" 2>/dev/null || true
command -v cargo >/dev/null 2>&1 || {
    echo "cargo is required; install Rust 1.97.1 with rustup first" >&2
    exit 1
}

version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
if [[ -z "${version}" ]]; then
    echo "cannot read workspace package version" >&2
    exit 1
fi

cargo fmt --all --check
cargo test --workspace --lib --locked
cargo clippy -p bug-runtime --all-targets --locked -- -D warnings
cargo clippy -p bug-windows --all-targets --locked -- -D warnings
cargo build -p bug-windows --release --bins --locked

staging_directory="$(mktemp -d "${TMPDIR:-/tmp}/awesome-bug-macos.XXXXXX")"
trap 'rm -rf "${staging_directory}"' EXIT

package_name="cockroach-overlay-macos-${package_arch}"
package_root="${staging_directory}/${package_name}"
mkdir -p "${package_root}"

make_app() {
    local app_name="$1"
    local executable="$2"
    local bundle_id="$3"
    local app_root="${package_root}/${app_name}.app"

    mkdir -p "${app_root}/Contents/MacOS" "${app_root}/Contents/Resources"
    install -m 755 "target/release/${executable}" "${app_root}/Contents/MacOS/${executable}"
    cp -R bugs "${app_root}/Contents/Resources/bugs"
    sed \
        -e "s|@APP_NAME@|${app_name}|g" \
        -e "s|@EXECUTABLE@|${executable}|g" \
        -e "s|@BUNDLE_ID@|${bundle_id}|g" \
        -e "s|@VERSION@|${version}|g" \
        packaging/macos/Info.plist.in >"${app_root}/Contents/Info.plist"
    plutil -lint "${app_root}/Contents/Info.plist" >/dev/null
    codesign --force --deep --sign - --timestamp=none "${app_root}"
}

make_app "Cockroach Overlay" "cockroach_overlay" "com.tiansongyu.awesome-bug.overlay"
make_app "Cockroach Swarm 20" "cockroach_swarm_20" "com.tiansongyu.awesome-bug.swarm20"

cp packaging/MACOS-README.txt "${package_root}/README.txt"
cp packaging/THIRD_PARTY_LICENSES.txt "${package_root}/THIRD_PARTY_LICENSES.txt"
cp ASSET-NOTICE.md LICENSE "${package_root}/"

mkdir -p dist
archive="dist/${package_name}.zip"
ditto -c -k --sequesterRsrc --keepParent "${package_root}" "${archive}"
shasum -a 256 "${archive}" >"${archive}.sha256"

echo "created ${archive}"
echo "created ${archive}.sha256"
