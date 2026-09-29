# Playground

**Live:** <https://terotests.github.io/ComponentEngine/>

ComponentEngine's evaluator, built four ways, running in the browser next to
the browser's own engine and three other small JavaScript engines. A
benchmark runs a set of workloads in every engine you tick and compares both
the time and the answer with the browser's; an editor runs your own script in
all of them.

| Engine | What it is |
| --- | --- |
| Browser JS | the page's own engine (V8, SpiderMonkey, JavaScriptCore), through `new Function`: the reference |
| CEr · Rust → WASM | `cer/` compiled by cargo for `wasm32-wasip1` (`cer-wasm/`, a C ABI) |
| CE · Ranger → C++ → WASM | `engine/` compiled by rgrc to C++, then by clang from wasi-sdk (`ce-wasm/`) |
| CE · Ranger → JS | `engine/` compiled by rgrc to JavaScript (`bin/engine_module.cjs`) |
| CEr · Rust → Ranger → JS | `cer/src/lib.rs` read by rgrc as a strict Rust module, written as JavaScript |
| QuickJS · C → WASM | Fabrice Bellard's QuickJS, from `quickjs-emscripten` (MIT) |
| Sval · JS interpreter | an AST-walking interpreter in JavaScript (MIT) |
| JS-Interpreter · ES5 | Neil Fraser's step-by-step ES5 interpreter (Apache-2.0) |

Each engine runs in a Web Worker of its own, one engine at a time, so no two
compete for the CPU; a run past the time limit ends its worker. Times are
the median of the runs, wall-clock around the engine's eval call, parsing
included and loading excluded. The chart is the geometric mean, over the
workloads an engine answered as the browser did, of its time over the
browser's.

The WASM builds call the few WASI functions they need (clock, random, stdout)
through `src/wasi.js`; there are no files and no network.

## Building

```sh
npm ci
node playground/build.mjs          # -> playground/dist
node playground/smoke.mjs          # Chromium: runs the benchmark, checks the answers
npx serve playground/dist          # or any static server
```

`build.mjs` needs:

- `rustup target add wasm32-wasip1` for CEr · WASM;
- [wasi-sdk](https://github.com/WebAssembly/wasi-sdk) (29 or later) at
  `WASI_SDK_PATH` (default `/opt/wasi-sdk`) for CE · C++ → WASM;
- a Ranger compiler with the Rust-syntax fixes (4.0.1, or a Ranger checkout
  at `RANGER_ROOT`) for CEr · JS.

A step whose toolchain is missing is skipped and its engine greyed out on
the page with the reason; `--strict` (CI) fails instead, `--skip=<ids>`
skips steps on purpose.

wasi-sdk's C++ runtime has no exception support, so the C++ build is
compiled with `-fno-exceptions`. rgrc's C++ output throws in two places,
neither on a path a working script takes; `ce-wasm/patch-cpp.mjs` rewrites
them. The engine's own JavaScript exceptions are values, not C++ exceptions.

## Publishing

`.github/workflows/pages.yml` builds and smoke-tests on every push and pull
request and deploys `main` to GitHub Pages. The repository's
**Settings → Pages → Source** has to be **GitHub Actions** once.

## Adding an engine

An adapter in `src/engines/` exports `{ id, name, about, load(base) }`;
`load` answers `{ run(src) → { out: [lines], error } }`. Add its id to
`ENGINES` in `build.mjs`. The worker bundle, the manifest entry and the
page's checkbox follow from that.
