// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The sandboxes' engine side, the same code wherever it runs: on the page,
// in a Web Worker or in an iframe. One WebAssembly instance of CErXes
// (cerxes-wasm) holds one engine per sandbox; a scheduler gives each running
// sandbox one time slice (cx_resume) in turn and yields to the event loop
// after every round, so the host's own events (input, messages, painting)
// get in between.
//
// Messages in (serve's handler):
//   { type: "start", id, code, view, slice }   a fresh engine runs `code`
//   { type: "view", id, view }                  the program's `view` global
//   { type: "slice", ms }                       every sandbox's slice length
//   { type: "pause" | "unpause" | "stop", id }
//   { type: "freeze", id }   -> { type: "frozen", id, snapshot, ms }  the engine
//                               as text; the sandbox is gone from this host
//   { type: "thaw", id, snapshot, slice }        an engine from that text goes on
//   { type: "fetch", id }    -> { type: "fetched", id, progress, result }
// Messages out, once per round for each sandbox that ran:
//   { type: "tick", id, rows, logs, progress, slices, cpu, status, error }

import { makeWasi } from "../wasi.js";

const enc = new TextEncoder();
const dec = new TextDecoder();

export async function loadCerxes(url) {
  const wasi = makeWasi(() => {});
  const bytes = await (await fetch(url)).arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, wasi.imports);
  const x = instance.exports;
  wasi.setMemory(x.memory);
  if (x._initialize) x._initialize();
  return x;
}

export function serve(xPromise, post, yieldNow) {
  let x = null;
  const boxes = new Map(); // id -> { e, paused, waiting, slices, cpu, slice }
  let sliceMs = 4;
  let looping = false;

  const put = (s) => {
    const d = enc.encode(s);
    const p = x.cx_alloc(d.length);
    new Uint8Array(x.memory.buffer, p, d.length).set(d);
    return [p, d.length];
  };
  const result = () => dec.decode(new Uint8Array(x.memory.buffer, x.cx_result_ptr(), x.cx_result_len()));
  const output = () => dec.decode(new Uint8Array(x.memory.buffer, x.cx_output_ptr(), x.cx_output_len()));
  const setGlobal = (e, name, value) => {
    const [np, nn] = put(name);
    const [vp, vn] = put(String(value));
    x.cx_set_global(e, np, nn, vp, vn);
    x.cx_free(np, nn);
    x.cx_free(vp, vn);
  };
  const getGlobal = (e, name) => {
    const [np, nn] = put(name);
    x.cx_get_global(e, np, nn);
    x.cx_free(np, nn);
    return result();
  };

  // What one slice printed and left in `progress`, as a tick message.
  function tick(id, b, status, ms) {
    b.slices++;
    b.cpu += ms;
    const rows = [];
    const logs = [];
    for (const line of output().split("\n")) {
      if (!line) continue;
      if (line.startsWith("row ")) rows.push(line);
      else logs.push(line);
    }
    const m = { type: "tick", id, rows, logs, progress: "", slices: b.slices, cpu: b.cpu, status, error: "" };
    if (status === 1) m.error = result();
    if (status === 2 || status === 0) m.progress = getGlobal(b.e, "progress");
    // a program that drew its last pass waits for a new view: no slices
    // for it until then
    if (/^\d+ done/.test(m.progress)) b.waiting = true;
    if (status !== 2) b.ended = true;
    return m;
  }

  function runnable(b) {
    return !b.paused && !b.waiting && !b.ended;
  }

  async function loop() {
    if (looping) return;
    looping = true;
    while ([...boxes.values()].some(runnable)) {
      for (const [id, b] of boxes) {
        if (!runnable(b)) continue;
        x.cx_set_slice(b.e, sliceMs);
        const t = performance.now();
        const status = x.cx_resume(b.e);
        post(tick(id, b, status, performance.now() - t));
      }
      await yieldNow();
    }
    looping = false;
  }

  function drop(id) {
    const b = boxes.get(id);
    if (!b) return;
    x.cx_drop(b.e);
    boxes.delete(id);
  }

  return async function handle(m) {
    if (!x) x = await xPromise;
    const b = boxes.get(m.id);
    switch (m.type) {
      case "start": {
        drop(m.id);
        if (m.slice) sliceMs = m.slice;
        const e = x.cx_new();
        const nb = { e, paused: false, waiting: false, ended: false, slices: 0, cpu: 0 };
        boxes.set(m.id, nb);
        setGlobal(e, "view", m.view);
        x.cx_set_slice(e, sliceMs);
        const [p, n] = put(m.code);
        const t = performance.now();
        const status = x.cx_start(e, p, n);
        x.cx_free(p, n);
        post(tick(m.id, nb, status, performance.now() - t));
        break;
      }
      case "view":
        if (b) {
          setGlobal(b.e, "view", m.view);
          b.waiting = false;
        }
        break;
      case "slice":
        sliceMs = m.ms;
        break;
      case "pause":
        if (b) b.paused = true;
        break;
      case "unpause":
        if (b) b.paused = false;
        break;
      case "stop":
        drop(m.id);
        break;
      case "fetch":
        post({ type: "fetched", id: m.id, progress: b ? getGlobal(b.e, "progress") : "", result: b ? getGlobal(b.e, "result") : "" });
        break;
      case "freeze": {
        if (!b) {
          post({ type: "frozen", id: m.id, snapshot: "", error: "no such sandbox" });
          break;
        }
        const t = performance.now();
        const failed = x.cx_snapshot(b.e);
        const snapshot = failed ? "" : result();
        const state = { paused: b.paused, waiting: b.waiting, ended: b.ended, slices: b.slices, cpu: b.cpu };
        if (!failed) drop(m.id);
        post({ type: "frozen", id: m.id, snapshot, state, ms: performance.now() - t, error: failed ? "the engine is busy" : "" });
        break;
      }
      case "thaw": {
        drop(m.id);
        if (m.slice) sliceMs = m.slice;
        const [p, n] = put(m.snapshot);
        const t = performance.now();
        const e = x.cx_from_snapshot(p, n);
        x.cx_free(p, n);
        if (!e) {
          post({ type: "thawed", id: m.id, ok: false, error: "not a snapshot" });
          break;
        }
        const s = m.state || {};
        boxes.set(m.id, { e, paused: !!s.paused, waiting: !!s.waiting, ended: !!s.ended, slices: s.slices || 0, cpu: s.cpu || 0 });
        post({ type: "thawed", id: m.id, ok: true, ms: performance.now() - t });
        break;
      }
    }
    loop();
  };
}

// Yielding to the event loop: a message to ourselves is a task of its own
// without setTimeout's 4 ms clamp.
export function messageYield() {
  const ch = new MessageChannel();
  const waiting = [];
  ch.port1.onmessage = () => waiting.shift()();
  return () =>
    new Promise((r) => {
      waiting.push(r);
      ch.port2.postMessage(0);
    });
}

// On the page's thread (the page itself, an iframe) a chain of message tasks
// can hold off painting; a timeout lets the browser paint in between.
export const timeoutYield = () => new Promise((r) => setTimeout(r, 0));
