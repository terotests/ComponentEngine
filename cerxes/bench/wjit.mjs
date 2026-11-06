// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The baseline JIT (cer/src/wjit.rs) in a WebAssembly host: CErXes built
// for wasm32-wasip1 with the `wjit` feature (playground/cerxes-wasm), its
// compiled loops made into WebAssembly modules that this host (Node's V8)
// compiles and puts into CErXes' function table.
//
//   node cerxes/bench/wjit.mjs [--build] [--cases] [--bench] [--min=N]
//
// --cases runs every case of cer/tests/cases.txt and cerxes/tests/cases.txt
// with the JIT off and on (min 0: every loop compiled at its first back
// edge) and fails on a different answer. --bench times the loop workloads
// below and cerxes/bench/cpu/*.js, off and on (the best of --runs). With
// neither, both.
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { makeWasi } from "../../playground/src/wasi.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "../..");
const CRATE = path.join(REPO, "playground/cerxes-wasm");
const TARGET = path.join(CRATE, "target-jit");
const WASM = path.join(TARGET, "wasm32-wasip1/release/cerxes_wasm.wasm");
const args = process.argv.slice(2);
const opt = (name, def) => args.find((a) => a.startsWith("--" + name + "="))?.slice(name.length + 3) ?? def;
const doCases = args.includes("--cases") || !args.includes("--bench");
const doBench = args.includes("--bench") || !args.includes("--cases");
const runs = Number(opt("runs", "3"));
const min = Number(opt("min", "64"));

if (args.includes("--build") || !fs.existsSync(WASM)) {
  console.log("==> cargo build --target wasm32-wasip1 --features wjit");
  const r = spawnSync("cargo", ["build", "--release", "--target", "wasm32-wasip1", "--features", "wjit"], {
    cwd: CRATE,
    stdio: "inherit",
    env: {
      ...process.env,
      CARGO_TARGET_DIR: TARGET,
      RUSTFLAGS: "-Awarnings -C link-arg=--export-table -C link-arg=--growable-table",
    },
  });
  if (r.status !== 0) process.exit(1);
}

const wasmModule = new WebAssembly.Module(fs.readFileSync(WASM));
const enc = new TextEncoder();
const dec = new TextDecoder();

/** A fresh CErXes instance with the JIT host. */
function engine() {
  const out = [];
  const wasi = makeWasi((_fd, line) => out.push(line));
  let x = null;
  const jit = {
    modules: 0,
    compileMs: 0,
    install(ptr, len) {
      const t0 = performance.now();
      const bytes = new Uint8Array(x.memory.buffer, ptr, len).slice();
      let m;
      try {
        m = new WebAssembly.Module(bytes);
      } catch (e) {
        fs.writeFileSync("/tmp/wjit-bad.wasm", bytes);
        throw new Error("wjit: invalid module (/tmp/wjit-bad.wasm): " + e.message);
      }
      if (!jit.helpers) {
        // from the function table: an export of a wasip1 module runs the
        // constructors and destructors around every call
        const t = x.__indirect_function_table;
        jit.helpers = ["get_elem", "set_elem", "get_prop", "set_prop", "op", "call", "slow", "ret"].map((h, k) => t.get(x.cerxes_jit_helper(k)));
      }
      const env = { memory: x.memory, table: x.__indirect_function_table };
      ["get_elem", "set_elem", "get_prop", "set_prop", "op", "call", "slow", "ret"].forEach((h, k) => (env[h] = jit.helpers[k]));
      const inst = new WebAssembly.Instance(m, { env });
      const t = x.__indirect_function_table;
      const idx = t.grow(1);
      t.set(idx, inst.exports.f);
      jit.modules++;
      jit.compileMs += performance.now() - t0;
      return idx;
    },
  };
  const inst = new WebAssembly.Instance(wasmModule, { ...wasi.imports, cerxes_jit: { install: (p, n) => jit.install(p, n) } });
  x = inst.exports;
  wasi.setMemory(x.memory);
  if (x._initialize) x._initialize();
  const e = x.cx_new();
  const text = (p, n) => dec.decode(new Uint8Array(x.memory.buffer, p, n));
  return {
    jit,
    out,
    setJit(on, m) {
      const mask = opt("mask", "").split(",").filter(Boolean).map(Number);
      let lo = 0, hi = 0;
      for (const k of mask) if (k < 32) lo |= 1 << k; else hi |= 1 << (k - 32);
      x.cerxes_jit_mask(lo, hi);
      return x.cx_jit(e, on ? 1 : 0, m);
    },
    eval(src) {
      const b = enc.encode(src);
      const p = x.cx_alloc(b.length);
      new Uint8Array(x.memory.buffer, p, b.length).set(b);
      const failed = x.cx_eval(e, p, b.length);
      x.cx_free(p, b.length);
      const value = text(x.cx_result_ptr(), x.cx_result_len());
      const o = text(x.cx_output_ptr(), x.cx_output_len());
      return { failed, value, output: o };
    },
    stats() {
      x.cx_jit_stats();
      return JSON.parse(text(x.cx_result_ptr(), x.cx_result_len()));
    },
  };
}

