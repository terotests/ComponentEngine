# cerxes-fuzz

A fuzzer for CErXes. It writes random JavaScript, mutates the engines' own
test cases, runs every program in a worker process and checks the result
against several oracles. It needs nothing but stable Rust (Node only for
`--node`), so it runs the same on a laptop, in CI and on a cloud VM.

```sh
cerxes/fuzz/run-local.sh                       # 10 min, all cores but one
MINUTES=60 cerxes/fuzz/run-local.sh --node     # an hour, also against Node
cerxes/fuzz/run-local.sh --heavy               # resource limits
```

Findings go to `cerxes/fuzz/out/<date-time>/<class>/<id>.js`, with a
`<id>.txt` beside each that says what went wrong, and `summary.txt` counts
them. A finding is saved once per signature (a panic's source line, the
words a mismatch differs by). The rest are only counted.

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
| `mismatch-vm` | The register VM and the stack VM printed different lines. | correctness |
| `mismatch-compiler` | pcomp and the tree compiler differ. | correctness |
| `mismatch-tsx` | The same JavaScript read as TSX gives another result (generated programs only). | correctness |
| `mismatch-snapshot` | The program run in 0.02 ms slices, with a snapshot and a restore into a new engine at each one, differs from one plain run. This is the check that suspend/resume and moving an instance between machines keep its state. | correctness |
| `mismatch-node` | Node prints something else (`--node`). Conformance, so there is a lot of it. The groups found first: strings with lone surrogates, early errors CErXes lets through, error types. | conformance |
| `slow` | The program did not finish in 3 s. Not a finding (an endless loop is allowed); only counted. | – |

The reference run is the register VM with pcomp, the product default. Each
program also runs on the stack VM, and by chance on the tree compiler, as
TSX and in slices with snapshots. 10 % of programs also have their final
snapshot corrupted.

`known/` holds the minimised findings of the first runs.

## Commands

```sh
B=cerxes/fuzz/target/release/cerxes-fuzz
$B run --time 600 --jobs 4 --corpus cerxes/tests/cases.txt [--node] [--heavy] [--rc-check] [--seed N]
$B repro out/…/panic/<id>.js          # the oracles in this process, prints the verdict and lines
$B min   out/…/panic/<id>.js          # shrinks it while the class and signature stay: <id>.min.js
$B gen --seed 7                       # one generated program, e.g. to run in node
```

- `--node` also runs each well-formed generated program in Node and compares the printed lines.
- `--heavy` lets the generator write deep recursion, huge strings and arrays, deep nesting, ReDoS regexps and long promise chains.
- `--rc-check` checks every reference-count reclaim against a full mark (`Vm::rc_check`). It is slow but finds GC bugs.
- `--mutate P` sets the share of programs that are mutations (default 30 %).
- `--seed N` makes a run repeatable. The seed of each finding is in its .txt.

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
- **GitHub Actions**: `.github/workflows/fuzz.yml` runs nightly (30 min,
  default and heavy) and on demand ("Run workflow": minutes, flags). It is not
  part of "All tests". Findings are the `fuzz-out-*` artifacts.
- **Cloud VM / container**: `cerxes/fuzz/Dockerfile`
  (`docker run -v $PWD/out:/out -e MINUTES=240 -e JOBS=16 cerxes-fuzz`).
  Throughput scales with cores: about 6 programs/s per core with every oracle
  on (4 vCPU, 2026-11).
