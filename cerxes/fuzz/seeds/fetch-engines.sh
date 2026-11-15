#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Fetches other engines' regression tests and turns them into seeds for
# fuzzer v3 (`cerxes-fuzz import`, src/import.rs): JavaScriptCore
# (JSTests/stress), SpiderMonkey (jit-test, non262/regress), ChakraCore
# (test/), Hermes (test/hermes) and QuickJS (tests/). Shallow, sparse
# clones of the GitHub mirrors (only the test folders are checked out),
# about 300 MB of git data and 120 MB of seeds.
#
#   cerxes/fuzz/seeds/fetch-engines.sh [OUT]     default cerxes/fuzz/foreign
#
# The tests keep their own licenses (BSD, MPL 2.0, MIT), so they are not
# committed; foreign/ is in .gitignore. run-local.sh reads foreign/ when it
# is there.
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$here/foreign}
work=${WORK:-$here/target/engines}
mkdir -p "$work"
fetch() { # repo dir paths...
  repo=$1; dir=$2; shift 2
  if [ ! -d "$work/$dir" ]; then
    git clone -q --depth 1 --filter=blob:none --sparse "https://github.com/$repo" "$work/$dir"
    (cd "$work/$dir" && git sparse-checkout set --no-cone $(for p in "$@"; do printf '/%s/ ' "$p"; done))
  fi
}
fetch WebKit/WebKit webkit JSTests/stress &
fetch mozilla-firefox/firefox firefox js/src/jit-test/tests js/src/tests/non262/regress &
fetch chakra-core/ChakraCore chakra test &
fetch facebook/hermes hermes test/hermes &
fetch bellard/quickjs quickjs tests &
wait
bin=${FUZZ:-}
if [ -z "$bin" ]; then
  (cd "$here" && cargo build --profile checked --quiet)
  bin="$here/target/checked/cerxes-fuzz"
fi
rm -rf "$out"
"$bin" import "$work/webkit/JSTests/stress" --out "$out" --engine jsc
"$bin" import "$work/firefox/js/src/jit-test/tests" --out "$out" --engine sm
"$bin" import "$work/firefox/js/src/tests/non262/regress" --out "$out" --engine sm-regress
"$bin" import "$work/chakra/test" --out "$out" --engine chakra
"$bin" import "$work/hermes/test/hermes" --out "$out" --engine hermes
"$bin" import "$work/quickjs/tests" --out "$out" --engine qjs
