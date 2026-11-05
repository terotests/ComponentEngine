#!/bin/bash
# Builds target/release/cerxes with profile-guided optimisation: an
# instrumented build runs the training scripts, and the release build is laid
# out by what they ran (hot code together, cold paths apart). On React and
# Chart.js, which are not in the training set, this cuts instruction cache
# misses by 20-25 % and the instruction count by 1-3 %.
#
#   cerxes/pgo.sh            # bench/cpu, plus Octane when Ranger is found
#
# Needs llvm-profdata of rustc's own LLVM: rustup component add llvm-tools.
# RANGER_ROOT (default ../../Ranger) holds the Octane sources; without it
# only bench/cpu trains, which helps less.
set -e
cd "$(dirname "$0")"
HOST=$(rustc -vV | sed -n 's/^host: //p')
PD="$(rustc --print sysroot)/lib/rustlib/$HOST/bin/llvm-profdata"
if [ ! -x "$PD" ]; then
  echo "llvm-profdata not found: rustup component add llvm-tools" >&2
  exit 1
fi
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> instrumented build"
RUSTFLAGS="-Awarnings -Cprofile-generate=$WORK/raw" CARGO_TARGET_DIR=target/pgo-gen cargo build -q --release
BIN=target/pgo-gen/release/cerxes

echo "==> training"
TRAIN=(bench/cpu/*.js)
export RANGER_ROOT=${RANGER_ROOT:-$(cd ../.. && pwd)/Ranger}
if [ -d "$RANGER_ROOT" ] && (cd ../cer && node bench/octane.mjs --dump="$WORK/octane" >/dev/null 2>&1); then
  TRAIN+=("$WORK"/octane/*.js)
else
  echo "    (no Octane: RANGER_ROOT=$RANGER_ROOT not usable)"
fi
for js in "${TRAIN[@]}"; do
  CER_REGS=1 timeout 300 "$BIN" "$js" >/dev/null 2>&1 || echo "    $js did not finish"
done
"$PD" merge -o "$WORK/merged.profdata" "$WORK"/raw/*.profraw

echo "==> optimised build"
RUSTFLAGS="-Awarnings -Cprofile-use=$WORK/merged.profdata" cargo build -q --release
echo "built target/release/cerxes with PGO (${#TRAIN[@]} training scripts)"
