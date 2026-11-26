#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Prepares the playground's "JetStream 3" tab: reads WebKit/JetStream's
// benchmark list (JetStreamDriver.js, the BENCHMARKS array and the groups
// pushed after it) at a pinned revision, keeps the benchmarks JetStream runs
// by default that are JavaScript only, and writes them with the files they
// load into <out>:
//
//   <out>/manifest.json   every default benchmark: kind, iterations, worst
//                         case count, files, preloads; those the playground
//                         cannot run are listed with the reason
//   <out>/workloads/...   the files of the runnable ones (".z" inflated)
//   <out>/LICENSE*        JetStream's licence and each workload's own
//
// JetStream is not kept in this repository (~64 MB of workloads): without
// --src the pinned revision is fetched into playground/jetstream/JetStream
// (git ignored) as a sparse checkout of only the files needed.
//
//   node playground/jetstream/update.mjs --out=playground/dist/jetstream [--src=../JetStream]

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import zlib from "node:zlib";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const REPO = "https://github.com/WebKit/JetStream.git";
// JetStream 3.0 (main, 2026-10-29)
export const REV = "5fb9b4915f47792e6368d54d79ba3ba85e5600cc";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const opt = (k) => (process.argv.find((a) => a.startsWith("--" + k + "=")) || "").slice(k.length + 3);

