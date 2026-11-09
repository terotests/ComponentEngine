# Hermes · C++ → WASM

The playground's **Hermes · C++ → WASM** engine is Facebook Hermes
(`static_h`, the build [javascript-zoo](https://github.com/ivankra/javascript-zoo)
measures natively) compiled by Emscripten to WebAssembly, so it can sit next
to CErXes in the Zoo ranking tab. `prebuilt/` is that build; `build.mjs`
copies it into `playground/dist` on every playground build, including CI
and GitHub Pages.

The binaries are Hermes's MIT license (`LICENSE` in this directory).

```
prebuilt/hermes.{js,wasm}
  facebook/hermes static_h 51ae2a08ce8cb06fcfa718cddab9c94ea9eee7f6
  Emscripten 7.0.0 (777e6e5db7b4990bcaeb1b9998ed642cc84e6629)
```

To rebuild (cmake, ninja, a host C++ compiler, and emsdk):

```sh
# emsdk (once): git clone https://github.com/emscripten-core/emsdk.git ~/emsdk
#               ~/emsdk/emsdk install latest && ~/emsdk/emsdk activate latest
playground/hermes-wasm/build.sh     # -> /tmp/hermes-ws/build-wasm/bin and prebuilt/
node playground/build.mjs
```

`build.sh` clones `facebook/hermes` at `static_h` into `$HERMES_WS/hermes`
(default `/tmp/hermes-ws`) and does the two-stage host `hermesc` + Emscripten
`hermes` build from Hermes's [Emscripten.md](https://github.com/facebook/hermes/blob/static_h/doc/Emscripten.md).
Override the workspace with `HERMES_WS` and the SDK with `EMSDK`.
`HERMES_WASM_DIR` makes `build.mjs` copy from somewhere else.
