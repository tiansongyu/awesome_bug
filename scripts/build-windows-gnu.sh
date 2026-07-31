#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd -- "${script_dir}/.." && pwd)"
dependency_dir="${project_dir}/build-windows-deps"
download_dir="${dependency_dir}/downloads"
source_dir="${dependency_dir}/sources"
target="x86_64-pc-windows-gnu"

sdl_version="2.32.10"
sdl_archive="SDL2-devel-${sdl_version}-mingw.tar.gz"
sdl_sha256="83a5d74012311edc3c0d40ea6faecbe57ad692aa033fa5dc273cc937e3938ff2"
sdl_url="https://github.com/libsdl-org/SDL/releases/download/release-${sdl_version}/${sdl_archive}"
sdl_root="${source_dir}/SDL2-${sdl_version}/x86_64-w64-mingw32"

run_tests=true
run_smoke=false
ui_smoke=false
while (($# > 0)); do
    case "$1" in
        --skip-tests)
            run_tests=false
            ;;
        --skip-smoke)
            run_smoke=false
            ;;
        --wine-smoke)
            run_smoke=true
            ;;
        --ui-smoke)
            ui_smoke=true
            ;;
        --help|-h)
            echo "Usage: $0 [--skip-tests] [--wine-smoke] [--ui-smoke]"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 2
            ;;
    esac
    shift
done

required_programs=(
    cargo
    curl
    find
    install
    rustc
    rustup
    sha256sum
    sort
    tar
    touch
    x86_64-w64-mingw32-gcc
    x86_64-w64-mingw32-windres
    zip
)
for program in "${required_programs[@]}"; do
    if ! command -v "${program}" >/dev/null 2>&1; then
        echo "Missing required command: ${program}" >&2
        exit 1
    fi
done
if [[ "${ui_smoke}" == true ]] && ! command -v timeout >/dev/null 2>&1; then
    echo "Missing required command for --ui-smoke: timeout" >&2
    exit 1
fi

wine_command=""
if command -v wine >/dev/null 2>&1; then
    wine_command="$(command -v wine)"
elif command -v wine64 >/dev/null 2>&1; then
    wine_command="$(command -v wine64)"
elif [[ "${run_smoke}" == true || "${ui_smoke}" == true ]]; then
    echo "Wine is required for the requested smoke check." >&2
    exit 1
fi

active_rust_version="$(rustc --version)"
if [[ "${active_rust_version}" != rustc\ 1.97.1\ * ]]; then
    echo "Rust 1.97.1 is required; active toolchain: ${active_rust_version}" >&2
    exit 1
fi
if ! rustup target list --installed | grep -Fx "${target}" >/dev/null; then
    echo "Missing Rust target ${target}; install it with:" >&2
    echo "  rustup target add ${target} --toolchain 1.97.1" >&2
    exit 1
fi

mkdir -p -- "${download_dir}" "${source_dir}"
archive_path="${download_dir}/${sdl_archive}"
if [[ ! -f "${archive_path}" ]]; then
    temporary_archive="${archive_path}.download.$$"
    trap 'rm -f -- "${temporary_archive:-}"' EXIT
    curl --fail --location --retry 3 --proto '=https' --tlsv1.2 \
        --output "${temporary_archive}" "${sdl_url}"
    printf '%s  %s\n' "${sdl_sha256}" "${temporary_archive}" |
        sha256sum --check -
    mv -- "${temporary_archive}" "${archive_path}"
    trap - EXIT
fi
printf '%s  %s\n' "${sdl_sha256}" "${archive_path}" | sha256sum --check -

if [[ ! -d "${sdl_root}" ]]; then
    tar --extract --gzip --file "${archive_path}" \
        --directory "${source_dir}" --no-same-owner
fi

sdl_library_dir="${sdl_root}/lib"
sdl_include_dir="${sdl_root}/include/SDL2"
sdl_dll="${sdl_root}/bin/SDL2.dll"
for required_path in \
    "${sdl_library_dir}/libSDL2.dll.a" \
    "${sdl_include_dir}/SDL.h" \
    "${sdl_dll}"; do
    if [[ ! -f "${required_path}" ]]; then
        echo "Incomplete SDL2 ${sdl_version} tree: ${required_path} is missing" >&2
        exit 1
    fi
done

export SDL2_LIB_DIR="${sdl_library_dir}"
export SDL2_INCLUDE_PATH="${sdl_include_dir}"
if [[ -n "${wine_command}" ]]; then
    export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER="${wine_command}"
