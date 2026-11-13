# ComponentEngine

A JavaScript / TSX evaluator written in [Ranger](https://github.com/terotests/Ranger),
and CEr, the same evaluator written again as a strict Rust module.

| Path | What |
| --- | --- |
| `engine/` | ComponentEngine in Ranger — the Ranger package `componentengine` (entry `ComponentEngine.rgr`) |
| `cer/` | CEr: the evaluator in Rust; builds with cargo, and with `rgrc` into the Ranger targets |
| `cerxes/` | CErXes: CEr with TypeScript and JSX — a front-end of its own that builds CEr's syntax tree; builds with cargo and with `rgrc` ([`cerxes/README.md`](cerxes/README.md)) |
| `tools/` | the generators of `engine/`'s Unicode and locale tables |
| `test/` | the Node module build (`engine_module.rgr`) and a smoke test |
| `playground/` | the browser playground: CEr as WebAssembly (and, as a curiosity, as JavaScript through Ranger), benchmarked against the browser's own engine and QuickJS — <https://terotests.github.io/ComponentEngine/>; and a game loop in TSX run by CErXes and laid out by EVG — <https://terotests.github.io/ComponentEngine/evg/> |

Both moved here from Ranger (`gallery/game_engine/v2/interp/migrate/src` and
`gallery/game_engine/v2/cer`) in September 2026. The parts of `interp/` that
tie the evaluator to the game engine — the adapter, the registry bridge, the
benchmarks' Octane suites and the runtime-conformance test — stay in Ranger
and fetch this package.

## Using it from a Ranger project

In the project's `ranger.json`:

```json
{
  "dependencies": {
    "componentengine": {
      "git": "https://github.com/terotests/componentengine.git",
      "rev": "<commit>",
      "subdir": "engine"
    }
  }
}
```

then `rgrc install` (writes `ranger.lock`) and

```ranger
Import "pkg:componentengine/ComponentEngine.rgr"
Import "pkg:componentengine/JSXToEVG.rgr"
```

`engine/ranger.json` names its own dependencies, which `rgrc install` fetches
along with it: `ts_parser` (Ranger's `gallery/ts_parser`) and `evg`
(`terotests/evg`, `storm/`, used by `JSXToEVG`). A project that already has
either one names it in its own `ranger.json`; the project's entry wins, so
there is one copy of each. `core/RgNum.rgr` and `core/RgText.rgr` come with
the compiler.

## Building and testing

```sh
npm ci                  # ranger-compiler, the compiler
npm test                # rgrc install, bin/engine_module.cjs, test/smoke.mjs
npm run cer:build       # also CEr to JavaScript, cer/bin/Cer.cjs
cargo run --release --manifest-path cer/Cargo.toml --bin cer -- file.js
cargo run --release --manifest-path cerxes/Cargo.toml --bin cerxes -- app.tsx
```

The compiler is `RANGER_ROOT/dist/rgrc.js` when that is set, else the
`ranger-compiler` package, else a Ranger checkout at `RANGER_DIR` (default
`../Ranger`). CEr's `rgrc` build needs ranger-compiler 4.0.1 or later (the
Rust-syntax fixes CEr was written against); the cargo build takes the
`ranger` prelude crate from Ranger by git revision (`cer/Cargo.toml`).

The CEr benchmarks (`npm run cer:conformance`, `cer:micro`, `cer:octane`)
read the Octane suites and the conformance probes from a Ranger checkout at
`RANGER_DIR` (default `../Ranger`); see [`cer/README.md`](cer/README.md).

## What has been tested and run

A summary of the test suites, their latest results and the software that has
been run on CErXes. Every number names its date and the PR that measured it;
rerun the command to get today's.

### Test suites in this repository

| Suite | Command | What it checks | Latest result |
| --- | --- | --- | --- |
| Unit and syntax cases | `cargo test --release` in `cer/` and `cerxes/` (CI) | `cer/tests/cases.txt`, `cerxes/tests/cases.txt` (JS / TS / TSX / JSX), the same cases on the `rgrc` JavaScript, C++ and Go builds (`cerxes/bench/cases.mjs`); `cerxes/tests/*.rs`: hostile input, limits, lone surrogates, host calls | green on every merge (required "All tests" check) |
| Runtime conformance | `npm run cer:conformance`, `npm run cerxes:conformance` (CI) | Ranger's 2,143 runtime-conformance probes against Node; CI fails below 1,667 | CErXes 1,938 / 2,143 agree with Node |
| Test262 | runner outside the repo (a sparse tc39/test262 checkout) | the full suite, strict and sloppy | 94.90 % excluding Temporal (r17, #75, 2026-11-08; QuickJS 95.45 %, QuickJS-ng 95.39 % on the same run). Later PRs check the affected directories for regressions |
| Known V8 bug classes | `node cerxes/tests/v8/run.mjs own` | 593 tests in 11 classes ([`cerxes/tests/v8/README.md`](cerxes/tests/v8/README.md)) on the register VM, stack VM, tree compiler, snapshot-every-slice and the WebAssembly JIT, against Node | 494 / 593 same as Node in every configuration; over all runs 2,654 same, 304 differ, 7 hangs, no panics, aborts, OOM or wasm traps (#160, 2026-11-13; crash verdicts were 220 on the first run) |
| V8 mjsunit | `node cerxes/tests/v8/run.mjs mjsunit --v8=<checkout>` | V8's own regression tests | 2,869 / 3,777 pass on the register VM (Node 3,272; most of the rest test V8 internals), #153, 2026-11-13 |
| Fuzzing | `cerxes/fuzz/run-local.sh`; nightly `.github/workflows/fuzz.yml` | generated and mutated programs in worker processes on the checked build; oracles for panics, crashes, hangs, OOM, slice gaps, forged snapshots and code-cache units, preemption and time limits, register vs stack VM, pcomp vs tree compiler, TSX, snapshot/restore, wasm32 and the JIT, Node ([`cerxes/fuzz/README.md`](cerxes/fuzz/README.md)) | v1 #141 (2026-11-12), v2 #156 (2026-11-13); findings fixed in #154, #158, #159; open ones in `cerxes/fuzz/known/known.txt` |
| Host limits | `cargo test` (`cerxes/tests/limits.rs`) and the fuzzer's `deadline` oracle | `Engine::set_time_limit`, the regex backtrack budget, depth and size limits, snapshot / unit validation | #154, #158 (2026-11-13) |

### JavaScript software run on CErXes

- **73 open-source libraries** with `node cerxes/bench/libs.mjs`: 73 / 73 give
  V8's results (#146, 2026-11-13). Among them React-style UI (preact,
  vue-reactivity, mobx, immer, redux-toolkit, xstate, rxjs), compilers and
  parsers (typescript, babel-standalone, babel-parser, prettier, terser,
  sucrase, acorn, esprima, escodegen, coffeescript, peggy, postcss, csso,
  parse5, htmlparser2, graphql, markdown-it, marked), data and maths (lodash,
  ramda, underscore, immutable, d3, mathjs, decimal.js, big.js, bignumber.js,
  bn.js, date-fns, dayjs, luxon, moment, zod, ajv, superstruct), files and
  crypto (pdf-lib, jszip, fflate, pako, protobufjs, yjs, crypto-js,
  tweetnacl, elliptic, js-sha256), three, matter-js, katex, highlight.js; the
  full list is `cerxes/bench/libs/cases/`.
- **Benchmarks** against V8, QuickJS, Hermes and SpiderMonkey: Octane v9 in
  the playground's Zoo tab (<https://terotests.github.io/ComponentEngine/>),
  the Zoo's 1,370 ES1–ES2025 conformance tests, React and Chart.js bundles
  (the per-PR instruction-count gate), `cerxes/bench/cpu` and
  `cerxes/bench/features` micro benchmarks.
- **PDFKit** 0.20.2, 10–100 page documents, identical bytes in Node, CErXes
  native, CErXes WebAssembly with and without the JIT, and Hermes
  (`cerxes/bench/pdfkit.mjs`, #85, 2026-11-09).
- **Excel generation** (Uint8Array loops, CRC-32, UTF-8; #142, #152).
- **three.js** r185 unchanged over a WebGL2 bridge, pixels identical to V8,
  and RangerThree scenes (terotests/RangerThree, 2026-11-12).

### UI structures run on CErXes

These run in [MFilesAPI](https://github.com/terotests/MFilesAPI): JavaScript
in CErXes, the DOM and CSS in its own `web/dom`, layout and drawing by EVG.

- React UI extensions and dashboards, including the Aino assistant UI.
- Chart.js live dashboards (canvas 2D), React Flow (drag, edges).
- The Monaco editor: renders, highlights, types, scrolls (2026-11-13).
- `.tsx` documents and games with React Three Fiber naming.
- A UI library battery (`desktop/tests/ui-bench`, 2026-11-13) of 19 cases
  against Chromium: js-framework-benchmark (vanilla, React, Preact),
  Bootstrap 5, PrimeReact, shadcn + Tailwind, AG Grid, TanStack Virtual,
  Motion, Shoelace, Golden Layout, Interact and synthetic DOM / CSS tests.
  9 of 19 ran all their steps; the slow part was `web/dom`'s restyle and
  render, not the JavaScript.

## License

The sources are **AGPL-3.0-or-later**, as their SPDX headers say; the full
text is [`LICENSE-AGPL-3.0`](LICENSE-AGPL-3.0). They were AGPL in Ranger's
`gallery/` and moved here unchanged. The MIT `LICENSE` file this repository
was created with does not cover them.
