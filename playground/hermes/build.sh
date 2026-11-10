#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds Hermes (facebook/hermes, MIT) as WebAssembly for the playground's
# Hermes engine: dist/hermes.mjs (emscripten's ES module factory) and
# dist/hermes.wasm. Follows Hermes's doc/Emscripten.md: a host build gives
# hermesc, then host.cpp (a JSI runtime) is built with emscripten.
#
# Needs emsdk active (EMSDK set, emcc on PATH), cmake, ninja, git, and about
# half an hour. CI (pages.yml) caches dist/ by the hash of this file and host.cpp.
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
git -C "$WORK/src" checkout -q -f "$REV"
git -C "$WORK/src" reset -q --hard

# Without pthreads std::thread throws, which aborts (no C++ exceptions):
# SerialExecutor, where the JSI layer defers deleting host function
# contexts, would start its worker at runtime teardown and abort after the
# script has run. In a single-threaded build it keeps the tasks and runs
# them in its destructor instead.
python3 - "$WORK/src/lib/Support/SerialExecutor.cpp" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read()
st = "#if defined(__EMSCRIPTEN__) && !defined(__EMSCRIPTEN_PTHREADS__)\n"
dtor = "SerialExecutor::~SerialExecutor() {\n"
add = "void SerialExecutor::add(llvh::unique_function<void()> task) {\n"
assert dtor in s and add in s
s = s.replace(dtor, dtor + st +
  "  while (!tasks_.empty()) {\n"
  "    llvh::unique_function<void()> t = std::move(tasks_.front());\n"
  "    tasks_.pop_front();\n"
  "    t();\n"
  "  }\n"
  "  return;\n"
  "#endif\n", 1)
s = s.replace(add, add + st +
  "  tasks_.push_back(std::move(task));\n"
  "  return;\n"
  "#endif\n", 1)
open(p, "w").write(s)
PY

# stage 1: the host compilers the wasm build runs
cmake -G Ninja -DCMAKE_BUILD_TYPE=Release -S "$WORK/src" -B "$WORK/host"
cmake --build "$WORK/host" --target hermesc --target hermes --target shermes --parallel

# stage 2: host.cpp, one JSI runtime kept in the instance (hermes_eval,
# hermes_reset), as an ES module factory for a page or a worker. Added to
# Hermes's tools next to its own JSI tool (tools/jsi), linked the same way.
# No test suite and no Node-API: their shared-library addons collide in
# ninja under emscripten ("multiple rules generate ...test_exception.node").
mkdir -p "$WORK/src/tools/playground"
cp "$HERE/host.cpp" "$WORK/src/tools/playground/host.cpp"
cat > "$WORK/src/tools/playground/CMakeLists.txt" <<'CM'
set(HERMES_ENABLE_EH ON)
add_hermes_tool(hermes-playground host.cpp LINK_OBJLIBS hermesvm_a)
CM
grep -q 'add_subdirectory(playground)' "$WORK/src/tools/CMakeLists.txt" ||
  echo 'add_subdirectory(playground)' >> "$WORK/src/tools/CMakeLists.txt"

LINK="-sALLOW_MEMORY_GROWTH=1 -sSTACK_SIZE=1MB -sMODULARIZE=1 -sEXPORT_ES6=1"
LINK="$LINK -sEXPORT_NAME=HermesModule -sEXIT_RUNTIME=0 --profiling-funcs"
LINK="$LINK -sENVIRONMENT=web,worker -sEXPORTED_FUNCTIONS=_main,_malloc,_free"
LINK="$LINK -sEXPORTED_RUNTIME_METHODS=stringToNewUTF8,UTF8ToString"
cmake -G Ninja -S "$WORK/src" -B "$WORK/wasm" \
  -DCMAKE_TOOLCHAIN_FILE="$EMSDK/upstream/emscripten/cmake/Modules/Platform/Emscripten.cmake" \
  -DCMAKE_BUILD_TYPE=Release \
  -DIMPORT_HOST_COMPILERS="$WORK/host/ImportHostCompilers.cmake" \
  -DHAVE_SYS_IOCTL_H=0 \
  -DHERMES_ENABLE_TEST_SUITE=OFF \
  -DHERMES_ENABLE_NAPI=OFF \
  -DCMAKE_EXE_LINKER_FLAGS="$LINK"
cmake --build "$WORK/wasm" --target hermes-playground --parallel

mkdir -p "$HERE/dist"
cp "$WORK/wasm/bin/hermes-playground.js" "$HERE/dist/hermes.mjs"
cp "$WORK/wasm/bin/hermes-playground.wasm" "$HERE/dist/hermes.wasm"
echo "$REV" > "$HERE/dist/REVISION"
ls -l "$HERE/dist"