fi

cd -- "${project_dir}"
if [[ "${run_tests}" == true ]]; then
    cargo test -p bug-runtime --locked
    cargo test \
        -p bug-windows \
        --all-targets \
        --target "${target}" \
        --no-run \
        --locked
fi
cargo build -p bug-windows --bins --release --target "${target}" --locked

release_dir="${project_dir}/target/${target}/release"
for executable in cockroach_overlay.exe cockroach_swarm_20.exe; do
    if [[ ! -s "${release_dir}/${executable}" ]]; then
        echo "Rust did not produce ${release_dir}/${executable}" >&2
        exit 1
    fi
done

temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/bug-overlay-package.XXXXXXXX")"
trap 'rm -rf -- "${temporary_root}"' EXIT
payload_dir="${temporary_root}/windows-x64"
working_dir="${temporary_root}/unrelated working directory"
mkdir -p -- "${payload_dir}" "${working_dir}"

install -m 0755 "${release_dir}/cockroach_overlay.exe" "${payload_dir}/"
install -m 0755 "${release_dir}/cockroach_swarm_20.exe" "${payload_dir}/"
install -m 0755 "${sdl_dll}" "${payload_dir}/SDL2.dll"
install -m 0644 "${project_dir}/packaging/WINDOWS-README.txt" \
    "${payload_dir}/README.txt"
install -m 0644 "${project_dir}/LICENSE" "${payload_dir}/LICENSE"
install -m 0644 "${project_dir}/ASSET-NOTICE.md" \
    "${payload_dir}/ASSET-NOTICE.md"
install -m 0644 "${project_dir}/packaging/THIRD_PARTY_LICENSES.txt" \
    "${payload_dir}/THIRD_PARTY_LICENSES.txt"

mkdir -p -- "${payload_dir}/bugs"
for package in runtime cockroach template; do
    if [[ ! -d "${project_dir}/bugs/${package}" ]]; then
        echo "Required species package is missing: bugs/${package}" >&2
        exit 1
    fi
    cp -R -- "${project_dir}/bugs/${package}" "${payload_dir}/bugs/${package}"
done

(
    cd -- "${payload_dir}"
    find . -type f ! -name SHA256SUMS.txt -print0 |
        LC_ALL=C sort -z |
        xargs -0 sha256sum |
        sed 's#  \./#  #'
) > "${payload_dir}/SHA256SUMS.txt"

if [[ "${run_smoke}" == true ]]; then
    (
        cd -- "${working_dir}"
        WINEDEBUG=-all "${wine_command}" \
            "${payload_dir}/cockroach_overlay.exe" --help
        WINEDEBUG=-all "${wine_command}" \
            "${payload_dir}/cockroach_swarm_20.exe" --help
    )
fi
if [[ "${ui_smoke}" == true ]]; then
    if [[ -z "${wine_command}" ]]; then
        echo "--ui-smoke requires Wine." >&2
        exit 1
    fi
    (
        cd -- "${working_dir}"
        WINEDEBUG=-all timeout 30s "${wine_command}" \
            "${payload_dir}/cockroach_overlay.exe" \
            --frames 3 --seed 1
        WINEDEBUG=-all timeout 30s "${wine_command}" \
            "${payload_dir}/cockroach_swarm_20.exe" \
            --frames 3 --seed 1
    )
fi

# Normalize input mtimes and omit host-specific ZIP metadata.  The sorted file
# list makes identical sources produce an identical archive.
find "${payload_dir}" -type f -exec touch -t 198001010000 {} +
archive_in_stage="${temporary_root}/cockroach-overlay-windows-x64.zip"
(
    cd -- "${temporary_root}"
    find windows-x64 -type f -print |
        LC_ALL=C sort |
        zip -X -q "${archive_in_stage}" -@
)

dist_dir="${project_dir}/dist"
archive="${dist_dir}/cockroach-overlay-windows-x64.zip"
mkdir -p -- "${dist_dir}"
install -m 0644 "${archive_in_stage}" "${archive}"
archive_hash="$(sha256sum "${archive}" | cut -d ' ' -f 1)"
printf '%s  %s\n' "${archive_hash}" "$(basename "${archive}")" \
    > "${archive}.sha256"

echo "GNU Windows package: ${archive}"
echo "SHA-256: ${archive_hash}"
echo "SDL2 archive SHA-256: ${sdl_sha256}"
