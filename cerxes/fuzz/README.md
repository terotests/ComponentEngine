# cerxes-fuzz

A fuzzer for CErXes. It writes random JavaScript, mutates the engines' own
test cases, runs every program in a worker process and checks the result
against several oracles. It needs nothing but stable Rust (Node only for
`--node`), so it runs the same on a laptop, in CI and on a cloud VM.

```sh
cerxes/fuzz/run-local.sh                       # 10 min, all cores but one
MINUTES=60 cerxes/fuzz/run-local.sh --node     # an hour, also against Node
cerxes/fuzz/run-local.sh --heavy               # resource limits
cerxes/fuzz/run-local.sh --hostile 60 --preempt 30   # forged snapshots / units, preemption
JIT=1 cerxes/fuzz/run-local.sh                 # also the wasm engine, JIT off / on (needs node)
COV=1 cerxes/fuzz/run-local.sh                 # coverage-guided (see below)
```

The workers run the **checked build** (`--profile checked`: release speed
with debug assertions on). The engine's unchecked indexes assert there, so
an index out of range is a `panic` with the engine function and line
instead of a silent read past a vector or a SIGSEGV. `CHECKED=0` uses the
release build.

Findings go to `cerxes/fuzz/out/<date-time>/<class>/<id>.js`, with a
`<id>.txt` beside each that says what went wrong, and `summary.txt` counts
them. A finding is saved once per signature (a panic's message and the
engine function it happened in, a crash's stage, the words a mismatch
differs by). The rest are only counted.

Findings that are known and not fixed yet are listed in
`known/known.txt`. They are counted as `known <class>`, not saved again,
and `summary.txt` lists the hits per entry, so what a run prints as new is
new. A known hang (`skip` lines) runs only 2 % of the time instead of
taking a worker for its whole timeout. `cerxes-fuzz known` says which
minimised cases in `known/` still reproduce. When a fix is merged, remove
its line from `known.txt`.

`summary.txt` also gives the share of programs that compiled (`valid`):
all generated programs and about 40 % of mutated ones (half the mutations
change whole lines, operators, names or numbers; the other half bytes).

## What is checked

| Class | Meaning | Security? |
|---|---|---|
| `panic` | A Rust panic in the engine (index out of bounds, unwrap …). A host that embeds CErXes loses the instance, and a script can make that happen whenever it wants. | yes (DoS) |
| `crash` | The worker died on a signal: abort, segfault. Under ASan (libfuzzer/) a memory error. | yes |
| `stack-overflow` | Native recursion overflowed the Rust stack. This is an abort that cannot be caught. | yes (DoS) |
| `oom` | An allocation failed under the worker's memory limit (`--mem-mb`, 4096 by default). The engine should throw a RangeError instead. The oracle's engines run with `Engine::set_heap_limit` at half of `--mem-mb` (a counting allocator is the probe), so a script that only keeps taking memory stops as a host would stop it and is not compared; what is left as `oom` is one request the limit cannot see coming. | yes (DoS) |
| `hang` | The worker gave no answer within `--timeout` s (15): the engine did not give control back. | yes (DoS) |
| `slice-gap` | A 50 ms slice ran over 1 s before the engine yielded. The host could not have stopped it in time. | yes (DoS) |
| `snapshot-corrupt` | `from_snapshot` panicked on a snapshot with a few bytes changed. | yes |
| `hostile-snapshot` | A forged snapshot (one index field of the state set to a boundary value: free list, frame pc / bp / env, handler, stack value, object proto / shape, opcode; or a number token of the text) restored and was then run, and the run panicked. `crash … in hostile-snapshot run` when it died. | yes |
| `hostile-unit` | The same for a code-cache unit (`unit::encode` text) with an opcode or operand changed: decoded and run on the register or stack VM. | yes |
| `preempt` | A program that loops forever inside a call form (getter, valueOf, sort comparator, Promise job, … 38 forms, two deep) or calls a long native: a 50 ms (or 5 ms) slice ran over its length + 20 ms twice, or never ended (3 s). The signature names the forms, `sort>getter`. | yes (DoS) |
| `deadline` | The same program under a host time limit of one slice (`Engine::set_time_limit`) and no slices ran over the limit + 20 ms twice: the limit, which also holds where a slice cannot stop, failed. | yes (DoS) |
| `mismatch-wasm` | The wasm32 build (interpreter) printed something else than the native register VM (`--jit`). | correctness |
| `mismatch-jit` | The wasm build with the baseline JIT on (at once, or at the threshold 64) differs from it with the JIT off (`--jit`). | correctness |
| `jit-error` | A trap (a Rust panic is an abort in wasm) or an invalid JIT module in the wasm runs (`--jit`). | yes |
| `mismatch-vm` | The register VM and the stack VM printed different lines. | correctness |
| `mismatch-compiler` | pcomp and the tree compiler differ. | correctness |
| `mismatch-tsx` | The same JavaScript read as TSX gives another result (generated programs only). | correctness |
| `mismatch-snapshot` | The program run in 0.02 ms slices, with a snapshot and a restore into a new engine at each one, differs from one plain run. This is the check that suspend/resume and moving an instance between machines keep its state. | correctness |
| `snapshot-roundtrip` | A finished program's engine and the engine restored from its snapshot answer a probe differently (globals `v0`…`v8`, the functions `f0`… called, a `D0` instance made, the global names; twice, so changes the first probe makes are compared too), or the snapshot text is not a fixed point: taken twice, or after a second restore, it differs. | correctness |
| `nondeterminism` | The same program run twice in one process printed different lines: state leaks between engines (statics, caches). | correctness |
| `mismatch-node` | Node prints something else (`--node`). Conformance, so there is a lot of it. The groups found first: strings with lone surrogates, early errors CErXes lets through, error types. | conformance |
| `slow` | The program did not finish in 3 s. Not a finding (an endless loop is allowed); only counted. | – |

