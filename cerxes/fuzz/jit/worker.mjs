// SPDX-License-Identifier: AGPL-3.0-or-later
//
// cerxes-fuzz's WebAssembly + JIT worker (README "JIT"). Loads the
// cerxes-wasm build with the baseline JIT (jit/build.sh), reads programs
// framed as `<len>\n<bytes>` on stdin and answers `<len>\n<json>` with the
// printed lines of each configuration:
//   off   the WebAssembly interpreter, JIT off
//   on0   JIT on, every function compiled at once (min 0)
//   on64  JIT on at the product threshold (64 back edges)
// Each run is a fresh instance in 50 ms slices with a 3 s budget; a trap
// (a Rust panic is an abort in wasm) is reported as `TRAP: …`.
//
//   node cerxes/fuzz/jit/worker.mjs path/to/cerxes_wasm.wasm

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const { makeWasi } = await import(path.join(here, "../../../playground/src/wasi.js"));
const wasmPath = process.argv[2];
const wasmModule = new WebAssembly.Module(fs.readFileSync(wasmPath));
const enc = new TextEncoder();
const dec = new TextDecoder();
const HELPERS = ["get_elem", "set_elem", "get_prop", "set_prop", "op", "call", "slow", "ret"];
const BUDGET_MS = 3000;

function engine() {
  const out = [];
  const wasi = makeWasi((_fd, line) => out.push(line));
  let x = null;
  const jit = {
    modules: 0,
    release(base, n) {
      const t = x.__indirect_function_table;
      for (let k = 0; k < n; k++) t.set(base + k, jit.rt.gone);
    },
    install(ptr, len) {
      const bytes = new Uint8Array(x.memory.buffer, ptr, len).slice();
      const m = new WebAssembly.Module(bytes); // throws on an invalid module: a finding
      if (!jit.helpers) {
        const t = x.__indirect_function_table;
        jit.helpers = HELPERS.map((h, k) => t.get(x.cerxes_jit_helper(k)));
      }
      const env = { memory: x.memory, table: x.__indirect_function_table };
      HELPERS.forEach((h, k) => (env[h] = jit.helpers[k]));
      const inst = new WebAssembly.Instance(m, jit.rt ? { env, rt: jit.rt } : { env });
      if (inst.exports.look) {
        jit.rt = inst.exports;
        return 0;
      }
      const t = x.__indirect_function_table;
      let n = 0;
      while (inst.exports["f" + n]) n++;
      const idx = t.grow(n);
      for (let k = 0; k < n; k++) t.set(idx + k, inst.exports["f" + k]);
      jit.modules++;
      return idx;
    },
  };
  const inst = new WebAssembly.Instance(wasmModule, {
    ...wasi.imports,
    cerxes_jit: { install: (p, n) => jit.install(p, n), release: (b, n) => jit.release(b, n) },
  });
  x = inst.exports;
  wasi.setMemory(x.memory);
  if (x._initialize) x._initialize();
  const e = x.cx_new();
  const text = (p, n) => dec.decode(new Uint8Array(x.memory.buffer, p, n));
  return {
    jit,
    run(src, on, min) {
      if (x.cerxes_jit_mask) x.cerxes_jit_mask(0, 0);
      x.cx_jit(e, on ? 1 : 0, min);
      x.cx_set_slice(e, 50);
      const b = enc.encode(src);
      const p = x.cx_alloc(b.length);
      new Uint8Array(x.memory.buffer, p, b.length).set(b);
      const lines = [];
      const take = () => {
        const o = text(x.cx_output_ptr(), x.cx_output_len());
        if (o.length) lines.push(...o.split("\n"));
      };
      const t0 = performance.now();
      let st = x.cx_start(e, p, b.length);
      x.cx_free(p, b.length);
      while (st === 2) {
        take();
        if (performance.now() - t0 > BUDGET_MS) return { lines, slow: true };
        st = x.cx_resume(e);
      }
      take();
      lines.push("=> " + text(x.cx_result_ptr(), x.cx_result_len()));
      return { lines, slow: false };
    },
  };
}

function one(src, on, min) {
  try {
    const e = engine();
    const r = e.run(src, on, min);
    r.modules = e.jit.modules;
    return r;
  } catch (err) {
    const msg = String((err && err.message) || err).slice(0, 200);
    return { lines: [(err instanceof WebAssembly.RuntimeError ? "TRAP: " : "HOST ERROR: ") + msg], slow: false, modules: 0, error: true };
  }
}

let buf = Buffer.alloc(0);
let need = -1;
process.stdin.on("data", (chunk) => {
  buf = Buffer.concat([buf, chunk]);
  for (;;) {
    if (need < 0) {
      const nl = buf.indexOf(10);
      if (nl < 0) return;
      need = Number(buf.subarray(0, nl).toString());
      buf = buf.subarray(nl + 1);
    }
    if (buf.length < need) return;
    const src = buf.subarray(0, need).toString();
    buf = buf.subarray(need);
    need = -1;
    const res = { off: one(src, false, 0), on0: one(src, true, 0), on64: one(src, true, 64) };
    const body = Buffer.from(JSON.stringify(res));
    process.stdout.write(body.length + "\n");
    process.stdout.write(body);
  }
});
