#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds Hermes (facebook/hermes, MIT) as WebAssembly for the playground's
# Hermes engine: dist/hermes.mjs (emscripten's ES module factory) and
# dist/hermes.wasm. Follows Hermes's doc/Emscripten.md: a host build gives
# hermesc, then the VM's `hermes` command is built with emscripten.
#
# Needs emsdk active (EMSDK set, emcc on PATH), cmake, ninja, git, and about
# half an hour. CI (pages.yml) caches dist/ by this file's hash.
#
#   playground/hermes/build.sh            # work tree in playground/hermes/work
#   HERMES_WORK=/tmp/hw playground/hermes/build.sh
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REV="${HERMES_REV:-51ae2a08ce8cb06fcfa718cddab9c94ea9eee7f6}"
WORK="${HERMES_WORK:-$HERE/work}"
: "${EMSDK:?activate emsdk first (source <emsdk>/emsdk_env.sh)}"

mkdir -p "$WORK"
if [ ! -d "$WORK/src/.git" ]; then
  git clone --filter=blob:none https://github.com/facebook/hermes.git "$WORK/src"
fi
git -C "$WORK/src" fetch -q origin "$REV" || true
git -C "$WORK/src" checkout -q "$REV"

# stage 1: the host compilers the wasm build runs
cmake -G Ninja -DCMAKE_BUILD_TYPE=Release -S "$WORK/src" -B "$WORK/host"
cmake --build "$WORK/host" --target hermesc --target hermes --target shermes --parallel

# stage 2: the VM's command line as an ES module factory for a page or a
# worker; the playground writes each script into MEMFS and calls main
LINK="-sALLOW_MEMORY_GROWTH=1 -sSTACK_SIZE=1MB -sMODULARIZE=1 -sEXPORT_ES6=1"
LINK="$LINK -sEXPORT_NAME=HermesModule -sINVOKE_RUN=0 -sEXIT_RUNTIME=0"
LINK="$LINK -sFORCE_FILESYSTEM=1 -sENVIRONMENT=web,worker -sEXPORTED_RUNTIME_METHODS=callMain,FS"
cmake -G Ninja -S "$WORK/src" -B "$WORK/wasm" \
  -DCMAKE_TOOLCHAIN_FILE="$EMSDK/upstream/emscripten/cmake/Modules/Platform/Emscripten.cmake" \
  -DCMAKE_BUILD_TYPE=Release \
  -DIMPORT_HOST_COMPILERS="$WORK/host/ImportHostCompilers.cmake" \
  -DHAVE_SYS_IOCTL_H=0 \
  -DCMAKE_EXE_LINKER_FLAGS="$LINK"
cmake --build "$WORK/wasm" --target hermes --parallel

mkdir -p "$HERE/dist"
cp "$WORK/wasm/bin/hermes.js" "$HERE/dist/hermes.mjs"
cp "$WORK/wasm/bin/hermes.wasm" "$HERE/dist/hermes.wasm"
echo "$REV" > "$HERE/dist/REVISION"
ls -l "$HERE/dist"
