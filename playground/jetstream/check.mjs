#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Runs the JetStream 3 benchmarks of a built playground (dist/jetstream,
// dist/wasm) outside the browser, the way the page runs them: each
// benchmark's script (src/jetstream-core.js) in a fresh realm of an engine, in a worker thread that is ended past the time limit.
//
//   node playground/jetstream/check.mjs [--engine=node,cerxes-wasm,cerxes-jit]
//        [--only=richards,splay] [--cap=5] [--limit=120] [--json=out.json]
//
// node: Node's own engine (node:vm, a new context per benchmark); the
// CErXes engines: playground/src/engines/cerxes-*.js on dist/wasm.

import fs from "node:fs";
import path from "node:path";
import { Worker, isMainThread, parentPort, workerData } from "node:worker_threads";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DIST = path.join(HERE, "../dist");
const core = await import(pathToFileURL(path.join(HERE, "../src/jetstream-core.js")).href);

async function child() {
  const { engine, bench, cap } = workerData;
  const dir = path.join(DIST, "jetstream");
  const load = async (f, binary) => (binary ? new Uint8Array(fs.readFileSync(path.join(dir, f))) : fs.readFileSync(path.join(dir, f), "utf8"));
  let inst;
  if (engine === "node") {
    const vm = await import("node:vm");
    let out = [];
    const log = (...a) => out.push(a.map(String).join(" "));
    const ctx = vm.createContext({ print: log, console: { log, info: log, warn: log, error: log, debug: log }, performance, setTimeout, clearTimeout, TextEncoder, TextDecoder });
    inst = {
      run(src) {
        out = [];
        try {
          vm.runInContext(src, ctx);
          return { out, error: false };
        } catch (e) {
          out.push("Uncaught " + (e && e.stack ? e.stack : e));
          return { out, error: true };
        }
      },
      async settle() {
        const o = out;
        for (let i = 0; i < 200 && !core.parse(o); i++) await new Promise((r) => setTimeout(r, 0));
      },
    };
  } else {
    const real = globalThis.fetch;
    globalThis.fetch = async (u) => (String(u).startsWith("file:") ? new Response(fs.readFileSync(fileURLToPath(u))) : real(u));
    const mod = await import(pathToFileURL(path.join(HERE, "../src/engines", engine + ".js")).href);
    inst = await mod.default.load(pathToFileURL(DIST + "/").href);
  }
  const t0 = performance.now();
  const parts = bench.subs || [bench];
  const res = [];
  for (const b of parts) {
    if (inst.reset) inst.reset();
    const sc = await core.script(b, cap, load);
    const r = inst.run(sc.src);
    if (sc.async && inst.settle) await inst.settle();
    let got = core.parse(r.out);
    const last = r.out;
    if (!got && r.error) got = { error: r.out.slice(-1)[0] };
    if (!got) got = { error: "no result: " + last.slice(-3).join(" | ") };
    res.push({ name: b.name, ...got });
    if (got.error) break;
  }
  parentPort.postMessage({ res, ms: performance.now() - t0 });
}

async function main() {
  const opt = (k, d) => (process.argv.find((a) => a.startsWith("--" + k + "=")) || "").slice(k.length + 3) || d;
  const engines = opt("engine", "node,cerxes-wasm").split(",");
  const only = opt("only", "") ? new Set(opt("only").split(",")) : null;
  const cap = Number(opt("cap", "5"));
  const limit = Number(opt("limit", "120")) * 1000;
  const m = JSON.parse(fs.readFileSync(path.join(DIST, "jetstream/manifest.json"), "utf8"));
  const all = [];
  for (const b of m.benchmarks) {
    if (b.subs) for (const s of b.subs) all.push({ ...s, group: b.name });
    else all.push(b);
  }
  const list = all.filter((b) => !only || only.has(b.name) || only.has(b.group));
  const results = {};
  for (const b of list) {
    const row = [b.name.padEnd(30)];
    for (const engine of engines) {
      const r = await new Promise((resolve) => {
        const w = new Worker(fileURLToPath(import.meta.url), { workerData: { engine, bench: b, cap }, resourceLimits: { maxOldGenerationSizeMb: 4096 } });
        const timer = setTimeout(() => {
          w.terminate();
          resolve({ error: `timeout ${limit / 1000} s` });
        }, limit);
        w.on("message", (x) => {
          clearTimeout(timer);
          w.terminate();
          resolve(x);
        });
        w.on("error", (e) => {
          clearTimeout(timer);
          resolve({ error: String(e && e.message) });
        });
      });
      let cell;
      if (r.error) cell = "ERR " + r.error;
      else {
        const bad = r.res.find((x) => x.error);
        if (bad) cell = "ERR " + bad.error;
        else {
          const subs = r.res.length > 1 ? core.groupScores(r.res.map((x) => core.subScores(b.subs.find((s) => s.name === x.name), x.times, cap))) : core.subScores(b, r.res[0].times, cap);
          const sc = core.scoreOf(subs);
          const dig = r.res.map((x) => x.digest || "-").join(",");
          cell = `${sc.toFixed(2)} (${(r.ms / 1000).toFixed(1)} s) #${dig}`;
          (results[b.name] ||= {})[engine] = sc;
          (results[b.name].digest ||= {})[engine] = dig;
        }
      }
      row.push(engine + " " + cell.replace(/\s+/g, " ").slice(0, 160));
    }
    process.stdout.write(row.join(" | ") + "\n");
  }
  if (opt("json", "")) fs.writeFileSync(opt("json"), JSON.stringify(results, null, 1));
}

if (isMainThread) await main();
else await child();
