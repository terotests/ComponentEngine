// SPDX-License-Identifier: AGPL-3.0-or-later
// Hermes (Meta, MIT) compiled to WebAssembly by emscripten
// (playground/hermes/build.sh): host.cpp keeps one Hermes runtime in the
// instance, as the other engines keep their VM, so a run is compile and
// execute; starting the runtime is part of loading.
export default {
  id: "hermes",
  name: "Hermes · C++ → WASM",
  about: "Meta's Hermes, the bytecode VM of React Native, in C++, compiled by emscripten (playground/hermes/build.sh). Scripts are compiled with its optimizer (as hermes -O) and run through the JSI API; no JIT in WebAssembly.",
  async load(base) {
    const { default: factory } = await import(base + "wasm/hermes.mjs");
    const module = await WebAssembly.compileStreaming(fetch(base + "wasm/hermes.wasm"));
    let out = [];
    let err = [];
    // main() makes the runtime while the factory starts the instance
    const make = () =>
      factory({
        print: (s) => out.push(s),
        printErr: (s) => err.push(s),
        instantiateWasm(imports, done) {
          WebAssembly.instantiate(module, imports).then((inst) => done(inst, module));
          return {};
        },
      });
    let m = await make();
    return {
      reset() {
        if (m) m._hermes_reset();
      },
      async run(src) {
        // an abort ends the instance; the next run gets a new one
        if (!m) m = await make();
        out = [];
        err = [];
        const p = m.stringToNewUTF8(src);
        let code;
        try {
          code = m._hermes_eval(p);
          m._free(p);
        } catch (e) {
          m = null;
          const where = e && e.stack ? String(e.stack).split("\n").slice(1, 8).map((l) => l.trim()).join(" < ") : "";
          out.push("Uncaught " + [String(e && e.message ? e.message : e), ...err, where].filter(Boolean).join(" | "));
          return { out, error: true };
        }
        if (code !== 0) out.push("Uncaught " + m.UTF8ToString(m._hermes_error()));
        return { out, error: code !== 0 };
      },
    };
  },
};
