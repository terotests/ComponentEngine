// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The travelling-engine demo's page. Each host is a Web Worker (host.js) with
// its own cerxes.wasm instance and one shard of the data; the page carries
// text between them, as a network would, and estimates what that network
// would add from the gzip size of each transfer.
//
// 1. Snapshot speed: an engine's whole state as text (take), a new engine
//    from that text (restore), its size raw and gzip'd.
// 2. One query over all shards, three ways:
//    A  the whole engine moves host to host (snapshot, restore)
//    B  only its data root (__carry, JSON) moves; each host makes the engine
//       from a code-only template snapshot it was given once
//    C  map-reduce: the query code goes to every shard at once, the partials
//       come back and are merged on the first host

import { SHARD, AGENT } from "./programs.js";

const $ = (id) => document.getElementById(id);
const statusEl = $("status");
const now = () => performance.now();
const median = (a) => [...a].sort((p, q) => p - q)[a.length >> 1];
const kb = (b) => (b / 1024 < 10 ? (b / 1024).toFixed(1) : Math.round(b / 1024).toLocaleString("en")) + " KB";
const ms = (t) => (t < 10 ? t.toFixed(1) : Math.round(t).toLocaleString("en")) + " ms";
const bytes = (s) => new TextEncoder().encode(s).length;

async function gzip(s) {
  const t = now();
  const stream = new Blob([s]).stream().pipeThrough(new CompressionStream("gzip"));
  const size = (await new Response(stream).arrayBuffer()).byteLength;
  return { size, ms: now() - t };
}

// ---- hosts ------------------------------------------------------------------

let seq = 0;
function makeHost() {
  const w = new Worker(new URL("./host.js", import.meta.url), { type: "module" });
  const waiting = new Map();
  w.onmessage = (ev) => {
    const m = ev.data;
    const r = waiting.get(m.seq);
    waiting.delete(m.seq);
    if (m.type === "error") r.reject(new Error(m.error));
    else r.resolve(m);
  };
  w.onerror = (ev) => {
    for (const r of waiting.values()) r.reject(new Error(ev.message || "worker failed"));
    waiting.clear();
  };
  return {
    call(m) {
      const s = ++seq;
      return new Promise((resolve, reject) => {
        waiting.set(s, { resolve, reject });
        w.postMessage({ ...m, seq: s });
      });
    },
    close: () => w.terminate(),
  };
}

let hosts = [];
function setHosts(count) {
  while (hosts.length > count) hosts.pop().close();
  while (hosts.length < count) hosts.push(makeHost());
}

// ---- 1. snapshot speed --------------------------------------------------------

const CASES = [
  { id: "empty", name: "Empty engine", setup: "0" },
  { id: "code", name: "Query code, no data (the template)", setup: SHARD + AGENT },
  { id: "rows50", name: "50 000 rows live", setup: SHARD + "globalThis.rows = loadShard(0, 50000); 0" },
  { id: "rows200", name: "200 000 rows live", setup: SHARD + "globalThis.rows = loadShard(0, 200000); 0" },
  { id: "dropped", name: "50 000 rows dropped, gc() asked", setup: SHARD + "{ let r = loadShard(0, 50000); globalThis.count = r.length; } gc(); 0" },
  { id: "collected", name: "50 000 rows dropped, collected", setup: SHARD + "{ let r = loadShard(0, 50000); globalThis.count = r.length; } 0", collect: true },
];

async function runBench() {
  setHosts(Math.max(1, hosts.length));
  const h = hosts[0];
  const out = [];
  const body = $("bench-body");
  body.innerHTML = "";
  for (const c of CASES) {
    statusEl.textContent = "Snapshot: " + c.name + "…";
    const r = await h.call({ type: "bench", setup: c.setup, collect: !!c.collect, runs: 5 });
    const g = await gzip(r.text);
    const row = { id: c.id, name: c.name, text: bytes(r.text), gz: g.size, gzMs: g.ms, take: median(r.takeMs), restore: median(r.restoreMs) };
    out.push(row);
    const tr = document.createElement("tr");
    tr.innerHTML = `<td>${c.name}</td><td>${kb(row.text)}</td><td>${kb(row.gz)}</td><td>${ms(row.take)}</td><td>${ms(row.restore)}</td><td>${ms(row.gzMs)}</td>`;
    body.appendChild(tr);
  }
  statusEl.textContent = "";
  return out;
}

