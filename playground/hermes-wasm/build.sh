#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds Facebook Hermes (static_h) to WebAssembly with Emscripten, for the
# playground's hermes-wasm engine.
#
#   playground/hermes-wasm/build.sh
#
# Needs cmake, ninja, a host C++ compiler, and emsdk (EMSDK, or ~/emsdk).
# Writes hermes.js / hermes.wasm under $HERMES_WS/build-wasm/bin (default
# /tmp/hermes-ws) and copies them into playground/hermes-wasm/prebuilt for
# the playground build.

set -eu
HERMES_WS="${HERMES_WS:-/tmp/hermes-ws}"
EMSDK="${EMSDK:-${HOME}/emsdk}"
SRC="${HERMES_WS}/hermes"
HOST="${HERMES_WS}/build-host"
WASM="${HERMES_WS}/build-wasm"
JOBS="${JOBS:-$(nproc)}"

if [ ! -f "${EMSDK}/emsdk_env.sh" ]; then
  echo "emsdk not found at ${EMSDK}; clone emscripten-core/emsdk and run ./emsdk install latest && ./emsdk activate latest" >&2
  exit 1
fi
# shellcheck disable=SC1091
. "${EMSDK}/emsdk_env.sh"

if [ ! -d "${SRC}/.git" ]; then
  git clone --depth 1 --branch static_h https://github.com/facebook/hermes.git "${SRC}"
fi

if [ ! -x "${HOST}/bin/hermesc" ]; then
  cmake -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DHERMES_ENABLE_TEST_SUITE=OFF \
    -DHERMES_ENABLE_INTL=OFF \
    -DHERMES_ENABLE_DEBUGGER=OFF \
    -S "${SRC}" -B "${HOST}"
  cmake --build "${HOST}" --target hermesc --parallel "${JOBS}"
fi

LINKER_FLAGS="-sALLOW_MEMORY_GROWTH=1 -sSTACK_SIZE=256KB -sWASM=1 -sMODULARIZE=1 -sEXPORT_ES6=1 -sEXPORT_NAME=createHermes -sENVIRONMENT=web,worker -sINVOKE_RUN=0 -sEXIT_RUNTIME=0 -sFORCE_FILESYSTEM=1 -sEXPORTED_RUNTIME_METHODS=['FS','callMain']"
# emscripten 7's libc++ does not pull <exception> / <type_traits> into
# cstdlib the way libstdc++ does; Hermes's vendored LLVM demangler needs both.

cmake -G Ninja \
  -S "${SRC}" -B "${WASM}" \
  -DCMAKE_TOOLCHAIN_FILE="${EMSDK}/upstream/emscripten/cmake/Modules/Platform/Emscripten.cmake" \
  -DCMAKE_BUILD_TYPE=Release \
  -DIMPORT_HOST_COMPILERS="${HOST}/ImportHostCompilers.cmake" \
  -DHAVE_SYS_IOCTL_H=0 \
  -DHERMES_ENABLE_TEST_SUITE=OFF \
  -DHERMES_ENABLE_INTL=OFF \
  -DHERMES_ENABLE_DEBUGGER=OFF \
  -DHERMES_ENABLE_NAPI=OFF \
  -DCMAKE_CXX_FLAGS="-include exception -include type_traits" \
  -DCMAKE_EXE_LINKER_FLAGS="${LINKER_FLAGS}"

cmake --build "${WASM}" --target hermes --parallel "${JOBS}"
ls -l "${WASM}/bin/hermes.js" "${WASM}/bin/hermes.wasm"

PREBUILT="$(CDPATH= cd -- "$(dirname "$0")" && pwd)/prebuilt"
mkdir -p "${PREBUILT}"
cp "${WASM}/bin/hermes.js" "${WASM}/bin/hermes.wasm" "${PREBUILT}/"
echo "Hermes WASM ready at ${WASM}/bin (copied to ${PREBUILT})"
