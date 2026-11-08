# Hermes · C++ → WASM

The playground's **Hermes · C++ → WASM** engine is Facebook Hermes
(`static_h`, the build [javascript-zoo](https://github.com/ivankra/javascript-zoo)
measures natively) compiled by Emscripten to WebAssembly, so it can sit next
to CErXes in the Zoo ranking tab.

It is optional: `playground/build.mjs` copies a pre-built `hermes.js` /
`hermes.wasm` when they exist (see below) and otherwise greys the engine out.
CI does not build it.

```sh
# emsdk (once): git clone https://github.com/emscripten-core/emsdk.git ~/emsdk
#               ~/emsdk/emsdk install latest && ~/emsdk/emsdk activate latest
playground/hermes-wasm/build.sh     # -> /tmp/hermes-ws/build-wasm/bin
node playground/build.mjs           # copies those files into playground/dist
ENGINES=native,cerxes-jit,hermes-wasm node playground/smoke.mjs --zoo=all
```

`build.sh` clones `facebook/hermes` at `static_h` into `$HERMES_WS/hermes`
(default `/tmp/hermes-ws`) and does the two-stage host `hermesc` + Emscripten
`hermes` build from Hermes's [Emscripten.md](https://github.com/facebook/hermes/blob/static_h/doc/Emscripten.md).
Override the workspace with `HERMES_WS` and the SDK with `EMSDK`.