// ---- 2. one query, three ways ---------------------------------------------------

function settings() {
  return {
    shards: Math.max(2, Math.min(8, +$("shards").value || 4)),
    rows: Math.max(1000, Math.min(200000, +$("rows").value || 50000)),
    key: $("key").value,
    rtt: Math.max(0, +$("rtt").value || 0),
    mbit: Math.max(1, +$("mbit").value || 100),
    collect: $("collect").checked,
  };
}
// one transfer one way: half a round trip and the gzip'd bytes on the wire
const wire = (gz, s) => s.rtt / 2 + (gz * 8) / (s.mbit * 1000);

async function runQuery(opts) {
  const s = { ...settings(), ...(opts || {}) };
  setHosts(s.shards);
  statusEl.textContent = "Loading shards…";
  await Promise.all(hosts.map((h, k) => h.call({ type: "init", k, n: s.rows })));

  // A: the whole engine
  statusEl.textContent = "A: moving the engine…";
  const A = { hops: [], text: 0, gz: 0, work: 0, net: 0 };
  let t0 = now();
  let snap = "";
  for (let k = 0; k < s.shards; k++) {
    const r = await hosts[k].call({ type: "hopA", snapshot: snap, key: s.key, collect: s.collect });
    snap = r.snapshot;
    A.hops.push(r);
    A.summary = r.summary;
  }
  A.wall = now() - t0;
  for (const r of A.hops) {
    const b = bytes(r.snapshot);
    const g = (await gzip(r.snapshot)).size;
    r.text = b;
    r.gz = g;
    A.text += b;
    A.gz += g;
    A.work += r.restoreMs + r.queryMs + r.takeMs;
    A.net += wire(g, s);
    delete r.snapshot;
  }

  // B: only the data root, into engines made from a template each host has
  statusEl.textContent = "B: installing the template…";
  const tpl = (await hosts[0].call({ type: "bench", setup: SHARD + AGENT, runs: 1 })).text;
  const tplGz = (await gzip(tpl)).size;
  await Promise.all(hosts.map((h) => h.call({ type: "template", text: tpl })));
  statusEl.textContent = "B: moving the data root…";
  const B = { hops: [], text: 0, gz: 0, work: 0, net: 0, template: bytes(tpl), templateGz: tplGz };
  t0 = now();
  let carry = "null";
  for (let k = 0; k < s.shards; k++) {
    const r = await hosts[k].call({ type: "hopB", carry, key: s.key });
    carry = r.carry;
    B.hops.push(r);
    B.summary = r.summary;
  }
  B.wall = now() - t0;
  for (const r of B.hops) {
    r.text = bytes(r.carry);
    r.gz = (await gzip(r.carry)).size;
    B.text += r.text;
    B.gz += r.gz;
    B.work += r.makeMs + r.ioMs + r.queryMs;
    B.net += wire(r.gz, s);
    delete r.carry;
  }

  // C: map-reduce
  statusEl.textContent = "C: map-reduce…";
  const C = { text: 0, gz: 0 };
  t0 = now();
  const parts = await Promise.all(hosts.map((h) => h.call({ type: "mapC", code: AGENT, key: s.key })));
  const tm = now();
  C.summary = await summaryOf(parts);
  C.mergeMs = now() - tm;
  C.wall = now() - t0;
  const codeGz = (await gzip(AGENT)).size;
  let slowest = 0;
  C.work = 0;
  for (const p of parts) {
    const g = (await gzip(p.part)).size;
    C.text += bytes(p.part) + bytes(AGENT);
    C.gz += g + codeGz;
    C.work = Math.max(C.work, p.queryMs);
    slowest = Math.max(slowest, s.rtt + ((g + codeGz) * 8) / (s.mbit * 1000));
  }
  C.net = slowest;
  C.work += C.mergeMs;

  statusEl.textContent = "";
  const res = { settings: s, A, B, C };
  showQuery(res);
  return res;
}

