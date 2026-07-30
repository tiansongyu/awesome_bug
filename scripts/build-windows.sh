#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd -- "${script_dir}/.." && pwd)"
dependency_dir="${project_dir}/build-windows-deps"
download_dir="${dependency_dir}/downloads"
source_dir="${dependency_dir}/sources"
libpng_build_dir="${dependency_dir}/libpng-build"
libpng_install_dir="${dependency_dir}/libpng-install"
windows_build_dir="${project_dir}/build-windows-x64"
package_dir="${project_dir}/dist/windows-x64"

sdl_version="2.32.10"
libpng_version="1.6.58"
sdl_archive="SDL2-devel-${sdl_version}-mingw.tar.gz"
libpng_archive="libpng-v${libpng_version}.tar.gz"
sdl_sha256="83a5d74012311edc3c0d40ea6faecbe57ad692aa033fa5dc273cc937e3938ff2"
libpng_sha256="a9d4df463d36a6e5f9c29bd6f4967312d17e996c1854f3511f833924eb1993cf"
sdl_url="https://github.com/libsdl-org/SDL/releases/download/release-${sdl_version}/${sdl_archive}"
libpng_url="https://github.com/pnggroup/libpng/archive/refs/tags/v${libpng_version}.tar.gz"

for program in cmake curl sha256sum tar x86_64-w64-mingw32-gcc \
               x86_64-w64-mingw32-g++; do
    if ! command -v "${program}" >/dev/null 2>&1; then
        echo "Missing required command: ${program}" >&2
        exit 1
    fi
done

if [[ ! -f /usr/x86_64-w64-mingw32/include/zlib.h ||
      ! -f /usr/x86_64-w64-mingw32/lib/libz.a ]]; then
    echo "Missing MinGW zlib. On Debian/Ubuntu install libz-mingw-w64-dev." >&2
    exit 1
fi

mkdir -p "${download_dir}" "${source_dir}"

download_and_verify() {
    local url="$1"
    local output="$2"
    local expected="$3"
    if [[ ! -f "${output}" ]]; then
        curl -fL --retry 3 --output "${output}" "${url}"
    fi
    printf '%s  %s\n' "${expected}" "${output}" | sha256sum --check -
}

download_and_verify "${sdl_url}" "${download_dir}/${sdl_archive}" \
                    "${sdl_sha256}"
download_and_verify "${libpng_url}" "${download_dir}/${libpng_archive}" \
                    "${libpng_sha256}"

sdl_source_dir="${source_dir}/SDL2-${sdl_version}"
libpng_source_dir="${source_dir}/libpng-${libpng_version}"
if [[ ! -d "${sdl_source_dir}" ]]; then
    tar -xzf "${download_dir}/${sdl_archive}" -C "${source_dir}"
fi
if [[ ! -d "${libpng_source_dir}" ]]; then
    tar -xzf "${download_dir}/${libpng_archive}" -C "${source_dir}"
fi

toolchain="${project_dir}/cmake/mingw-w64-x86_64.cmake"
cmake -S "${libpng_source_dir}" -B "${libpng_build_dir}" \
    -DCMAKE_TOOLCHAIN_FILE="${toolchain}" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="${libpng_install_dir}" \
    -DPNG_SHARED=OFF \
    -DPNG_STATIC=ON \
    -DPNG_TESTS=OFF \
    -DPNG_TOOLS=OFF \
    -DZLIB_INCLUDE_DIR=/usr/x86_64-w64-mingw32/include \
    -DZLIB_LIBRARY=/usr/x86_64-w64-mingw32/lib/libz.a
cmake --build "${libpng_build_dir}" --parallel
cmake --install "${libpng_build_dir}"

png_library="${libpng_install_dir}/lib/libpng16.a"
if [[ ! -f "${png_library}" ]]; then
    echo "Cross-compiled libpng archive was not produced." >&2
    exit 1
fi

sdl_prefix="${sdl_source_dir}/x86_64-w64-mingw32"
cmake -S "${project_dir}" -B "${windows_build_dir}" \
    -DCMAKE_TOOLCHAIN_FILE="${toolchain}" \
    -DCMAKE_BUILD_TYPE=Release \
    -DSDL2_DIR="${sdl_prefix}/lib/cmake/SDL2" \
    -DPNG_PNG_INCLUDE_DIR="${libpng_install_dir}/include" \
    -DPNG_LIBRARY="${png_library}" \
    -DZLIB_INCLUDE_DIR=/usr/x86_64-w64-mingw32/include \
    -DZLIB_LIBRARY=/usr/x86_64-w64-mingw32/lib/libz.a
cmake --build "${windows_build_dir}" --parallel

rm -rf "${package_dir}"
mkdir -p "${package_dir}/assets"
install -m 0755 "${windows_build_dir}/cockroach_overlay.exe" \
    "${package_dir}/cockroach_overlay.exe"
install -m 0755 "${windows_build_dir}/cockroach_swarm_20.exe" \
    "${package_dir}/cockroach_swarm_20.exe"
install -m 0755 "${sdl_prefix}/bin/SDL2.dll" \
    "${package_dir}/SDL2.dll"
install -m 0644 "${project_dir}/assets/cockroach_parts_atlas.png" \
    "${package_dir}/assets/cockroach_parts_atlas.png"
install -m 0644 "${project_dir}/packaging/WINDOWS-README.txt" \
    "${package_dir}/README.txt"

archive="${project_dir}/dist/cockroach-overlay-windows-x64.zip"
rm -f "${archive}"
(
    cd "${project_dir}/dist"
    cmake -E tar cf "${archive}" --format=zip windows-x64
)

echo
echo "Windows x64 package: ${package_dir}"
echo "ZIP archive: ${archive}"
