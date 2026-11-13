#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds cerxes-fuzz and runs it on this machine (Linux or macOS).
#
#   cerxes/fuzz/run-local.sh                 10 minutes, all cores but one
#   cerxes/fuzz/run-local.sh --time 3600 --node
#   MINUTES=60 JOBS=6 cerxes/fuzz/run-local.sh --heavy
#   JIT=1 cerxes/fuzz/run-local.sh           also the wasm engine, JIT off / on (needs node)
#   CHECKED=0 cerxes/fuzz/run-local.sh       the release build (no debug assertions)
#
# The checked build (debug assertions on) is the default: an unchecked
# index out of range then panics with a source line instead of reading
# past a vector. Findings go to cerxes/fuzz/out/<date-time>/<class>/<id>.js
# (+ .txt); summary.txt there counts them and lists the known ones
# (known/known.txt). Any other argument goes to `cerxes-fuzz run`.
set -e
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"
if [ "${CHECKED:-1}" = 0 ]; then
  cargo build --release --quiet
  bin="$here/target/release/cerxes-fuzz"
else
  cargo build --profile checked --quiet
  bin="$here/target/checked/cerxes-fuzz"
fi
jit=""
if [ "${JIT:-0}" = 1 ]; then
  jit="--jit $("$here/jit/build.sh")"
fi
if [ -z "$JOBS" ]; then
  n=$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2)
  JOBS=$(( n > 1 ? n - 1 : 1 ))
fi
MINUTES=${MINUTES:-10}
out="$here/out/$(date +%Y%m%d-%H%M%S)"
# seeds: the engines' case files, the known V8 bug tests (cerxes/tests/v8,
# when present) and seeds/ (extra .js files, e.g. V8 regression tests)
exec "$bin" run \
  --time $((MINUTES * 60)) --jobs "$JOBS" --out "$out" \
  --corpus "$here/../tests/cases.txt" --corpus "$here/../../cer/tests/cases.txt" \
  --corpus "$here/../tests/v8/own" --corpus "$here/seeds" \
  $jit "$@"
