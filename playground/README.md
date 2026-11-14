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
| CErXes · register VM | `cerxes/` (CEr's VM with a TypeScript + JSX front end) by cargo for `wasm32-wasip1` (`cerxes-wasm/`), on the register machine |
| CErXes · stack VM | the same `cerxes.wasm` on the older stack machine, to compare the two interpreters |
| CErXes · small build | `cerxes-wasm/` at `opt-level = "s"`: a smaller download for some speed |
| CErXes · detachable | `cerxes.wasm` on the register VM in 4 ms time slices (`cx_set_slice`): the running engine can be stopped and written out as a snapshot at any branch, call or return. Its score against the register VM's is what being movable costs |
| CErXes · moving | as detachable, and every 100 ms of a run the engine is written out (`cx_snapshot`) and replaced by a new engine made from that text (`cx_from_snapshot`), mid-script: what moving costs |
| CErXes · JIT | `cerxes-wasm/` with the `wjit` feature: hot register code (64 runs) is compiled to WebAssembly modules the page instantiates; the engine falls back to the interpreter wherever the JIT exits |
| QuickJS · C → WASM | Fabrice Bellard's QuickJS, from `quickjs-emscripten` (MIT) |
| CEr · Rust → Ranger → JS | a curiosity: the same `cer/src/lib.rs` read by the Ranger compiler as a strict Rust module and written out as JavaScript |

The CErXes engines also run TypeScript and JSX in the editor (the other
engines answer those with a syntax error).

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
WebAssembly in a tab. With **Calibrate** on, every score is scaled by one
factor for the machine: the median over the suites of zoo's V8 score over this
browser's own (one factor, not one per suite: a suite's ratio also holds how
this browser's engine differs from zoo's V8 in that suite).
Results are kept in `localStorage` until the engines' build changes. The
material and its licences are in `zoo/`.

CEr's WASM build calls the few WASI functions it needs (clock, random,
stdout) through `src/wasi.js`; there are no files and no network.

## CErXes sandbox demo

**Live:** <https://terotests.github.io/ComponentEngine/sandbox/>

Four CErXes engines, each running a program that never ends: a fractal
(Mandelbrot, a Julia set, Burning Ship, z³ + c) drawn in passes of finer
blocks and then ever more iterations. CErXes stops a script by itself when
its time slice runs out (`Engine::set_slice`, at a branch, call or return,
inside `while (true)` too) and goes on with `resume`, so all four share one
thread and the page stays responsive. Each card picks its host:

| Host | What runs where |
| --- | --- |
| page | one `cerxes.wasm` instance on the page, one engine per card, the cards' slices in turn between the page's tasks |
| Worker | a Web Worker per card: a thread of its own; the slices let the worker take messages (zoom, freeze) while it computes |
| iframe | a hidden same-origin iframe per card: a document and instance of its own, on the page's thread |

Clicking a picture zooms in (Shift-click or right-click: out); the program
sees the new `view` at its next row and starts the new picture. **Freeze**
writes the engine's whole state, the suspended loop included, as text
(`cx_snapshot`, ~300 KB); **Thaw** makes a new engine from it. Changing a
card's host moves the running program that way. **Fetch results** reads each
program's `result` global between two slices without running any code. Each
card's **Code** button opens its program for editing.

| Part | What |
| --- | --- |
| `cerxes-wasm/src/lib.rs` | `cx_set_slice`, `cx_start`, `cx_resume` (0 done, 1 threw, 2 slice ran out), `cx_set_global` / `cx_get_global`, `cx_snapshot` / `cx_from_snapshot` |
| `src/sandbox/core.js` | the engine side, the same in every host: one engine per sandbox, a round of slices, then a yield |
| `src/sandbox/programs.js` | the four programs and their protocol (`view`, `progress`, `result`, `row` lines) |
| `src/sandbox/main.js`, `worker.js`, `frame.js` | the page, the Worker host, the iframe host |

`node playground/smoke.mjs --sandbox` checks it in Chromium.

## CErXes + EVG game demo

**Live:** <https://terotests.github.io/ComponentEngine/evg/>

A page with three tabs: **Game**, **TSX** and **CSS**. The TSX tab holds a
game in TypeScript with JSX, the CSS tab its stylesheet; the Example menu (or
`?game=`) picks one of two:

- **Scaffold Scramble** (`scaffold`): a climbing game. Pip the builder
  climbs slanted girders to a kitten while Grumble, a big crane-bot on
  treads, rolls cable spools down at them; jump them, or pick up a wrench and
  smash them. Every sprite is a small tree of boxes dressed by the
  stylesheet (gradients, rounded corners, rotation). The night sky, the city,
  the girders and the ladders are built once: the runtime serializes an
  unchanged JSX element only once, which keeps a 350-element frame at about
  2.5 ms in CErXes. On a touch screen the page shows arrow and JUMP buttons;
  **Play full screen** (or `?game=scaffold&play`) leaves only the game and
  the buttons, laid out for a phone held either way.
- **Breakout** (`breakout`). Run
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
| `src/evg/games/` | the example games and their stylesheets |

The script's side: `tick(dt, input)` and `view()` every frame, `onKeyDown(key)`
/ `onKeyUp(key)` on keys, `onClick` on elements; `input.keys`, `input.pointer`,
`width` and `height`. EVG's SVG painter and text measurer (`evg-html.js`,
`evg-measure.js`, MIT) are copied from the EVG package at build time.

`node playground/smoke.mjs --evg` loads the page in Chromium and checks that
each example paints frames and answers its input (a key press and a click
on Breakout's HUD button; starting, walking and climbing in Scaffold
Scramble).

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

## Travelling engine

**Live:** <https://terotests.github.io/ComponentEngine/travel/>

A CErXes engine written out as text and made again from it. Each host is a
Web Worker with its own `cerxes.wasm` instance and one shard of generated
rows; the page carries text between the hosts as a network would and
estimates the network's share from each transfer's gzip size, round trip and
bandwidth.

- **Snapshot speed**: take, restore and size (raw and gzip) of an empty
  engine, the query code alone, 50 000 and 200 000 live rows, and 50 000
  dropped rows before and after a collection (`gc()` only asks for one; it
  runs at the next call or backward jump).
- **One query, three ways**: A, the whole engine travels host to host; B, only
  its data root (`__carry`, JSON) travels into engines each host makes from a
  code-only template snapshot it was given once; C, map-reduce. The three
  answers have to agree (`smoke.mjs --travel`).
