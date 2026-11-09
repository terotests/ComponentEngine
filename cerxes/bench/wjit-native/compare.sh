#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Octane on one machine: CErXes native (register VM), CErXes WASM in the
# wasmtime host (interpreter / JIT), CErXes WASM in Node (interpreter / JIT),
# and native QuickJS and Hermes when QJS / HERMES point at their binaries.
#
#   OCTANE_DIR=dir [QJS=qjs] [HERMES=hermes] [RUNS=3] cerxes/bench/wjit-native/compare.sh
#
# OCTANE_DIR: the suites as cer/bench/octane.mjs --dump=dir writes them.
# Prints one row per suite (best of RUNS) and the geometric mean.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
RUNS="${RUNS:-3}"
: "${OCTANE_DIR:?OCTANE_DIR=dir from cer/bench/octane.mjs --dump=dir}"
CERXES="$REPO/cerxes/target/release/cerxes"
HOST="$HERE/target/release/wjit-native"
[ -x "$CERXES" ] || (cd "$REPO/cerxes" && cargo build --release --bin cerxes)
[ -x "$HOST" ] || (cd "$HERE" && cargo build --release)
WASM="$REPO/playground/cerxes-wasm/target-jit/wasm32-wasip1/release/cerxes_wasm.wasm"
[ -f "$WASM" ] || node "$REPO/cerxes/bench/wjit.mjs" --build --cases >/dev/null
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

engines=(native wasmtime-off wasmtime-jit node-off node-jit)
[ -n "${QJS:-}" ] && engines+=(quickjs)
[ -n "${HERMES:-}" ] && engines+=(hermes)

run() { # engine file -> "Name: score" lines
  local e="$1" f="$2"
  case "$e" in
    native) CER_REGS=1 "$CERXES" "$f" ;;
    wasmtime-off) "$HOST" --jit=0 "$f" 2>/dev/null ;;
    wasmtime-jit) "$HOST" --jit=1 "$f" 2>/dev/null ;;
    node-off) node "$REPO/cerxes/bench/wjit.mjs" --only-off=1 --eval="$f" ;;
    node-jit) node "$REPO/cerxes/bench/wjit.mjs" --only-on=1 --min=64 --eval="$f" ;;
    quickjs) "$QJS" --script "$f" ;;
    # Hermes has its own print(); the suites' console.log one goes
    hermes) sed '1,/^}$/d' "$f" > "$TMP/h.js"; "$HERMES" -O -w "$TMP/h.js" ;;
  esac 2>&1 | grep -E '^[A-Za-z]+: [0-9.]+$' || true
}

for e in "${engines[@]}"; do
  for f in "$OCTANE_DIR"/*.js; do
    for _ in $(seq "$RUNS"); do run "$e" "$f"; done
  done | awk -F': ' -v e="$e" '{ if (!($1 in b) || $2 > b[$1]) b[$1] = $2 } END { for (k in b) print e "\t" k "\t" b[k] }'
done > "$TMP/all.tsv"

python3 - "$TMP/all.tsv" "${engines[@]}" <<'EOF'
import sys, math, collections
rows = collections.defaultdict(dict)
for l in open(sys.argv[1]):
    e, k, v = l.rstrip("\n").split("\t")
    rows[k][e] = float(v)
engines = sys.argv[2:]
suites = [s for s in sorted(rows) if s != "SplayLatency"]
print("suite\t" + "\t".join(engines))
for s in suites:
    print(s + "\t" + "\t".join(str(int(rows[s].get(e, 0))) for e in engines))
geo = []
for e in engines:
    v = [rows[s][e] for s in suites if rows[s].get(e)]
    geo.append(str(round(math.exp(sum(map(math.log, v)) / len(v)))) + ("" if len(v) == len(suites) else f" ({len(v)}/{len(suites)})"))
print("geomean\t" + "\t".join(geo))
EOF
