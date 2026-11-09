#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# RUSTC_WRAPPER for the wjit wasm build: export a growable function table so
# the playground can install compiled modules. Cargo probes rustc with
# --print=* and host proc-macros must not see wasm-ld flags.
rustc="$1"
shift
case " $* " in
  *" --print="*|*" --print "*) exec "$rustc" "$@" ;;
esac
case " $* " in
  *" wasm32-wasip1 "*) exec "$rustc" -C link-arg=--export-table -C link-arg=--growable-table "$@" ;;
esac
exec "$rustc" "$@"
