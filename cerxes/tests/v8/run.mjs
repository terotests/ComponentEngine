// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Known V8 bug classes, run against CErXes (README.md).
//
//   node cerxes/tests/v8/run.mjs own [--configs=regs,stack,tree,snap,wjit] [--filter=re] [-j 4]
//   node cerxes/tests/v8/run.mjs mjsunit --v8=/path/to/v8 [--dirs=regress,.] [--configs=regs] [-j 4]
//
// `own`: every T(...) / A(...) / I(...) line of own/*.js becomes a script
// of its own (the rest of its file is the setup), run in Node (the
// reference) and in CErXes on each configuration. A line that Node prints
// and CErXes does not is a difference; a panic, abort, stack overflow,
// out-of-memory or hang is a crash class. known.json holds the verdicts of
// today, so a run says what got better or worse.
//
// `mjsunit`: V8's own tests (test/mjsunit/regress/*.js and test/mjsunit/*.js
// of a V8 checkout) with mjsunit.js's assertions in front and V8's %Natives
// turned into no-op calls; a test counts when Node passes it.
//
// Results: cerxes/tests/v8/out/<suite>.jsonl and <suite>-report.md.
import fs from "node:fs";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CE = path.resolve(HERE, "../..");
const OUT = path.join(HERE, "out");
const BIN = process.env.CERXES || path.join(CE, "target/release/cerxes");
const SNAP = path.join(CE, "fuzz/target/release/snaprun");
const args = process.argv.slice(2);
const suite = args[0];
const opt = (n, d) => args.find((a) => a.startsWith("--" + n + "="))?.slice(n.length + 3) ?? d;
const J = Number(args[args.indexOf("-j") + 1] || 0) || Math.max(1, (await import("node:os")).cpus().length - 1);
const TIMEOUT = Number(opt("timeout", "20")) * 1000;
const MEM_KB = Number(opt("mem-mb", "3000")) * 1024;

if (suite !== "own" && suite !== "mjsunit") {
  console.error("usage: run.mjs own|mjsunit … (see the head of this file)");
  process.exit(2);
}

// ---------------------------------------------------------------- configurations
const LIM = `ulimit -v ${MEM_KB}; ulimit -c 0; exec `;
const CONF = {
  // Node, in script mode (vm.runInThisContext), gc exposed
  node: (f) => ["node", ["--expose-gc", "--unhandled-rejections=warn", "--stack-size=900", "-e",
    "require('vm').runInThisContext(require('fs').readFileSync(process.argv[1],'utf8'),{filename:process.argv[1]})", f], {}],
  regs: (f) => ["bash", ["-c", LIM + `"${BIN}" "$0"`, f], {}],
  stack: (f) => ["bash", ["-c", LIM + `"${BIN}" "$0"`, f], { CER_REGS: "0" }],
  tree: (f) => ["bash", ["-c", LIM + `"${BIN}" "$0"`, f], { CER_PCOMP: "0" }],
  slice: (f) => ["bash", ["-c", LIM + `"${BIN}" "$0"`, f], { CERXES_SLICE_MS: "0.05" }],
  // 0.05 ms slices, a snapshot and a restore into a fresh engine at each one
  snap: (f) => ["bash", ["-c", LIM + `"${SNAP}" "$0" 0.05`, f], {}],
  // WebAssembly build with the baseline JIT, every loop compiled at once
  wjit: (f) => ["node", [path.join(CE, "bench/wjrun.mjs"), "--eval=" + f], {}],
};
const configs = opt("configs", suite === "own" ? "regs,stack,tree,snap,wjit" : "regs").split(",");