The reference run is the register VM with pcomp, the product default. Each
program also runs on the stack VM, and by chance on the tree compiler, as
TSX and in slices with snapshots. 5 % of programs also have their final
snapshot corrupted, 10 % (`--hostile P`) get a forged snapshot or unit,
and 5 % of the programs (`--preempt P`) are preemption programs. 10 % of
generated programs (`--roundtrip P`) get the snapshot round trip and 5 %
run twice (`nondeterminism`). `cerxes-fuzz snapfix FILE` (src/bin) shows
where the snapshot texts of a program differ.

### What the generator writes

Besides random expressions and statements, the generator writes the
patterns of the 2026-11-13 audit and of the known V8 bug classes
(`seeds/README.md` maps them): builtins whose argument, callback or element
shrinks or re-protos the array halfway (`fill(7, { valueOf() { a.length =
0; … } })`), typed arrays resized or detached the same way, functions
warmed up past the JIT threshold and then given an edge index or a shape
change, return / break / continue / throw through `finally`, numbers at
the edges computed (`-0 % 5`, `(2147483647 + 1) | 0`, `(1e300).toString(3)`),
and getters, revivers, traps and iterators that change what a builtin
walks.

`known/` holds the minimised findings of the first runs.

## Fuzzer v3: other engines' bugs

The bugs other engines have already had are the likeliest bugs of a new
one. v3 seeds the fuzzer with them and mutates in their shape:

- **Regression tests of other engines** (`seeds/fetch-engines.sh`, then
  `cerxes-fuzz import`, src/import.rs): JavaScriptCore `JSTests/stress`,
  SpiderMonkey `jit-test` and `non262/regress`, ChakraCore `test/`, Hermes
  `test/hermes`, QuickJS `tests/`: about 13 000 tests. A shim in plain
  JavaScript goes in front of each (`shouldBe`, `assertEq`, `noInline`,
  `WScript.Echo`, `%OptimizeFunctionOnNextCall` …): assertions print
  `FAIL …` and go on, JIT hints are no-ops, `detachArrayBuffer` really
  detaches, `Math.random` and `Date.now` are fixed. Loop bounds of 1000 or
  more in loop heads become 300 (still past the JIT threshold). Tests that
  need wasm, workers, new globals, modules or files are left out. They are
  fetched, not committed (their licenses), into `foreign/`, which
  run-local.sh reads when it is there.
- **CVE seeds** (`seeds/engines`, README there): 36 programs, each the shape
  of a published bug of JSC, SpiderMonkey, QuickJS, Hermes, ChakraCore or
  V8 (CVE-2016-4622 slice, CVE-2019-11707 pop, CVE-2019-9810 slice species,
  CVE-2019-17026 StoreElementHole, CVE-2020-9802 ArithNegate, CVE-2019-0567
  InitProto, CVE-2023-31922 Proxy isArray, CVE-2024-29943 range analysis …),
  plus the classes that matter for reference counting and snapshots.
- **Bug-class hooks** (src/hooks.rs): `--class P` (30 %) of the programs are
  a seed of those two kinds, as it is (15 %) or with one to three hooks:
  `arg` (a number argument of a builtin becomes a valueOf / toString /
  toPrimitive that changes the receiver), `species`, `proto` (index accessor
  on a prototype), `warm` (a call run past the JIT threshold, then once
  with an edge value), `proxy`, `detach`, `then` (Object.prototype.then),
  `drop` (a callback drops the last reference and collects), `getter`,
  `regexp`. The first line of such a program names its hooks
  (`// v3 hooks: arg+warm`). `cerxes-fuzz hook FILE.js --seed N` shows what
  the hooks do to a file. These programs also go to the JIT oracle.