function readCases(file) {
  const lines = fs.readFileSync(file, "utf8").split("\n");
  const cases = [];
  let cur = null;
  let part = 0;
  for (const l of lines) {
    if (l.startsWith("==== ")) {
      if (cur) cases.push(cur);
      cur = { title: l.slice(5), src: [], want: [] };
      part = 0;
    } else if (cur && l === "----" && part === 0) {
      part = 1;
    } else if (cur) {
      (part === 0 ? cur.src : cur.want).push(l);
    }
  }
  if (cur) cases.push(cur);
  return cases
    .filter((c) => c.title.startsWith("js "))
    .map((c) => ({ title: c.title, src: c.src.join("\n"), want: c.want.join("\n").replace(/\n+$/, "") }));
}

let failures = 0;

if (args.includes("--octane")) {
  // the Octane suites (cer/bench/octane.mjs --dump, which needs a Ranger
  // checkout: RANGER_DIR or ../Ranger), JIT off and on: scores, and a
  // failure when a suite reports an error with the JIT on
  const dir = fs.mkdtempSync("/tmp/wjit-octane-");
  const d = spawnSync("node", [path.join(REPO, "cer/bench/octane.mjs"), "--dump=" + dir], { stdio: "inherit" });
  if (d.status !== 0) process.exit(1);
  const only = opt("only", "");
  console.log("suite\toff\ton\tspeedup");
  for (const f of fs.readdirSync(dir).filter((f) => f.endsWith(".js")).sort()) {
    if (only && !only.split(",").some((o) => f.includes(o))) continue;
    const src = fs.readFileSync(path.join(dir, f), "utf8");
    // off, on, off, on...: the best score of each (V8 tiers CErXes' own
    // code up while it runs, which a single first run would charge to it)
    const best = { false: new Map(), true: new Map() };
    const rx = /^([A-Za-z]+): (.*)$/gm;
    for (let i = 0; i < runs; i++) {
      for (const on of [false, true]) {
        const e = engine();
        e.setJit(on, min);
        const r = e.eval(src);
        for (const [, k, v] of (r.output + "\n" + e.out.join("\n")).matchAll(rx)) {
          const old = best[on].get(k);
          if (old === undefined || !/^[0-9.]+$/.test(v) || (/^[0-9.]+$/.test(old) && Number(v) > Number(old))) best[on].set(k, v);
        }
      }
    }
    const offs = [...best[false].entries()].map(([k, v]) => [null, k, v]);
    const ons = best[true];
    for (const [, k, v] of offs) {
      const w = ons.get(k) ?? "missing";
      const ok = /^[0-9.]+$/.test(w);
      if (!ok && /^[0-9.]+$/.test(v)) failures++;
      console.log(`${k}\t${v}\t${w}\t${ok ? (Number(w) / Number(v)).toFixed(2) + "x" : "FAIL"}`);
    }
  }
  process.exit(failures ? 1 : 0);
}

if (opt("eval", "")) {
  // --eval=file: its answer with the JIT off and on (min 0)
  const src = fs.readFileSync(opt("eval", ""), "utf8");
  for (const on of opt("only-on", "") ? [true] : opt("only-off", "") ? [false] : [false, true]) {
    const e = engine();
    e.setJit(on, on ? Number(opt("min", "0")) : 0);
    const r = e.eval(src);
    console.log((on ? "on:  " : "off: ") + r.value + (r.output ? "\n" + r.output : ""));
    if (on) console.log(JSON.stringify(e.stats()));
    if (on && e.out.length) console.log(e.out.join("\n"));
  }
  process.exit(0);
}