function build() {
  const need = new Set(configs);
  if (!fs.existsSync(BIN) || need.has("regs")) run1("cargo", ["build", "--release", "--bin", "cerxes"], CE);
  if (need.has("snap")) run1("cargo", ["build", "--release", "--bin", "snaprun"], path.join(CE, "fuzz"));
}
function run1(cmd, a, cwd) {
  const r = spawnSync(cmd, a, { cwd, stdio: ["ignore", "ignore", "inherit"], env: { ...process.env, RUSTFLAGS: "-Awarnings" } });
  if (r.status !== 0) process.exit(1);
}

// ---------------------------------------------------------------- classes
export function classify(r) {
  const err = (r.err || "") + (r.out || "");
  if (r.timedOut) return "hang";
  if (/memory allocation of \d+ bytes failed|capacity overflow/.test(err) || r.sig === "SIGKILL") return "oom";
  if (/panicked at/.test(err)) return "panic";
  if (/has overflowed its stack/.test(err)) return "stack-overflow";
  if (/HOSTERR|RuntimeError: (unreachable|memory access out of bounds)/.test(err)) return "wasm-trap";
  if (r.sig) return "crash:" + r.sig;
  return r.code === 0 ? "pass" : "fail";
}
const CRASH = (c) => !["pass", "fail"].includes(c);
const BAD = (v) => v !== "same" && v !== "differs"; // a verdict that is a crash class

function runOne(dir, f, c) {
  return new Promise((res) => {
    const [cmd, a, env] = CONF[c](path.join(dir, f));
    const t0 = Date.now();
    const p = spawn(cmd, a, { env: { ...process.env, RUST_BACKTRACE: "0", ...env } });
    let out = "", err = "", timedOut = false;
    p.stdout.on("data", (d) => { if (out.length < 20000) out += d; });
    p.stderr.on("data", (d) => { if (err.length < 20000) err += d; });
    const t = setTimeout(() => { timedOut = true; p.kill("SIGKILL"); }, TIMEOUT);
    p.on("close", (code, sig) => {
      clearTimeout(t);
      const r = { f, c, code, sig, timedOut, ms: Date.now() - t0, out: out.slice(-3000), err: err.slice(-1500) };
      r.cls = classify(r);
      res(r);
    });
  });
}

async function runAll(dir, files, cfgs, sink) {
  const jobs = [];
  for (const f of files) for (const c of cfgs) jobs.push([f, c]);
  let i = 0, done = 0;
  const worker = async () => {
    while (i < jobs.length) {
      const [f, c] = jobs[i++];
      sink(await runOne(dir, f, c));
      if (++done % 250 === 0) console.error(`${done}/${jobs.length}`);
    }
  };
  await Promise.all(Array.from({ length: J }, worker));
}

