#!/bin/bash
# The native JIT's cases (cases.js) on the C++ build, JIT on and off,
# against expected.txt (Node's answers).
#   cer/tests/njit/run.sh            x86-64 (or the host's machine)
#   ARM=1 cer/tests/njit/run.sh      AArch64 too, cross-built, under qemu
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
CER="$(cd "$HERE/../.." && pwd)"
RGRC="${RGRC:-$CER/../node_modules/ranger-compiler/dist/rgrc.js}"
OUT="${OUT:-/tmp/cer-njit}"
mkdir -p "$OUT"
node --stack-size=8000 "$RGRC" -l=cpp "$CER/bench/CerMainJit.rgr" -d="$OUT" -o=cer_main.cpp > "$OUT/rgrc.log" 2>&1 || { tail -20 "$OUT/rgrc.log"; exit 1; }
g++ -std=c++17 -O1 -o "$OUT/cer_main" "$OUT/cer_main.cpp"
fail=0
for mode in jit nojit; do
  arg=""; [ "$mode" = jit ] && arg=jit
  if "$OUT/cer_main" "$HERE" cases.js $arg | diff -q - "$HERE/expected.txt" > /dev/null; then echo "ok   native $mode"; else echo "FAIL native $mode"; fail=1; fi
done
if [ -n "$ARM" ]; then
  aarch64-linux-gnu-g++ -std=c++17 -O1 -static -o "$OUT/cer_main_arm" "$OUT/cer_main.cpp"
  if qemu-aarch64 "$OUT/cer_main_arm" "$HERE" cases.js jit | diff -q - "$HERE/expected.txt" > /dev/null; then echo "ok   aarch64 jit"; else echo "FAIL aarch64 jit"; fail=1; fi
fi
exit $fail
