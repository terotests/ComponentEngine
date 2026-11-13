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
about 96 % of generated programs and 24 % of mutated ones.

## What is checked

| Class | Meaning | Security? |
|---|---|---|
| `panic` | A Rust panic in the engine (index out of bounds, unwrap …). A host that embeds CErXes loses the instance, and a script can make that happen whenever it wants. | yes (DoS) |
| `crash` | The worker died on a signal: abort, segfault. Under ASan (libfuzzer/) a memory error. | yes |
| `stack-overflow` | Native recursion overflowed the Rust stack. This is an abort that cannot be caught. | yes (DoS) |
| `oom` | An allocation failed under the worker's memory limit (`--mem-mb`, 4096 by default). The engine should throw a RangeError instead. | yes (DoS) |
| `hang` | The worker gave no answer within `--timeout` s (15): the engine did not give control back. | yes (DoS) |
| `slice-gap` | A 50 ms slice ran over 1 s before the engine yielded. The host could not have stopped it in time. | yes (DoS) |
| `snapshot-corrupt` | `from_snapshot` panicked on a snapshot with a few bytes changed. | yes |
| `hostile-snapshot` | A forged snapshot (one index field of the state set to a boundary value: free list, frame pc / bp / env, handler, stack value, object proto / shape, opcode; or a number token of the text) restored and was then run, and the run panicked. `crash … in hostile-snapshot run` when it died. | yes |
| `hostile-unit` | The same for a code-cache unit (`unit::encode` text) with an opcode or operand changed: decoded and run on the register or stack VM. | yes |
| `preempt` | A program that loops forever inside a call form (getter, valueOf, sort comparator, Promise job, … 38 forms, two deep) or calls a long native: a 50 ms (or 5 ms) slice ran over its length + 20 ms twice, or never ended (3 s). The signature names the forms, `sort>getter`. | yes (DoS) |
| `mismatch-wasm` | The wasm32 build (interpreter) printed something else than the native register VM (`--jit`). | correctness |
| `mismatch-jit` | The wasm build with the baseline JIT on (at once, or at the threshold 64) differs from it with the JIT off (`--jit`). | correctness |
| `jit-error` | A trap (a Rust panic is an abort in wasm) or an invalid JIT module in the wasm runs (`--jit`). | yes |
| `mismatch-vm` | The register VM and the stack VM printed different lines. | correctness |
| `mismatch-compiler` | pcomp and the tree compiler differ. | correctness |
| `mismatch-tsx` | The same JavaScript read as TSX gives another result (generated programs only). | correctness |
| `mismatch-snapshot` | The program run in 0.02 ms slices, with a snapshot and a restore into a new engine at each one, differs from one plain run. This is the check that suspend/resume and moving an instance between machines keep its state. | correctness |
| `mismatch-node` | Node prints something else (`--node`). Conformance, so there is a lot of it. The groups found first: strings with lone surrogates, early errors CErXes lets through, error types. | conformance |
| `slow` | The program did not finish in 3 s. Not a finding (an endless loop is allowed); only counted. | – |

The reference run is the register VM with pcomp, the product default. Each
program also runs on the stack VM, and by chance on the tree compiler, as
TSX and in slices with snapshots. 5 % of programs also have their final
snapshot corrupted, 10 % (`--hostile P`) get a forged snapshot or unit,
and 5 % of the programs (`--preempt P`) are preemption programs.

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

## Coverage-guided fuzzing (libfuzzer/)

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
