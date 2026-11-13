#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds the WebAssembly engine with the baseline JIT for `cerxes-fuzz run
# --jit` (as playground/build.mjs does): prints the .wasm path.
set -e
here=$(cd "$(dirname "$0")" && pwd)
rustup target add wasm32-wasip1 >/dev/null 2>&1 || true
out="$here/../target/wasm-jit"
RUSTFLAGS="-Awarnings -C link-arg=--export-table -C link-arg=--growable-table" \
  cargo build --release --quiet --target wasm32-wasip1 --features wjit \
  --manifest-path "$here/../../../playground/cerxes-wasm/Cargo.toml" --target-dir "$out"
echo "$out/wasm32-wasip1/release/cerxes_wasm.wasm"
