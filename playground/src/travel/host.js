// SPDX-License-Identifier: AGPL-3.0-or-later
//
// One host of the travelling-engine demo, in a Web Worker: a server that
// holds shard k of the data and lets query engines visit it. It loads its own
// cerxes.wasm instance; the page passes everything between hosts as text, as
// a network would.
//
// Messages in -> out (each carries `seq`, echoed back):
//   { type: "init", k, n }            -> { type: "ready", loadMs }
//       makes the resident shard engine: shard k's n rows live in it (mode C)
//   { type: "template", text }        -> { type: "installed", ms }
//       keeps a code-only engine snapshot (mode B makes its engines from it)
//   { type: "hopA", snapshot, key, collect } -> { snapshot, restoreMs, queryMs, takeMs, summary }
//       mode A: the whole engine arrives as text (or is made here when
//       snapshot is ""), reads the shard, and leaves as text again
//   { type: "hopB", carry, key }      -> { carry, makeMs, ioMs, queryMs }
//       mode B: only the data root (__carry as JSON) arrives; the engine is
//       made from the template
//   { type: "mapC", key }             -> { part, queryMs }
//       mode C: the query code arrives; the shard engine answers a partial
//   { type: "summary", setup }       -> { summary }   (mode C's merge)
//   { type: "bench", setup, collect, runs } -> { takeMs[], restoreMs[], text }
//       a snapshot of an engine that ran `setup`, taken and restored `runs` times

import { loadCerxes } from "../sandbox/core.js";
import { SHARD, AGENT, COLLECT } from "./programs.js";

const enc = new TextEncoder();
const dec = new TextDecoder();
const now = () => performance.now();
let x = null;
let k = 0;
let n = 0;
let shard = 0; // the resident shard engine
let template = "";

function put(s) {
  const d = enc.encode(s);
  const p = x.cx_alloc(d.length);
  new Uint8Array(x.memory.buffer, p, d.length).set(d);
  return [p, d.length];
}
const result = () => dec.decode(new Uint8Array(x.memory.buffer, x.cx_result_ptr(), x.cx_result_len()));

function evalIn(e, src) {
  const [p, len] = put(src);
  const failed = x.cx_eval(e, p, len);
  x.cx_free(p, len);
  const r = result();
  if (failed) throw new Error(r);
  return r;
}
function snapshot(e) {
  if (x.cx_snapshot(e)) throw new Error("the engine is busy");
  return result();
}
function restore(text) {
  const [p, len] = put(text);
  const e = x.cx_from_snapshot(p, len);
  x.cx_free(p, len);
  if (!e) throw new Error("not a snapshot");
  return e;
}
const lit = (s) => JSON.stringify(s);

async function handle(m) {
  if (!x) x = await loadCerxes(new URL("../wasm/cerxes.wasm", import.meta.url));
  switch (m.type) {
    case "init": {
      k = m.k;
      n = m.n;
      if (shard) x.cx_drop(shard);
      const t = now();
      shard = x.cx_new();
      evalIn(shard, SHARD);
      evalIn(shard, `globalThis.rows = loadShard(${k}, ${n}); 0`);
      return { type: "ready", loadMs: now() - t };
    }
    case "template": {
      const t = now();
      template = m.text;
      x.cx_drop(restore(template)); // checks it, and warms the instance
      return { type: "installed", ms: now() - t };
    }
    case "hopA": {
      let t = now();
      let e;
      if (m.snapshot) e = restore(m.snapshot);
      else {
        e = x.cx_new();
        evalIn(e, SHARD);
        evalIn(e, AGENT);
      }
      const restoreMs = m.snapshot ? now() - t : 0;
      // the host's local read stands as loadShard inside the visiting engine;
      // the rows are dropped before the engine leaves
      t = now();
      evalIn(e, `{ let rows = loadShard(${k}, ${n}); step(rows, ${lit(m.key)}); rows = null; } 0`);
      if (m.collect) evalIn(e, COLLECT);
      const queryMs = now() - t;
      t = now();
      const text = snapshot(e);
      const takeMs = now() - t;
      const summary = evalIn(e, "summary()");
      x.cx_drop(e);
      return { type: "hoppedA", snapshot: text, restoreMs, queryMs, takeMs, summary };
    }
    case "hopB": {
      let t = now();
      const e = restore(template);
      const makeMs = now() - t;
      t = now();
      evalIn(e, `globalThis.__carry = JSON.parse(${lit(m.carry)}) || __carry; 0`);
      let ioMs = now() - t;
      t = now();
      evalIn(e, `{ let rows = loadShard(${k}, ${n}); step(rows, ${lit(m.key)}); } 0`);
      const queryMs = now() - t;
      t = now();
      const carry = evalIn(e, "JSON.stringify(__carry)");
      ioMs += now() - t;
      const summary = evalIn(e, "summary()");
      x.cx_drop(e);
      return { type: "hoppedB", carry, makeMs, ioMs, queryMs, summary };
    }
    case "mapC": {
      const t = now();
      evalIn(shard, `globalThis.__carry = null; ${m.code}; step(rows, ${lit(m.key)}); 0`);
      const part = evalIn(shard, "JSON.stringify(__carry)");
      return { type: "mappedC", part, queryMs: now() - t };
    }
    case "summary": {
      const e = x.cx_new();
      evalIn(e, m.setup);
      const summary = evalIn(e, "summary()");
      x.cx_drop(e);
      return { type: "summarized", summary };
    }
    case "bench": {
      const e = x.cx_new();
      evalIn(e, m.setup);
      if (m.collect) evalIn(e, COLLECT);
      const takeMs = [];
      const restoreMs = [];
      let text = "";
      for (let i = 0; i < m.runs; i++) {
        let t = now();
        text = snapshot(e);
        takeMs.push(now() - t);
        t = now();
        const r = restore(text);
        restoreMs.push(now() - t);
        x.cx_drop(r);
      }
      x.cx_drop(e);
      return { type: "benched", takeMs, restoreMs, text };
    }
  }
  throw new Error("unknown message " + m.type);
}

self.onmessage = async (ev) => {
  const m = ev.data;
  try {
    self.postMessage({ seq: m.seq, ...(await handle(m)) });
  } catch (e) {
    self.postMessage({ seq: m.seq, type: "error", error: String(e.message || e) });
  }
};