if (doCases) {
  const files = [path.join(REPO, "cer/tests/cases.txt"), path.join(REPO, "cerxes/tests/cases.txt")];
  let n = 0;
  let same = 0;
  let compiled = 0;
  for (const f of files) {
    for (const c of readCases(f)) {
      if (opt("case", "") && !c.title.includes(opt("case", ""))) continue;
      n++;
      const a = engine();
      a.setJit(false, 0);
      const ra = a.eval(c.src);
      const b = engine();
      if (!b.setJit(true, 0)) {
        console.log("FAIL this build cannot run compiled code");
        process.exit(1);
      }
      let rb;
      try {
        rb = b.eval(c.src);
      } catch (e) {
        rb = { failed: 2, value: String(e), output: "" };
      }
      compiled += b.stats().compiled;
      if (ra.value === rb.value && ra.output === rb.output && ra.failed === rb.failed) {
        same++;
      } else {
        failures++;
        console.log(`FAIL ${path.basename(path.dirname(f))}: ${c.title}\n  off: ${ra.value}\n  on:  ${rb.value}`);
      }
    }
  }
  console.log(`cases: ${same}/${n} the same with the JIT on (${compiled} functions compiled)`);
}

const LOOPS = {
  "sum-loop": `function f() { let s = 0; for (let i = 0; i < 20000000; i++) { s += i * 0.5; } return s } f()`,
  "nested-int": `function f() { let c = 0; for (let i = 0; i < 3000; i++) for (let j = 0; j < 3000; j++) c = (c + (i ^ j)) & 1048575; return c } f()`,
  "while-collatz": `function f() { let steps = 0; for (let n = 1; n < 300000; n++) { let x = n; while (x !== 1) { x = (x % 2 === 0) ? x / 2 : 3 * x + 1; steps++; } } return steps } f()`,
  "fn-loop": `function f(n) { let a = 0, b = 1; for (let i = 0; i < n; i++) { const t = a + b; a = b; b = t % 1000007; } return b; } let r = 0; for (let k = 0; k < 200; k++) r += f(50000); r`,
  "array-sum": `function f() { const a = []; for (let i = 0; i < 1000000; i++) a.push(i); let s = 0; for (let r = 0; r < 10; r++) for (let i = 0; i < a.length; i++) s += a[i]; return s } f()`,
  "top-level-sum": `let s = 0; for (let i = 0; i < 5000000; i++) { s += i * 0.5; } s`,
};

function best(fn) {
  let b = Infinity;
  let v;
  for (let i = 0; i < runs; i++) {
    const t0 = performance.now();
    v = fn();
    b = Math.min(b, performance.now() - t0);
  }
  return [b, v];
}

if (doBench) {
  const rows = [];
  const cpu = path.join(REPO, "cerxes/bench/cpu");
  const work = Object.entries(LOOPS).map(([k, v]) => [k, v]);
  for (const f of fs.readdirSync(cpu).filter((f) => f.endsWith(".js")).sort()) work.push(["cpu/" + f.slice(0, -3), fs.readFileSync(path.join(cpu, f), "utf8")]);
  const only = opt("only", "");
  for (const [name, src] of work) {
    if (only && !only.split(",").some((o) => name.includes(o))) continue;
    const res = {};
    for (const on of [false, true]) {
      let stats = null;
      let jit = null;
      const [ms, r] = best(() => {
        const e = engine();
        e.setJit(on, min);
        const r = e.eval(src);
        stats = e.stats();
        jit = e.jit;
        return r;
      });
      res[on ? "on" : "off"] = { ms, r, stats, jit };
    }
    const ok = res.off.r.value === res.on.r.value && res.off.r.failed === res.on.r.failed;
    if (!ok) failures++;
    const s = res.on.stats;
    rows.push([
      name + (args.includes("--exits") ? " refused " + res.on.stats.refused : ""),
      res.off.ms.toFixed(0),
      res.on.ms.toFixed(0),
      (res.off.ms / res.on.ms).toFixed(2) + "x",
      `${s.compiled} fn (${s.dropped} dropped), ${s.bytes} B, ${s.opsDone}/${s.opsAll} ops, ${s.entries} entries, ${res.on.jit.compileMs.toFixed(1)} ms` +
        (args.includes("--exits") ? " exits " + s.exits.map((n, k) => (n ? k + ":" + n : "")).filter(Boolean).join(" ") : ""),
      ok ? "" : "DIFFERENT: " + res.off.r.value.slice(0, 60) + " vs " + res.on.r.value.slice(0, 60),
    ]);
  }
  console.log("\nworkload\toff ms\ton ms\tspeedup\tcompiled (stats are for the last run)");
  for (const r of rows) console.log(r.join("\t"));
}

process.exit(failures ? 1 : 0);
