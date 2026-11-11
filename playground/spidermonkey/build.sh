#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds the playground's SpiderMonkey engine: dist/spidermonkey.wasm, a
# wasm32-wasi module (a reactor: no main, exported functions) of host.cpp
# linked with SpiderMonkey (Mozilla, MPL-2.0) as the Bytecode Alliance builds
# it for WASI (bytecodealliance/spidermonkey-wasi-embedding: the static
# library StarlingMonkey uses, SpiderMonkey 129 with the Portable Baseline
# Interpreter, no JIT, no Intl). The library is downloaded, not built: a
# build of SpiderMonkey takes over an hour.
#
# Needs curl, tar and about 700 MB of disk; wasi-sdk is downloaded too
# unless WASI_SDK points at one. CI (pages.yml) caches dist/ by the hash of
# this file and host.cpp.
#
#   playground/spidermonkey/build.sh
#   SM_WORK=/tmp/sm playground/spidermonkey/build.sh
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
WORK="${SM_WORK:-$HERE/work}"
# the embedding repository's commit whose release holds the library
SM_REV="${SM_REV:-b02d76023a15a3fa8c8f54bff5dac91099669003}"
SM_URL="https://github.com/bytecodealliance/spidermonkey-wasi-embedding/releases/download/rev_${SM_REV}/spidermonkey-wasm-static-lib_release.tar.gz"
SDK_VER=24
SDK_URL="https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${SDK_VER}/wasi-sdk-${SDK_VER}.0-x86_64-linux.tar.gz"

mkdir -p "$WORK"
if [ ! -d "$WORK/release/lib" ]; then
  curl --fail -sSL -o "$WORK/sm.tar.gz" "$SM_URL"
  tar -xf "$WORK/sm.tar.gz" -C "$WORK"
  rm "$WORK/sm.tar.gz"
fi
if [ -z "${WASI_SDK:-}" ]; then
  WASI_SDK="$WORK/wasi-sdk-${SDK_VER}.0-x86_64-linux"
  if [ ! -d "$WASI_SDK" ]; then
    curl --fail -sSL -o "$WORK/sdk.tar.gz" "$SDK_URL"
    tar -xf "$WORK/sdk.tar.gz" -C "$WORK"
    rm "$WORK/sdk.tar.gz"
  fi
fi

SM="$WORK/release"
CXX="$WASI_SDK/bin/clang++ --sysroot=$WASI_SDK/share/wasi-sysroot"
$CXX -std=c++20 -O2 -fno-exceptions -fno-rtti -DNDEBUG \
  -I"$SM/include" -include "$SM/include/js-config.h" \
  -c "$HERE/host.cpp" -o "$WORK/host.o"

# getpid is not in WASI (the emulation library answers a constant); the
# stack below the data so that an overflow traps instead of writing over it
mkdir -p "$HERE/dist"
$CXX -mexec-model=reactor -O2 "$WORK/host.o" "$SM"/lib/*.o \
  "$SM/lib/libjs_static.a" "$SM/lib/libjsrust.a" -lwasi-emulated-getpid \
  -Wl,--stack-first -Wl,-z,stack-size=1048576 -Wl,--strip-debug \
  -o "$HERE/dist/spidermonkey.wasm"
echo "$SM_REV" > "$HERE/dist/REVISION"
ls -l "$HERE/dist"