// the merged answer, by the same code in a fresh engine on the first host
async function summaryOf(parts) {
  const setup = AGENT + parts.map((p) => `merge(${p.part});`).join("\n") + " 0";
  const r = await hosts[0].call({ type: "summary", setup });
  return r.summary;
}

function showQuery({ settings: s, A, B, C }) {
  const rows = [
    ["A · whole engine moves", A, `${s.shards} hops, sequential`],
    ["B · data root moves", B, `${s.shards} hops, sequential; template ${kb(B.templateGz)} gzip once per host`],
    ["C · map-reduce", C, `${s.shards} shards in parallel`],
  ];
  $("query-body").innerHTML = rows
    .map(([name, r, note]) => `<tr><td>${name}<div class="note">${note}</div></td><td>${kb(r.text)}</td><td>${kb(r.gz)}</td><td>${ms(r.work)}</td><td>${ms(r.wall)}</td><td>${ms(r.net)}</td><td><b>${ms(r.work + r.net)}</b></td></tr>`)
    .join("");
  const same = A.summary === B.summary && B.summary === C.summary;
  $("query-check").textContent = same ? `All three agree: ${A.summary}.` : `The answers differ: A ${A.summary}; B ${B.summary}; C ${C.summary}.`;
  $("query-check").className = "note " + (same ? "ok" : "bad");
  $("hops").textContent = [
    ...A.hops.map((r, i) => `A hop ${i}: restore ${ms(r.restoreMs)}, query ${ms(r.queryMs)}, take ${ms(r.takeMs)}, ${kb(r.text)} (${kb(r.gz)} gzip)`),
    ...B.hops.map((r, i) => `B hop ${i}: engine from template ${ms(r.makeMs)}, JSON in+out ${ms(r.ioMs)}, query ${ms(r.queryMs)}, ${kb(r.text)} (${kb(r.gz)} gzip)`),
    `C: slowest shard ${ms(C.work - C.mergeMs)}, merge ${ms(C.mergeMs)}`,
  ].join("\n");
}

// ---- page ---------------------------------------------------------------------

async function guarded(fn) {
  for (const b of document.querySelectorAll("button")) b.disabled = true;
  try {
    return await fn();
  } catch (e) {
    statusEl.textContent = "Failed: " + (e.message || e);
    throw e;
  } finally {
    for (const b of document.querySelectorAll("button")) b.disabled = false;
  }
}
$("run-bench").onclick = () => guarded(runBench);
for (const id of ["rtt", "mbit"]) $(id).oninput = () => window.travelDemo.last && showQuery(rewire(window.travelDemo.last));

// the same run under another network: only the wire estimates change
function rewire(res) {
  const s = { ...res.settings, rtt: Math.max(0, +$("rtt").value || 0), mbit: Math.max(1, +$("mbit").value || 100) };
  const A = { ...res.A, net: res.A.hops.reduce((t, r) => t + wire(r.gz, s), 0) };
  const B = { ...res.B, net: res.B.hops.reduce((t, r) => t + wire(r.gz, s), 0) };
  const C = { ...res.C, net: s.rtt + (res.C.gz / s.shards) * 8 / (s.mbit * 1000) };
  return { settings: s, A, B, C };
}

window.travelDemo = {
  runBench: () => guarded(runBench),
  runQuery: async (o) => (window.travelDemo.last = await guarded(() => runQuery(o))),
  last: null,
};
$("run-query").onclick = () => window.travelDemo.runQuery();