// ---------------------------------------------------------------- own
function splitOwn(dst) {
  fs.rmSync(dst, { recursive: true, force: true });
  fs.mkdirSync(dst, { recursive: true });
  const re = opt("filter", "") ? new RegExp(opt("filter", "")) : null;
  const isT = (l) => /^(T|A|I)\("/.test(l);
  const files = [];
  for (const f of fs.readdirSync(path.join(HERE, "own")).filter((x) => x.endsWith(".js")).sort()) {
    const lines = fs.readFileSync(path.join(HERE, "own", f), "utf8").split("\n");
    lines.forEach((l, i) => {
      if (!isT(l)) return;
      const id = f.slice(0, 2) + "-" + l.match(/^\w\("([^"]+)"/)[1].replace(/[^\w.-]/g, "_");
      if (re && !re.test(id)) return;
      fs.writeFileSync(path.join(dst, id + ".js"), lines.filter((m, j) => j === i || !isT(m)).join("\n"));
      files.push(id + ".js");
    });
  }
  return files;
}

const norm = (s) => (s || "").trim().split("\n").filter((x) => x && !/^(slices|snapshots):/.test(x)).join("\n");

function verdict(ref, r) {
  if (CRASH(r.cls)) return r.cls;
  if (ref && norm(r.out) === norm(ref.out)) return "same";
  return "differs";
}

function panicAt(r) {
  const m = (r.err || "").match(/panicked at (?:.*\/)?(src\/[^\n]*?):\s*\n([^\n]*)/);
  return m ? `${m[1]} ${m[2].slice(0, 90)}` : "";
}

async function own() {
  build();
  const dir = path.join(OUT, "own");
  const files = splitOwn(dir);
  const res = {};
  fs.mkdirSync(OUT, { recursive: true });
  const jl = fs.createWriteStream(path.join(OUT, "own.jsonl"));
  await runAll(dir, files, ["node", ...configs], (r) => {
    (res[r.f] ??= {})[r.c] = r;
    jl.write(JSON.stringify(r) + "\n");
  });
  jl.end();
  const known = fs.existsSync(path.join(HERE, "known.json")) ? JSON.parse(fs.readFileSync(path.join(HERE, "known.json"), "utf8")) : {};
  const now = {};
  const md = ["# CErXes vs V8 bug classes (own tests)", "", `${files.length} tests, configurations ${configs.join(", ")}; reference Node ${process.version}.`, ""];
  const counts = {};
  const changes = [];
  const rows = [];
  for (const f of files.sort()) {
    const id = f.replace(/\.js$/, "");
    const ref = res[f].node;
    const v = {};
    for (const c of configs) v[c] = verdict(ref, res[f][c]);
    for (const c of configs) counts[v[c]] = (counts[v[c]] || 0) + 1;
    const worst = Object.values(v).find(BAD) || (Object.values(v).includes("differs") ? "differs" : "same");
    now[id] = worst === "same" ? "same" : Object.entries(v).filter(([, x]) => x !== "same").map(([c, x]) => c + ":" + x).join(" ");
    if ((known[id] ?? "same") !== now[id]) changes.push(`- ${id}: ${known[id] ?? "same"} → ${now[id]}`);
    if (worst !== "same") {
      const c0 = configs.find((c) => v[c] !== "same");
      const r = res[f][c0];
      const p = panicAt(r);
      rows.push(`| ${id} | ${now[id]} | ${JSON.stringify(norm(ref.out).replace(/^[^:]*: /, "")).slice(0, 60)} | ${(p || (CRASH(r.cls) ? r.cls : JSON.stringify(norm(r.out).replace(/^[^:]*: /, "")))).slice(0, 80).replace(/\|/g, "\\|")} |`);
    }
  }
  md.push("Verdicts (test × configuration): " + Object.entries(counts).map(([k, n]) => `${k} ${n}`).join(", "), "");
  md.push("## Changes against known.json", "", ...(changes.length ? changes : ["(none)"]), "");
  md.push("## Not the same as Node", "", "| test | configurations | Node | CErXes (first differing configuration) |", "|---|---|---|---|", ...rows, "");
  fs.writeFileSync(path.join(OUT, "own-report.md"), md.join("\n"));
  // with --filter, the tests left out keep their known verdicts
  const all = { ...known, ...now };
  const sorted = {};
  for (const k of Object.keys(all).sort()) sorted[k] = all[k];
  fs.writeFileSync(path.join(OUT, "own-known.json"), JSON.stringify(sorted, null, 1) + "\n");
  console.log(md.slice(0, 6).join("\n"));
  console.log(`report: ${path.join(OUT, "own-report.md")}`);
  console.log(`(cp ${path.join(OUT, "own-known.json")} ${path.join(HERE, "known.json")} to accept today's verdicts)`);
  // a crash class that known.json did not have fails the run
  const worse = changes.filter((l) => / → .*(panic|stack-overflow|oom|wasm-trap|crash)/.test(l) && !/ → same$/.test(l));
  process.exit(worse.length ? 1 : 0);
}

// ---------------------------------------------------------------- mjsunit
const SKIP = /WebAssembly|wasm|\bd8\.|Realm\.|new Worker|^\s*import\s|^\s*export\s|\bload\(|readbuffer|os\.system|externalize|--throws|--bundle|async_hooks|--expose-fast-api|--expose-statistics|--expose-cputracemark|%Debug|setTimeout\(|testRunner|performance\.|\bquit\(|Atomics\.wait|--harmony-struct|Shared(Struct|Array)\b|--stack-size|--no-lazy-feedback|--js-staging|--experimental/m;
const natives = (s) => s.replace(/(^|[^\w$)\]\s])(\s*)%([A-Z_]\w*)\s*\(/gm, "$1$2__nat(").replace(/(^|[\s(,;=!])%([A-Z_]\w*)\s*\(/gm, "$1__nat(");
const HEAD = `var __nat = function(){ return undefined; };
var print = function(){ console.log(Array.prototype.join.call(arguments, " ")); };
var printErr = print;
if (typeof gc !== "function") { globalThis.gc = function(){}; }
`;

async function mjsunit() {
  const v8 = opt("v8", "");
  if (!v8 || !fs.existsSync(path.join(v8, "test/mjsunit/mjsunit.js"))) {
    console.error("--v8=<a V8 checkout> (git clone --depth 1 --filter=blob:none --sparse https://github.com/v8/v8; git sparse-checkout set test/mjsunit/regress)");
    process.exit(2);
  }
  build();
  const mj = natives(fs.readFileSync(path.join(v8, "test/mjsunit/mjsunit.js"), "utf8"));
  const dir = path.join(OUT, "mjsunit");
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  const files = [];
  let skipped = 0;
  for (const d of opt("dirs", "regress,.").split(",")) {
    const src = path.join(v8, "test/mjsunit", d);
    for (const f of fs.readdirSync(src).filter((x) => x.endsWith(".js") && x !== "mjsunit.js").sort()) {
      const s = fs.readFileSync(path.join(src, f), "utf8");
      if (SKIP.test(s)) { skipped++; continue; }
      const name = (d === "." ? "" : d.replace(/\W/g, "_") + "__") + f;
      fs.writeFileSync(path.join(dir, name), HEAD + mj + "\n;\n" + natives(s) + "\n");
      files.push(name);
    }
  }
  console.error(`${files.length} tests, ${skipped} skipped (d8-only, wasm, modules, timers)`);
  const res = {};
  const jl = fs.createWriteStream(path.join(OUT, "mjsunit.jsonl"));
  await runAll(dir, files, ["node", ...configs], (r) => {
    (res[r.f] ??= {})[r.c] = r;
    jl.write(JSON.stringify(r) + "\n");
  });
  jl.end();
  const groups = {};
  let counted = 0;
  for (const f of files) {
    const x = res[f];
    for (const c of configs) {
      const k = x[c].cls;
      if (CRASH(k)) {
        const key = `${c} ${k}${k === "panic" ? " " + panicAt(x[c]) : ""}`;
        (groups[key] ??= []).push(f + (x.node.cls === "pass" ? "" : " (Node: " + x.node.cls + ")"));
      } else if (x.node.cls === "pass") {
        if (c === configs[0]) counted++;
        if (k === "fail") (groups[`${c} fail`] ??= []).push(f);
      }
    }
  }
  const md = ["# V8 mjsunit tests on CErXes", "", `${files.length} tests (${skipped} skipped), Node passes ${files.filter((f) => res[f].node.cls === "pass").length}.`, ""];
  for (const [k, v] of Object.entries(groups).sort((a, b) => b[1].length - a[1].length)) {
    md.push(`## ${k} (${v.length})`, "", v.join(", "), "");
  }
  fs.writeFileSync(path.join(OUT, "mjsunit-report.md"), md.join("\n"));
  console.log(md.slice(0, 3).join("\n"));
  for (const [k, v] of Object.entries(groups)) console.log(`${k}: ${v.length}`);
  console.log(`report: ${path.join(OUT, "mjsunit-report.md")}`);
}

await (suite === "own" ? own() : mjsunit());
