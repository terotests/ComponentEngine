#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds cerxes-fuzz and runs it on this machine (Linux or macOS).
#
#   cerxes/fuzz/run-local.sh                 10 minutes, all cores but one
#   cerxes/fuzz/run-local.sh --time 3600 --node
#   MINUTES=60 JOBS=6 cerxes/fuzz/run-local.sh --heavy
#
# Findings go to cerxes/fuzz/out/<date-time>/<class>/<id>.js (+ .txt);
# summary.txt there counts them. Any other argument goes to `cerxes-fuzz run`.
set -e
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"
cargo build --release --quiet
if [ -z "$JOBS" ]; then
  n=$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2)
  JOBS=$(( n > 1 ? n - 1 : 1 ))
fi
MINUTES=${MINUTES:-10}
out="$here/out/$(date +%Y%m%d-%H%M%S)"
exec "$here/target/release/cerxes-fuzz" run \
  --time $((MINUTES * 60)) --jobs "$JOBS" --out "$out" \
  --corpus "$here/../tests/cases.txt" --corpus "$here/../../cer/tests/cases.txt" \
  "$@"
