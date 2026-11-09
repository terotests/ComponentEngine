// SPDX-License-Identifier: AGPL-3.0-or-later
// Facebook Hermes (static_h) compiled by Emscripten to WebAssembly: the
// hermes CLI, running in a worker. Each realm is a fresh module instance
// of a compiled WebAssembly.Module. Octane's timer is performance.now;
// Hermes has print and console.log, but not performance, so a small
// prelude defines it.

const PRELUDE = `if (typeof performance === "undefined") {
  var performance = { now: function () { return Date.now(); } };
}
`;

function exitOk(e) {
  if (!e) return true;
  if (e.name === "ExitStatus" && (e.status === 0 || e.status == null)) return true;
  if (typeof e.status === "number") return e.status === 0;
  return false;
}

export default {
  id: "hermes-wasm",
  name: "Hermes · C++ → WASM",
  about: "Facebook Hermes (static_h) compiled by Emscripten to WebAssembly and shipped with this page: the same interpreter zoo.js.org measures natively, running in this tab.",
  optional: true,
  async load(base) {
    const wasmBinary = await (await fetch(base + "wasm/hermes.wasm")).arrayBuffer();
    const compiled = await WebAssembly.compile(wasmBinary);
    const factory = (await import(base + "wasm/hermes.mjs")).default;
    const make = async () => {
      const out = [];
      const push = (s) => {
        const t = String(s);
        if (t.includes("\n")) out.push(...t.split("\n").filter((l, i, a) => l.length || i < a.length - 1));
        else out.push(t);
      };
      const mod = await factory({
        print: push,
        printErr: push,
        noInitialRun: true,
        instantiateWasm(imports, done) {
          WebAssembly.instantiate(compiled, imports).then(done);
        },
      });
      return { mod, out, used: false };
    };
    let inst = await make();
    return {
      async reset() {
        inst = await make();
      },
      async run(src) {
        if (inst.used) inst = await make();
        inst.used = true;
        inst.out.length = 0;
        inst.mod.FS.writeFile("/t.js", PRELUDE + src);
        let error = false;
        try {
          inst.mod.callMain(["-w", "/t.js"]);
        } catch (e) {
          // Hermes runs the script, then the Emscripten runtime aborts in
          // llvm_shutdown / atexit. The JS already printed; treat that as
          // a finished run unless nothing came out.
          const abort = /Aborted/i.test(String(e && e.message ? e.message : e));
          if (!exitOk(e) && !abort) {
            error = true;
            inst.out.push(String(e && e.message ? e.message : e));
          }
        }
        const out = inst.out.filter((l) => !/^Aborted/.test(l) && l !== "Aborted()");
        while (out.length && out[out.length - 1] === "") out.pop();
        return { out, error };
      },
    };
  },
};
