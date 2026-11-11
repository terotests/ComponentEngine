// SPDX-License-Identifier: AGPL-3.0-or-later
// SpiderMonkey (Mozilla, MPL-2.0) for wasm32-wasi, as the Bytecode Alliance
// builds it (spidermonkey-wasi-embedding, what StarlingMonkey links), with
// playground/spidermonkey/host.cpp around it: one JSContext kept in the
// instance, a new global per reset. No JIT in WebAssembly: scripts run in
// its Portable Baseline Interpreter, which has inline caches.
import { makeWasi } from "../wasi.js";

export default {
  id: "spidermonkey",
  name: "SpiderMonkey · C++ → WASM",
  about: "Mozilla's SpiderMonkey 129 built for WASI by the Bytecode Alliance (the library StarlingMonkey uses), with its Portable Baseline Interpreter: inline caches, no JIT, no Intl (playground/spidermonkey/build.sh).",
  async load(base) {
    const module = await WebAssembly.compileStreaming(fetch(base + "wasm/spidermonkey.wasm"));
    const enc = new TextEncoder();
    const dec = new TextDecoder();
    let out = [];
    let x = null;
    // an instance that trapped (stack overflow, out of memory) is replaced
    const make = async () => {
      const wasi = makeWasi((_fd, line) => out.push(line));
      const { exports } = await WebAssembly.instantiate(module, wasi.imports);
      wasi.setMemory(exports.memory);
      exports._initialize();
      if (exports.sm_reset() !== 0) throw new Error("SpiderMonkey did not start");
      return exports;
    };
    x = await make();
    const cstr = (p) => {
      const m = new Uint8Array(x.memory.buffer);
      let e = p;
      while (m[e]) e++;
      return dec.decode(m.subarray(p, e));
    };
    return {
      reset() {
        if (x && x.sm_reset() !== 0) x = null;
      },
      async run(src) {
        if (!x) x = await make();
        out = [];
        const data = enc.encode(src);
        let code;
        try {
          const p = x.sm_alloc(data.length);
          new Uint8Array(x.memory.buffer, p, data.length).set(data);
          // 0: the Portable Baseline Interpreter with its own warm-up
          code = x.sm_eval(p, data.length, 0);
          x.sm_free(p);
        } catch (e) {
          x = null;
          out.push("Uncaught " + String(e && e.message ? e.message : e));
          return { out, error: true };
        }
        if (code !== 0) {
          const text = cstr(x.sm_error());
          out.push("Uncaught " + text);
          // a heap that ran full stays full: the next run gets a new instance
          if (text === "out of memory") x = null;
        }
        return { out, error: code !== 0 };
      },
    };
  },
};
