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
const WASM = process.env.WJIT_WASM || path.join(TARGET, "wasm32-wasip1/release/cerxes_wasm.wasm");
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
    evicted: 0,
    // an evicted module's slots get the runtime's `gone` (back to the
    // interpreter at once), so that nothing holds the module any more
    release(base, n) {
      const t = x.__indirect_function_table;
      for (let k = 0; k < n; k++) t.set(base + k, jit.rt.gone);
      jit.evicted += n;
    },
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
      // the runtime module (look, codes, hstore, callx): kept for the
      // compiled functions' modules, which import it as "rt"
      const inst = new WebAssembly.Instance(m, jit.rt ? { env, rt: jit.rt } : { env });
      if (inst.exports.look) {
        jit.rt = inst.exports;
        jit.compileMs += performance.now() - t0;
        return 0;
      }
      // a batch: f0, f1, … into consecutive slots; the first one's index
      const t = x.__indirect_function_table;
      let n = 0;
      while (inst.exports["f" + n]) n++;
      const idx = t.grow(n);
      for (let k = 0; k < n; k++) t.set(idx + k, inst.exports["f" + k]);
      jit.modules++;
      jit.compileMs += performance.now() - t0;
      return idx;
    },
  };
  const inst = new WebAssembly.Instance(wasmModule, { ...wasi.imports, cerxes_jit: { install: (p, n) => jit.install(p, n), release: (b, n) => jit.release(b, n) } });
  x = inst.exports;
  wasi.setMemory(x.memory);
  if (x._initialize) x._initialize();
  const e = x.cx_new();
  const text = (p, n) => dec.decode(new Uint8Array(x.memory.buffer, p, n));
  return {
    jit,
    out,
    setJit(on, m, extra = []) {
      const mask = opt("mask", "").split(",").filter(Boolean).map(Number).concat(extra);
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
  const dir = process.env.OCTANE_DIR || fs.mkdtempSync("/tmp/wjit-octane-");
  if (!process.env.OCTANE_DIR) { const d = spawnSync("node", [path.join(REPO, "cer/bench/octane.mjs"), "--dump=" + dir], { stdio: "inherit" });
  if (d.status !== 0) process.exit(1); }
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

// wjrun: --eval=file with the JIT on (min 0, every loop compiled at once);
// prints the console lines, exit 1 when the script threw
{
  const src = fs.readFileSync(opt("eval", ""), "utf8");
  const e = engine();
  e.setJit(opt("off", "") ? false : true, Number(opt("min", "0")), []);
  let r;
  try { r = e.eval(src); } catch (err) { console.log("HOSTERR " + err.message); process.exit(4); }
  if (r.output) process.stdout.write(r.output.endsWith("\n") ? r.output : r.output + "\n");
  if (e.out.length) console.log(e.out.join("\n"));
  if (r.failed) { console.error("Uncaught " + r.value); process.exit(1); }
  process.exit(0);
}
