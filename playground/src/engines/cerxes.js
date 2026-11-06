// SPDX-License-Identifier: AGPL-3.0-or-later
// CErXes (cerxes/: CEr's VM with a TypeScript + JSX front end) built by cargo
// for wasm32-wasip1 (playground/cerxes-wasm). The engines cerxes-*.js are its
// variants: which build (`wasm`) and which machine (`flags`, cx_new_with).
import { makeWasi } from "../wasi.js";

export function cerxes({ id, name, about, wasm, flags }) {
  return {
    id,
    name,
    about,
    async load(base) {
      const wasi = makeWasi(() => {});
      const bytes = await (await fetch(base + "wasm/" + wasm)).arrayBuffer();
      const { instance } = await WebAssembly.instantiate(bytes, wasi.imports);
      const x = instance.exports;
      wasi.setMemory(x.memory);
      if (x._initialize) x._initialize();
      const enc = new TextEncoder();
      const dec = new TextDecoder();
      let engine = x.cx_new_with(flags);
      const read = (p, n) => dec.decode(new Uint8Array(x.memory.buffer, p, n));
      return {
        reset() {
          x.cx_drop(engine);
          engine = x.cx_new_with(flags);
        },
        run(src) {
          const data = enc.encode(src);
          const p = x.cx_alloc(data.length);
          new Uint8Array(x.memory.buffer, p, data.length).set(data);
          const failed = x.cx_eval(engine, p, data.length);
          x.cx_free(p, data.length);
          const printed = read(x.cx_output_ptr(), x.cx_output_len()).replace(/\n$/, "");
          const out = printed ? printed.split("\n") : [];
          if (failed) out.push(read(x.cx_result_ptr(), x.cx_result_len()));
          return { out, error: failed !== 0 };
        },
      };
    },
  };
}
