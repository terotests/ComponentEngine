// SPDX-License-Identifier: AGPL-3.0-or-later
// ComponentEngine (the Ranger sources in engine/) compiled by rgrc to C++
// and by wasi-sdk's clang to WebAssembly.
import { makeWasi } from "../wasi.js";

export default {
  id: "ce-wasm",
  name: "CE · Ranger → C++ → WASM",
  about: "ComponentEngine's Ranger sources (engine/), compiled by rgrc to C++ and by clang (wasi-sdk) to WebAssembly.",
  async load(base) {
    let out = [];
    const wasi = makeWasi((_fd, line) => out.push(line.replace(/^\[tsx\]\s*/, "")));
    const bytes = await (await fetch(base + "wasm/ce.wasm")).arrayBuffer();
    const { instance } = await WebAssembly.instantiate(bytes, wasi.imports);
    const x = instance.exports;
    wasi.setMemory(x.memory);
    if (x._initialize) x._initialize();
    const enc = new TextEncoder();
    return {
      run(src) {
        out = [];
        const data = enc.encode(src);
        const p = x.ce_alloc(data.length);
        new Uint8Array(x.memory.buffer, p, data.length).set(data);
        x.ce_eval(p, data.length);
        x.ce_free(p);
        wasi.flushAll();
        const error = out.some((l) => l.startsWith("Uncaught"));
        return { out, error };
      },
    };
  },
};
