# Playground

**Live:** <https://terotests.github.io/ComponentEngine/>

CEr — ComponentEngine's evaluator written in Rust (`cer/`) — compiled to
WebAssembly and running in the browser next to the browser's own engine and
QuickJS. A benchmark runs a set of workloads in every engine you tick and
compares both the time and the answer with the browser's; an editor runs
your own script in all of them.

| Engine | What it is |
| --- | --- |
| Browser JS | the page's own engine (V8, SpiderMonkey, JavaScriptCore), through `new Function`: the reference |
| CEr · Rust → WASM | `cer/` compiled by cargo for `wasm32-wasip1` (`cer-wasm/`, a C ABI) |
| QuickJS · C → WASM | Fabrice Bellard's QuickJS, from `quickjs-emscripten` (MIT) |
| CEr · Rust → Ranger → JS | a curiosity: the same `cer/src/lib.rs` read by the Ranger compiler as a strict Rust module and written out as JavaScript |

Each engine runs in a Web Worker of its own, one engine at a time, so no two
compete for the CPU; a run past the time limit ends its worker. Times are
the median of the runs, wall-clock around the engine's eval call, parsing
included and loading excluded. The chart is the geometric mean, over the
workloads an engine answered as the browser did, of its time over the
browser's.

CEr's WASM build calls the few WASI functions it needs (clock, random,
stdout) through `src/wasi.js`; there are no files and no network.

## Building

```sh
npm ci
node playground/build.mjs          # -> playground/dist
node playground/smoke.mjs          # Chromium: runs the benchmark, checks the answers
npx serve playground/dist          # or any static server
```

`build.mjs` needs `rustup target add wasm32-wasip1` for CEr · WASM, and
ranger-compiler 4.0.1 or later (the devDependency) for CEr · JS. An engine
whose build fails is greyed out on the page with the reason; `--strict` (CI)
fails instead.

## Publishing

`.github/workflows/pages.yml` builds and smoke-tests on every push and pull
request and deploys `main` to GitHub Pages. The repository's
**Settings → Pages → Source** has to be **GitHub Actions** once.

## Adding an engine

An adapter in `src/engines/` exports `{ id, name, about, load(base) }`;
`load` answers `{ run(src) → { out: [lines], error } }`. Add its id to
`ENGINES` in `build.mjs`. The worker bundle, the manifest entry and the
page's checkbox follow from that.
