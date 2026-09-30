// SPDX-License-Identifier: AGPL-3.0-or-later
//
// CErXes (cerxes-wasm) in a Worker of its own, so a script that never returns
// costs the page nothing: the page stops waiting and ends the worker.
//
//   { type: "load", runtime, source }  a fresh engine; runs the runtime, then
//                                      the script        -> { type: "loaded" }
//   { type: "frame", arg }             __frame(arg)       -> { type: "frame", tree }
//
// Every reply carries `ok`, `error` (the uncaught exception or syntax error),
// `output` (console.log lines) and `ms` (time inside the engine).
import { makeWasi } from "../wasi.js";

let x = null;
let engine = 0;
let frameName = null;
const enc = new TextEncoder();
const dec = new TextDecoder();

async function boot() {
  const wasi = makeWasi(() => {});
  const bytes = await (await fetch(new URL("./cerxes.wasm", import.meta.url))).arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, wasi.imports);
  x = instance.exports;
  wasi.setMemory(x.memory);
  if (x._initialize) x._initialize();
}

function put(s) {
  const d = enc.encode(s);
  const p = x.cx_alloc(d.length);
  new Uint8Array(x.memory.buffer, p, d.length).set(d);
  return [p, d.length];
}

function read(ptr, len) {
  return dec.decode(new Uint8Array(x.memory.buffer, ptr, len));
}

function eval_(src) {
  const [p, n] = put(src);
  const failed = x.cx_eval(engine, p, n);
  x.cx_free(p, n);
  return { ok: failed === 0, value: read(x.cx_result_ptr(), x.cx_result_len()), output: read(x.cx_output_ptr(), x.cx_output_len()) };
}

const ready = boot();

self.onmessage = async (ev) => {
  const m = ev.data;
  try {
    await ready;
    if (m.type === "load") {
      if (engine) x.cx_drop(engine);
      engine = x.cx_new();
      if (!frameName) frameName = put("__frame");
      const t = performance.now();
      const rt = eval_(m.runtime);
      if (!rt.ok) {
        self.postMessage({ type: "loaded", ok: false, error: "runtime: " + rt.value, output: rt.output, ms: 0 });
        return;
      }
      const r = eval_(m.source);
      self.postMessage({ type: "loaded", ok: r.ok, error: r.ok ? "" : r.value, output: r.output, ms: performance.now() - t });
    } else if (m.type === "frame") {
      const [ap, an] = put(m.arg);
      const t = performance.now();
      const failed = x.cx_call(engine, frameName[0], frameName[1], ap, an);
      const ms = performance.now() - t;
      x.cx_free(ap, an);
      const value = read(x.cx_result_ptr(), x.cx_result_len());
      const output = read(x.cx_output_ptr(), x.cx_output_len());
      self.postMessage(failed === 0
        ? { type: "frame", ok: true, tree: value, error: "", output, ms }
        : { type: "frame", ok: false, tree: "", error: value, output, ms });
    }
  } catch (e) {
    self.postMessage({ type: m.type === "load" ? "loaded" : "frame", ok: false, error: "engine: " + String((e && e.message) || e), output: "", ms: 0 });
  }
};
