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

## Zoo ranking

The **Zoo ranking** tab runs [zoo.js.org](https://zoo.js.org/)'s two
measurements on every ticked engine and places the results among the ~100
engines zoo.js.org publishes (amd64):

- **Conformance**: javascript-zoo's ES1–ES5 tests (pass rate) and its
  compat-table ports (weighted pass rate, ES6 and ES2016+), 1,370 tests, each
  in a fresh realm; a test passes when it prints `<file>: OK`, as in the zoo's
  harness. A minute or two per engine.
- **Speed**: the eight Octane v9 suites; Score is their geometric mean.
  Prepared as `cer/bench/octane.mjs` prepares them. An interpreter takes
  several minutes per suite.

zoo.js.org measures native builds on a server; here the engines are
WebAssembly in a tab. With **Calibrate** on, each suite's score is scaled by
zoo's V8 score over this browser's own, so the browser stands in for zoo's V8.
Results are kept in `localStorage` until the engines' build changes. The
material and its licences are in `zoo/`.

CEr's WASM build calls the few WASI functions it needs (clock, random,
stdout) through `src/wasi.js`; there are no files and no network.

## CErXes + EVG game demo

**Live:** <https://terotests.github.io/ComponentEngine/evg/>

A page with three tabs: **Game**, **TSX** and **CSS**. The TSX tab holds a
game (Breakout) in TypeScript with JSX; the CSS tab its stylesheet. Run
(Ctrl+Enter) compiles the script and starts the loop; editing the CSS
restyles the running game.

```
input ──► CErXes (WASM, in a Worker)  __frame: clicks, tick(dt, input), view()
      ──► element tree (JSON)          functions passed as onClick stay in the engine under an id
      ──► EVG (Ranger → JavaScript)    stylesheet, layout at 640×480, display list, hit test
      ──► evg-html.js                  the display list as <svg>
```

| Part | What |
| --- | --- |
| `cerxes-wasm/` | CErXes behind a C ABI: `cx_eval` loads a script, `cx_call(name, arg)` runs one frame without compiling anything |
| `evg/EvgGameHost.rgr` | the EVG side: the stylesheet, layout, display list and `idAt` (the hit test); compiled by `rgrc` with EVG from `rgrc install` (`evg/ranger.json`) |
| `src/evg/runtime.js` | what CErXes runs before the script: `__frame`, and the JSX elements serialized with their handlers kept by id |
| `src/evg/tree.js` | the tree made into `EVGElement`s: classes, ids, inline `style` through EVG's own `setAttribute` |
| `src/evg/main.js`, `worker.js` | the page, the frame loop, input, and the worker (a script that does not answer in 3 s has its worker ended) |
| `src/evg/games/` | the example game and its stylesheet |

The script's side: `tick(dt, input)` and `view()` every frame, `onKeyDown(key)`
/ `onKeyUp(key)` on keys, `onClick` on elements; `input.keys`, `input.pointer`,
`width` and `height`. EVG's SVG painter and text measurer (`evg-html.js`,
`evg-measure.js`, MIT) are copied from the EVG package at build time.

`node playground/smoke.mjs --evg` loads the page in Chromium and checks that
frames are painted, a key press reaches the script and a click reaches its
handler.

## Building

```sh
npm ci
node playground/build.mjs          # -> playground/dist
node playground/smoke.mjs          # Chromium: runs the benchmark, checks the answers
node playground/smoke.mjs --zoo=conf   # the zoo tab: conformance (or speed, all)
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
