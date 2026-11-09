// SPDX-License-Identifier: AGPL-3.0-or-later
// Hermes (Meta, MIT) compiled to WebAssembly by emscripten
// (playground/hermes/build.sh): the VM's command line, called once per run
// in a fresh instance of the compiled module, so every script gets a new
// realm.
export default {
  id: "hermes",
  name: "Hermes · C++ → WASM",
  about: "Meta's Hermes, the bytecode VM of React Native, in C++, compiled by emscripten (playground/hermes/build.sh). Each run is a new instance.",
  async load(base) {
    const { default: factory } = await import(base + "wasm/hermes.mjs");
    const module = await WebAssembly.compileStreaming(fetch(base + "wasm/hermes.wasm"));
    // what the other engines have and Hermes's command line does not
    const prelude = "var console = { log: print }; var performance = { now: function () { return Date.now(); } };\n";
    let out = [];
    let err = [];
    const make = () =>
      factory({
        print: (s) => out.push(s),
        printErr: (s) => err.push(s),
        instantiateWasm(imports, done) {
          WebAssembly.instantiate(module, imports).then((inst) => done(inst, module));
          return {};
        },
      });
    // the next run's instance is made while the page waits between runs
    let next = make();
    return {
      async run(src) {
        const m = await next;
        next = make();
        out = [];
        err = [];
        m.FS.writeFile("/script.js", prelude + src);
        let code = 0;
        try {
          code = m.callMain(["-w", "/script.js"]);
        } catch (e) {
          if (e && typeof e.status === "number") code = e.status;
          else {
            code = 1;
            err.push(String(e && e.message ? e.message : e));
            // where it aborted, by function name (--profiling-funcs)
            if (e && e.stack) err.push(String(e.stack).split("\n").slice(1, 12).map((l) => l.trim()).join(" < "));
          }
        }
        if (code !== 0) {
          const lines = err.filter((l) => l.trim());
          const line = lines.join(" | ") || "exit " + code;
          out.push(line.startsWith("Uncaught") ? line : "Uncaught " + line);
        }
        return { out, error: code !== 0 };
      },
    };
  },
};
