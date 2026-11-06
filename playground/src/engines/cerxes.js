// SPDX-License-Identifier: AGPL-3.0-or-later
// CErXes (cerxes/: CEr's VM with a TypeScript + JSX front end) built by cargo
// for wasm32-wasip1 (playground/cerxes-wasm). The engines cerxes-*.js are its
// variants: which build (`wasm`), which machine (`flags`, cx_new_with) and
// whether the baseline JIT (cerxes::wjit, a build with the `wjit` feature)
// compiles hot register code to WebAssembly (`jit`: the hotness threshold).
import { makeWasi } from "../wasi.js";

const HELPERS = ["get_elem", "set_elem", "get_prop", "set_prop", "op", "call", "slow", "ret"];

/** The JIT's host side, as cerxes/bench/wjit.mjs has it: CErXes hands over a
 * module's bytes (cerxes_jit.install), the host instantiates it against the
 * engine's memory and table and puts its `f` in the table. The first module
 * is the runtime (look, codes, hstore, callx, pget) the others import. */
function jitHost(getExports) {
  let helpers = null;
  let rt = null;
  return {
    install(ptr, len) {
      const x = getExports();
      const t = x.__indirect_function_table;
      const m = new WebAssembly.Module(new Uint8Array(x.memory.buffer, ptr, len).slice());
      // the helpers come from the table: a wasip1 export runs the
      // constructors and destructors around every call
      if (!helpers) helpers = HELPERS.map((_, k) => t.get(x.cerxes_jit_helper(k)));
      const env = { memory: x.memory, table: t };
      HELPERS.forEach((h, k) => (env[h] = helpers[k]));
      const inst = new WebAssembly.Instance(m, rt ? { env, rt } : { env });
      if (inst.exports.look) {
        rt = inst.exports;
        return 0;
      }
      const idx = t.grow(1);
      t.set(idx, inst.exports.f);
      return idx;
    },
  };
}

export function cerxes({ id, name, about, wasm, flags, jit, optional }) {
  return {
    id,
    name,
    about,
    optional: !!optional,
    async load(base) {
      const wasi = makeWasi(() => {});
      const bytes = await (await fetch(base + "wasm/" + wasm)).arrayBuffer();
      let x = null;
      const host = jitHost(() => x);
      const { instance } = await WebAssembly.instantiate(bytes, { ...wasi.imports, cerxes_jit: { install: (p, n) => host.install(p, n) } });
      x = instance.exports;
      wasi.setMemory(x.memory);
      if (x._initialize) x._initialize();
      const enc = new TextEncoder();
      const dec = new TextDecoder();
      const make = () => {
        const e = x.cx_new_with(flags);
        if (jit && !x.cx_jit(e, 1, jit)) throw new Error("this cerxes build has no JIT");
        return e;
      };
      let engine = make();
      const read = (p, n) => dec.decode(new Uint8Array(x.memory.buffer, p, n));
      return {
        reset() {
          x.cx_drop(engine);
          engine = make();
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