```sh
cerxes/fuzz/seeds/fetch-engines.sh             # ~5 min: clone, import into foreign/
MINUTES=30 cerxes/fuzz/run-local.sh --class 60
```

The nightly CI has a `foreign` shard that does the same.

### What v3 found first (2026-11-15)

The 13 221 imported tests, run once each as they are, aborted CErXes 44
times (no panics): 8 native stack overflows and 36 allocation failures. The
first 15-minute v3 run (20 100 programs, 3 workers) added a register-VM
cache bug and a snapshot bug. Fixed in the same pull request:

- stack overflow: Array.isArray through 200 000 proxies (QuickJS
  CVE-2023-31922), `k in o` up a 300 000-deep prototype chain, native-to-
  native recursion without a script frame (a getter that is
  Object.prototype.toLocaleString, toString that is
  String.prototype.lastIndexOf), a JSON.parse reviver that makes the value
  cyclic (ChakraCore #3900);
- allocation failure: ArrayBuffer contents and long runs of holes did not
  count towards a collection (300 dropped 10 MB buffers), `k in o` made an
  atom for every new index (3 M checks: 355 MB that never came back),
  replace / `$1` expansion / new Function parameters / array spread grew
  past the string or element limit, push and `a[a.length] = x` had no
  element limit (the process asked for 4 GB), the generic Array methods
  (prelude) ran with the collector off, so `join` over an array-like of
  length 2 ** 32 kept every string;
- register VM: a typed array's `length` cache ignored an own `length`
  defined on a dictionary-mode typed array (JSC typedarray-length-dictionary);
- snapshot: a mapped `arguments` object lost its link to the parameters on
  restore (JSC parameter-scoping under the snapshot oracle);
- conformance: slice, join and sort read the array before the arguments'
  valueOf / the comparator changed it (the CVE-2016-4622 order), concat of a
  spreadable with length 2 ** 53 - 1 checked 2 ** 53 indexes.

Left open: about ten allocation failures that only a total heap limit fixes
(regexp over a 512 MB string, a 178 MB function source, 8 M-element array
literals; `known oom`), BigInt arithmetic on 2 ** 20-bit numbers (decimal
strings, 36 s for one JSC test: `hang`), and conformance diffs the seeds
show (Symbol.unscopables, an index setter on Array.prototype for a store
past the end, holes reading Object.prototype's prototype, case-insensitive
lookbehind on astral characters, String object `length` redefinable).

### Second round (2026-11-15)

The items left open above, worked through:

- allocation failures: none of the 19 aborts the fuzzer any more. The
  oracle runs under the heap limit (#171), so the hogs (typed arrays made
  inside their own source's getter, `new Array(a)` forever) stop cleanly;
  single requests past what is left of the limit, or past 2 GB
  (`Vm::alloc_refused`), are a RangeError "Out of memory": typed array and
  ArrayBuffer contents, eval / new Function sources (~16 bytes of tokens a
  byte). A regexp over a string past 128 M bytes is a RangeError (its
  UTF-16 copy is 8 bytes a unit), a backtrack list past 16 M entries is the
  backtracking RangeError (`/(){999999999}/` asked for 3 GB), JSON.stringify
  stops at the string limit, `flat` at the element limit, a generic `join`
  whose separators alone pass the string limit throws before reading.
- conformance: `with` honours Symbol.unscopables (OP_IN a=1 for a `with`
  object's binding), a store or push past the end of an array goes through
  an accessor or read-only index on a prototype, a String object's
  `length` and characters cannot be redefined, lookbehind under /u reads a
  surrogate pair as one character, `replace` with a frozen global regexp
  throws and a non-global one converts lastIndex (valueOf runs). Holes
  already read Object.prototype.
- BigInt on 2^20-bit numbers: Karatsuba multiplication, Knuth's division
  and, for long divisors, a Newton reciprocal, 2^k by squaring, radix
  conversion by halves. Before → after: `1n << 1048576n` 7.6 s → 0.7 s,
  a 2^19-bit square 4.3 s → 0.3 s, 2^20 / 2^19 bits 115 s → 2.2 s, a
  2^20-bit hex literal 57 s → 1.5 s, `toString(16)` 16 s → 7 s, `&` 19 s
  → 9 s; JSC bigint-divide-burnikel-ziegler 30 s → 3.4 s,
  bigint-to-string-divide-and-conquer-large 16 s → 3.8 s. Values are still
  decimal text, so conversions to binary (hex, bit operations) stay the
  slowest part.

Still open: an array store far past the end (16 M) fills the gap with holes
(0.5 s each; SpiderMonkey bug 1693328 is a `slice-gap`), as index
properties past the elements do not count towards `length`; long native
work (a 16 MB eval, many 128 MB typed arrays) shows as `slice-gap`; the
wasm JIT's inlined push does not yet look at prototype index setters.

## Commands

```sh
B=cerxes/fuzz/target/release/cerxes-fuzz
$B run --time 600 --jobs 4 --corpus cerxes/tests/cases.txt [--node] [--heavy] [--rc-check] [--seed N]
       [--hostile P] [--preempt P] [--jit WASM] [--jit-pct P] [--known FILE]
$B repro out/…/panic/<id>.js          # the oracles in this process, prints the verdict and lines
$B min   out/…/panic/<id>.js          # shrinks it while the class and signature stay: <id>.min.js
$B gen --seed 7                       # one generated program, e.g. to run in node
$B gen --seed 7 --preempt 100         # one preemption program
$B known                              # which known/ cases still reproduce
$B import DIR --out foreign [--engine jsc] [--max N]   # another engine's tests as seeds
$B hook FILE.js --seed 3              # FILE with bug-class hooks, printed
```
(`B=cerxes/fuzz/target/checked/cerxes-fuzz` for the checked build.)

- `--node` also runs each well-formed generated program in Node and compares the printed lines.
- `--heavy` lets the generator write deep recursion, huge strings and arrays, deep nesting, ReDoS regexps and long promise chains.
- `--rc-check` checks every reference-count reclaim against a full mark (`Vm::rc_check`). It is slow but finds GC bugs.
- `--mutate P` sets the share of programs that are mutations (default 30 %).
- `--seed N` makes a run repeatable. The seed of each finding is in its .txt.
- `--corpus PATH` takes a cases.txt file, a .js file or a directory of them (`seeds/`, `cerxes/tests/v8/own`).
- `--hostile P` / `--preempt P`: shares for the forged-input and preemption oracles.
- `--jit WASM` runs `--jit-pct` (30 %) of the well-formed generated programs also in the wasm engine (`jit/build.sh` builds it, `jit/worker.mjs` runs it in Node): JIT off, on at once and at 64.

When a worker dies, the driver reads the stage it wrote last (`.worker<N>.stage`:
`register VM`, `hostile-unit run`, …), so a SIGSEGV of one oracle does not
hide another's.

Generated programs are deterministic. They use no `Date.now`, no
`Math.random` and only bounded loops, and every statement prints through
`__p` inside its own `try/catch`, so two engines can be compared line by
line.

## Coverage-guided mutation on stable Rust (COV=1)

`COV=1 run-local.sh` builds the engine with LLVM SanitizerCoverage
(`-C passes=sancov-module`, inline 8-bit counters, into `target/cov`;
`--target` is given so that proc macros are built without it). The worker
reads the counters after each program (`src/cov.rs`); a program that lit an
edge, or an edge as many times (AFL buckets), that no worker had seen yet is
kept and written to `out/…/corpus/`. Mutation then draws from these as well
as from the seeds, and takes whole lines from a second program. A plain
build has no counters and only mutates the seeds. The status line and
`summary.txt` give the features seen and the programs kept. The CI default
shard runs this build.

## Coverage-guided fuzzing with ASan (libfuzzer/)

`libfuzzer/` has cargo-fuzz targets that need nightly Rust:

```sh
rustup toolchain install nightly && cargo install cargo-fuzz
cd cerxes/fuzz/libfuzzer
cargo +nightly fuzz run --fuzz-dir . compile  -- -max_total_time=600              # lexer, parser, both compilers
cargo +nightly fuzz run --fuzz-dir . eval     -- -max_total_time=600 -timeout=10  # runs, 5 ms slices, 1 s budget
cargo +nightly fuzz run --fuzz-dir . snapshot -- -max_total_time=600              # from_snapshot on bad input
```

These steer by coverage and build with AddressSanitizer, so they find memory
errors in the `unsafe` code that a panic would not show. The stable fuzzer
writes better-formed JavaScript and has the oracles. Use both.

## Where to run it

- **Locally**: `run-local.sh`. On macOS `ulimit -v` is not enforced, so `oom`
  shows up as the machine slowing down. Keep `--heavy` runs short there, or
  use Docker.
- **GitHub Actions**: `.github/workflows/fuzz.yml` runs nightly (30 min per
  shard) and on demand ("Run workflow": minutes, flags). It is not
  part of "All tests". Shards: default (with Node), heavy, hostile
  (`--hostile 60 --preempt 30`) and jit, all on the checked build.
  Findings are the `fuzz-out-*` artifacts.
- **Cloud VM / container**: `cerxes/fuzz/Dockerfile`
  (`docker run -v $PWD/out:/out -e MINUTES=240 -e JOBS=16 cerxes-fuzz`).
  Throughput scales with cores: about 6 programs/s per core with every oracle
  on (4 vCPU, 2026-11).
