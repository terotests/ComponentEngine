# ComponentEngine

A JavaScript / TSX evaluator written in [Ranger](https://github.com/terotests/Ranger),
and CEr, the same evaluator written again as a strict Rust module.

| Path | What |
| --- | --- |
| `engine/` | ComponentEngine in Ranger — the Ranger package `componentengine` (entry `ComponentEngine.rgr`) |
| `cer/` | CEr: the evaluator in Rust; builds with cargo, and with `rgrc` into the Ranger targets |
| `cerxes/` | CErXes: CEr with TypeScript and JSX — a front-end of its own that builds CEr's syntax tree ([`cerxes/README.md`](cerxes/README.md)) |
| `tools/` | the generators of `engine/`'s Unicode and locale tables |
| `test/` | the Node module build (`engine_module.rgr`) and a smoke test |
| `playground/` | the browser playground: CEr as WebAssembly (and, as a curiosity, as JavaScript through Ranger), benchmarked against the browser's own engine and QuickJS — <https://terotests.github.io/ComponentEngine/> |

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

## License

The sources are **AGPL-3.0-or-later**, as their SPDX headers say; the full
text is [`LICENSE-AGPL-3.0`](LICENSE-AGPL-3.0). They were AGPL in Ranger's
`gallery/` and moved here unchanged. The MIT `LICENSE` file this repository
was created with does not cover them.