function git(dir, ...argv) {
  const r = spawnSync("git", ["-C", dir, ...argv], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (r.status !== 0) throw new Error(`git ${argv.join(" ")}: ${(r.stderr || "").trim()}`);
  return r.stdout.trim();
}

/** The pinned revision as a sparse checkout holding `files` (and the
 * driver). Fetches only those blobs. */
function checkout(dir, files) {
  if (!fs.existsSync(path.join(dir, ".git"))) {
    fs.mkdirSync(dir, { recursive: true });
    git(dir, "init", "-q");
    git(dir, "remote", "add", "origin", REPO);
    git(dir, "config", "core.sparseCheckout", "true");
  }
  const have = spawnSync("git", ["-C", dir, "rev-parse", "HEAD"], { encoding: "utf8" }).stdout.trim();
  if (have !== REV) {
    git(dir, "fetch", "-q", "--depth", "1", "--filter=blob:none", "origin", REV);
  }
  const want = ["/JetStreamDriver.js", "/LICENSE", ...files.map((f) => "/" + f)];
  fs.writeFileSync(path.join(dir, ".git/info/sparse-checkout"), want.join("\n") + "\n");
  git(dir, "checkout", "-q", "--force", REV);
  git(dir, "read-tree", "-mu", "HEAD");
}

const mk = (kind) =>
  class {
    constructor(plan, subs) {
      this.kind = kind;
      Object.assign(this, plan);
      if (subs) this.subs = subs;
    }
  };

/** BENCHMARKS as plain records, by running that part of the driver with
 * the benchmark classes replaced by recorders. */
export function readBenchmarks(driverSrc) {
  const a = driverSrc.indexOf("function dotnetPreloads");
  const b = driverSrc.indexOf("const benchmarksByName");
  if (a < 0 || b < 0) throw new Error("JetStreamDriver.js: BENCHMARKS not found");
  const num = (name) => Number((new RegExp(`const ${name} = (\\d+);`).exec(driverSrc) || [])[1]);
  const ctx = { defaultIterationCount: num("defaultIterationCount"), defaultWorstCaseCount: num("defaultWorstCaseCount") };
  for (const k of ["DefaultBenchmark", "AsyncBenchmark", "WasmEMCCBenchmark", "WSLBenchmark", "AsyncWasmLegacyBenchmark", "GroupedBenchmark"]) ctx[k] = mk(k);
  vm.createContext(ctx);
  vm.runInContext(driverSrc.slice(a, b) + "\nthis.__list = BENCHMARKS;", ctx);
  return { list: ctx.__list, defaultIterationCount: ctx.defaultIterationCount, defaultWorstCaseCount: ctx.defaultWorstCaseCount };
}

const tagsOf = (b) => (b.tags || []).map((t) => t.toLowerCase());
const BINARY = /\.(wasm|glb|bin|png|jpg|ttf|onnx|raw|dat)$/i;
const served = (f) => f.replace(/^\.\//, "").replace(/\.z$/, "");

/** Why the playground cannot run a default benchmark, or null. */
function skipReason(b) {
  const t = tagsOf(b);
  if (t.includes("wasm") && !t.includes("js")) return "WebAssembly workload (CErXes and the other interpreters here have no WebAssembly API)";
  if (t.includes("workertests")) return "needs Web Workers (JetStream itself leaves it out in shells)";
  if (!t.includes("js")) return "not a JavaScript benchmark";
  return null;
}

function record(b, dic, dwc) {
  const iterations = b.iterations || dic;
  const r = {
    name: b.name,
    kind: b.kind.replace("Benchmark", "") || "Default",
    tags: tagsOf(b).filter((t) => t !== "default"),
    iterations,
    worstCaseCount: b.worstCaseCount !== undefined ? b.worstCaseCount : dwc,
    files: (b.files || []).map(served),
    preload: Object.fromEntries(Object.entries(b.preload || {}).map(([k, v]) => [k, served(v)])),
  };
  if (b.deterministicRandom) r.deterministicRandom = true;
  if (b.exposeBrowserTest) r.exposeBrowserTest = true;
  if (b.args && Object.keys(b.args).length) r.args = b.args;
  const bin = Object.values(r.preload).filter((f) => BINARY.test(f));
  if (bin.length) r.binary = bin;
  return r;
}

function main() {
  const out = path.resolve(opt("out") || path.join(HERE, "../dist/jetstream"));
  let src = opt("src") ? path.resolve(opt("src")) : path.join(HERE, "JetStream");
  const fetchIt = !opt("src");
  if (fetchIt) checkout(src, []);
  const driver = fs.readFileSync(path.join(src, "JetStreamDriver.js"), "utf8");
  const { list, defaultIterationCount: dic, defaultWorstCaseCount: dwc } = readBenchmarks(driver);

  const benchmarks = [];
  const skipped = [];
  const need = new Set();
  for (const b of list) {
    const subs = b.subs || null;
    const isDefault = tagsOf(b).includes("default");
    if (!isDefault) continue;
    const why = skipReason(b);
    if (why) {
      skipped.push({ name: b.name, tags: tagsOf(b).filter((t) => t !== "default"), reason: why });
      continue;
    }
    const r = subs ? { name: b.name, kind: "Grouped", tags: tagsOf(b).filter((t) => t !== "default"), subs: subs.map((s) => record(s, dic, dwc)) } : record(b, dic, dwc);
    for (const s of r.subs || [r]) {
      for (const f of s.files) need.add(f);
      for (const f of Object.values(s.preload)) need.add(f);
    }
    // the raw names (with .z) for the checkout
    for (const s of subs || [b]) {
      for (const f of s.files || []) need.add("raw:" + f.replace(/^\.\//, ""));
      for (const f of Object.values(s.preload || {})) need.add("raw:" + f.replace(/^\.\//, ""));
    }
    benchmarks.push(r);
  }
  const raw = [...need].filter((f) => f.startsWith("raw:")).map((f) => f.slice(4));
  // each workload directory's licence files
  const dirs = new Set(raw.filter((f) => f.startsWith("workloads/")).map((f) => f.split("/").slice(0, 2).join("/")));
  const licences = [];
  if (fetchIt) checkout(src, [...raw, ...[...dirs].map((d) => d + "/LICENSE*"), ...[...dirs].map((d) => d + "/*/LICENSE*")]);

  fs.rmSync(out, { recursive: true, force: true });
  fs.mkdirSync(out, { recursive: true });
  let bytes = 0;
  for (const f of raw) {
    let data = fs.readFileSync(path.join(src, f));
    if (f.endsWith(".z")) data = zlib.inflateSync(data);
    const dst = path.join(out, served(f));
    fs.mkdirSync(path.dirname(dst), { recursive: true });
    fs.writeFileSync(dst, data);
    bytes += data.length;
  }
  fs.copyFileSync(path.join(src, "LICENSE"), path.join(out, "LICENSE"));
  for (const d of dirs) {
    const walk = (dir, depth) => {
      if (!fs.existsSync(dir)) return;
      for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
        const p = path.join(dir, e.name);
        if (e.isFile() && /^(LICEN[CS]E|COPYING)/i.test(e.name)) {
          const rel = path.relative(src, p);
          const dst = path.join(out, rel);
          fs.mkdirSync(path.dirname(dst), { recursive: true });
          fs.copyFileSync(p, dst);
          licences.push(rel);
        } else if (e.isDirectory() && depth < 1) walk(p, depth + 1);
      }
    };
    walk(path.join(src, d), 0);
  }
  const manifest = {
    source: `${REPO.replace(/\.git$/, "")}/tree/${REV}`,
    rev: REV,
    date: git(src, "log", "-1", "--format=%cs"),
    defaultIterationCount: dic,
    defaultWorstCaseCount: dwc,
    benchmarks,
    skipped,
    licences,
  };
  fs.writeFileSync(path.join(out, "manifest.json"), JSON.stringify(manifest, null, 1));
  process.stdout.write(`jetstream: ${benchmarks.length} benchmarks (${skipped.length} skipped), ${raw.length} files, ${(bytes / 1048576).toFixed(1)} MB\n`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
