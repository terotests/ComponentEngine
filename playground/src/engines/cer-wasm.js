// SPDX-License-Identifier: AGPL-3.0-or-later
// CEr (the evaluator in Rust) built by cargo for wasm32-wasip1.
import { makeWasi } from "../wasi.js";

export default {
  id: "cer-wasm",
  name: "CEr · Rust → WASM",
  about: "ComponentEngine's evaluator written in Rust (cer/), compiled by cargo to WebAssembly.",
  async load(base) {
    const out = [];
    const wasi = makeWasi((_fd, line) => out.push(line));
    const bytes = await (await fetch(base + "wasm/cer.wasm")).arrayBuffer();
    const { instance } = await WebAssembly.instantiate(bytes, wasi.imports);
    const x = instance.exports;
    wasi.setMemory(x.memory);
    if (x._initialize) x._initialize();
    const enc = new TextEncoder();
    const dec = new TextDecoder();
    let engine = x.cer_new();
    return {
      reset() {
        x.cer_drop(engine);
        engine = x.cer_new();
      },
      run(src) {
        const data = enc.encode(src);
        const p = x.cer_alloc(data.length);
        new Uint8Array(x.memory.buffer, p, data.length).set(data);
        const failed = x.cer_eval(engine, p, data.length);
        x.cer_free(p, data.length);
        const text = dec.decode(new Uint8Array(x.memory.buffer, x.cer_result_ptr(), x.cer_result_len()));
        return { out: text ? text.split("\n") : [], error: failed !== 0 };
      },
    };
  },
};
