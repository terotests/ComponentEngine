// SPDX-License-Identifier: AGPL-3.0-or-later
// CErXes (cerxes/: CEr's VM with a TypeScript + JSX front end) built by cargo
// for wasm32-wasip1 (playground/cerxes-wasm). The engines cerxes-*.js are its
// variants: which build (`wasm`), which machine (`flags`, cx_new_with) and
// whether the baseline JIT (cerxes::wjit, a build with the `wjit` feature)
// compiles hot register code to WebAssembly (`jit`: the hotness threshold).
// `slice` (ms) runs scripts in time slices, so the engine can be stopped and
// written out as text at any branch, call or return (cx_set_slice); with
// `move` (ms) as well, the running engine is written out as a snapshot and
// replaced by a new engine made from it every `move` ms of the run, as an
// engine that travels between hosts would be (cx_snapshot, cx_from_snapshot).
import { makeWasi } from "../wasi.js";

const HELPERS = ["get_elem", "set_elem", "get_prop", "set_prop", "op", "call", "slow", "ret"];

/** The JIT's host side, as cerxes/bench/wjit.mjs has it: CErXes hands over a
 * module's bytes (cerxes_jit.install), the host instantiates it against the
 * engine's memory and table and puts its functions f0, f1, … (a batch) in
 * consecutive table slots. The first module
 * is the runtime (look, codes, hstore, callx, pget) the others import. */
function jitHost(getExports) {
  let helpers = null;
  let rt = null;
  return {
    // an evicted module's slots get the runtime's `gone` (back to the
    // interpreter at once), so that nothing holds the module any more
    release(base, n) {
      const t = getExports().__indirect_function_table;
      for (let k = 0; k < n; k++) t.set(base + k, rt.gone);
    },
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
      // a batch: f0, f1, … into consecutive slots; the first one's index
      let n = 0;
      while (inst.exports["f" + n]) n++;
      const idx = t.grow(n);
      for (let k = 0; k < n; k++) t.set(idx + k, inst.exports["f" + k]);
      return idx;
    },
  };
}

export function cerxes({ id, name, about, wasm, flags, jit, slice, move, optional }) {
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
      const { instance } = await WebAssembly.instantiate(bytes, { ...wasi.imports, cerxes_jit: { install: (p, n) => host.install(p, n), release: (b, n) => host.release(b, n) } });
      x = instance.exports;
      wasi.setMemory(x.memory);
      if (x._initialize) x._initialize();
      const enc = new TextEncoder();
      const dec = new TextDecoder();
      const make = () => {
        const e = x.cx_new_with(flags);
        if (jit && !x.cx_jit(e, 1, jit)) throw new Error("this cerxes build has no JIT");
        if (slice) x.cx_set_slice(e, slice);
        return e;
      };
      let engine = make();
      const read = (p, n) => dec.decode(new Uint8Array(x.memory.buffer, p, n));
      // the engine as text, and a new engine from that text in its place
      const relocate = () => {
        if (x.cx_snapshot(engine)) return;
        const n = x.cx_result_len();
        const p = x.cx_alloc(n);
        new Uint8Array(x.memory.buffer, p, n).set(new Uint8Array(x.memory.buffer, x.cx_result_ptr(), n));
        const moved = x.cx_from_snapshot(p, n);
        x.cx_free(p, n);
        if (!moved) throw new Error("the snapshot did not restore");
        x.cx_drop(engine);
        engine = moved;
        x.cx_set_slice(engine, slice);
      };
      return {
        reset() {
          x.cx_drop(engine);
          engine = make();
        },
        run(src) {
          const data = enc.encode(src);
          const p = x.cx_alloc(data.length);
          new Uint8Array(x.memory.buffer, p, data.length).set(data);
          let printed = "";
          let failed;
          if (!slice) {
            failed = x.cx_eval(engine, p, data.length);
            x.cx_free(p, data.length);
            printed = read(x.cx_output_ptr(), x.cx_output_len());
          } else {
            // 2: the slice ran out; what each slice printed is read before
            // the next one (or a move) starts
            let status = x.cx_start(engine, p, data.length);
            x.cx_free(p, data.length);
            let last = performance.now();
            for (;;) {
              printed += read(x.cx_output_ptr(), x.cx_output_len());
              if (status !== 2) break;
              if (move && performance.now() - last >= move) {
                relocate();
                last = performance.now();
              }
              status = x.cx_resume(engine);
            }
            failed = status;
          }
          printed = printed.replace(/\n$/, "");
          const out = printed ? printed.split("\n") : [];
          if (failed) out.push(read(x.cx_result_ptr(), x.cx_result_len()));
          return { out, error: failed !== 0 };
        },
      };
    },
  };
}
