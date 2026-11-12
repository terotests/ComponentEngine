// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Open-source JavaScript libraries on CErXes against V8: does each library
// give the same results, how fast, how much memory.
//
//   node cerxes/bench/libs.mjs [--cases=lodash,zod] [--engines=node,cerxes]
//        [--budget=400] [--target=esnext] [--timeout=900] [--no-lower]
//
// Each case (bench/libs/cases/<name>.js) imports its library from npm
// (bench/libs/package.json, versions pinned; `npm ci` there first) and runs
// checks (values compared engine against engine) and benches (timed calls).
// esbuild bundles case + runtime.js into one script with shim.js as its
// banner, written to bench/libs/out/<name>.js; every engine runs that file.
// When CErXes stops on a SyntaxError, the case is bundled again lowered to
// ES2017 and run once more (--no-lower: not), so the rest is still measured.
// Engines (--engines, default node,cerxes):
//   node     the Node running this, a fresh vm realm (node-run.cjs)
//   jitless  the same with --jitless (V8's interpreter, Ignition)
//   cerxes   cerxes/target/release/cerxes with CER_REGS=1 (register VM)
//   stack    the same with CER_REGS=0 (the stack VM)
//   base     $CERXES_BASE (another cerxes build, for an A/B), CER_REGS=1
// Each run reports wall and CPU time and its peak RSS (wait4). Results:
// bench/libs/out/results.json and results.md.
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DIR = path.join(HERE, "libs");
const OUT = path.join(DIR, "out");
const CERXES = process.env.CERXES || path.resolve(HERE, "../target/release/cerxes");
const args = process.argv.slice(2);
const opt = (name, def) => args.find((a) => a.startsWith("--" + name + "="))?.slice(name.length + 3) ?? def;
const flag = (name) => args.includes("--" + name);
const allCases = fs.readdirSync(path.join(DIR, "cases")).filter((f) => f.endsWith(".js")).map((f) => f.slice(0, -3)).sort();
const cases = opt("cases", "") ? opt("cases").split(",") : allCases;
const engines = opt("engines", "node,cerxes").split(",");
const budget = Number(opt("budget", "400"));
const target = opt("target", "esnext");
const timeout = Number(opt("timeout", "900"));

if (!fs.existsSync(path.join(DIR, "node_modules"))) {
  console.error("bench/libs/node_modules is missing: run `npm ci` in cerxes/bench/libs");
  process.exit(1);
}
const esbuild = (await import(path.join(DIR, "node_modules/esbuild/lib/main.js"))).default;
fs.mkdirSync(OUT, { recursive: true });

async function bundle(name, tgt) {
  const file = path.join(OUT, name + (tgt === "esnext" ? "" : "." + tgt) + ".js");
  const entry = `import { run } from ${JSON.stringify(path.join(DIR, "runtime.js"))};\nimport c from ${JSON.stringify(path.join(DIR, "cases", name + ".js"))};\nglobalThis.__BUDGET = ${budget};\nrun(c);\n`;
  await esbuild.build({
    stdin: { contents: entry, resolveDir: DIR, sourcefile: name + ".entry.js" },
    bundle: true,
    format: "iife",
    platform: "browser",
    target: tgt,
    mainFields: ["browser", "module", "main"],
    define: { "process.env.NODE_ENV": '"production"' },
    banner: { js: fs.readFileSync(path.join(DIR, "shim.js"), "utf8") },
    legalComments: "none",
    logLevel: "error",
    outfile: file,
  });
  return file;
}

// the run, with its rusage from wait4 (python: Node's spawn has no rusage)
const WAIT4 = `
import os, sys, json, time, subprocess
t = time.time()
p = subprocess.Popen(sys.argv[2:], stdout=open(sys.argv[1] + ".out", "w"), stderr=open(sys.argv[1] + ".err", "w"))
_, st, ru = os.wait4(p.pid, 0)
print(json.dumps({"status": os.waitstatus_to_exitcode(st), "wall": (time.time() - t) * 1000, "cpu": (ru.ru_utime + ru.ru_stime) * 1000, "rss": ru.ru_maxrss / 1024}))
`;

function runOne(engine, file) {
  const cmd = {
    node: [process.execPath, path.join(DIR, "node-run.cjs"), file],
    jitless: [process.execPath, "--jitless", path.join(DIR, "node-run.cjs"), file],
    cerxes: [CERXES, file],
    stack: [CERXES, file],
    base: [process.env.CERXES_BASE || CERXES, file],
  }[engine];
  const env = { ...process.env, TZ: "UTC", LC_ALL: "C", LANG: "C", CER_REGS: engine === "stack" ? "0" : "1" };
  const base = path.join(OUT, path.basename(file, ".js") + "." + engine);
  return new Promise((resolve) => {
    const p = spawn("timeout", [String(timeout), "python3", "-I", "-c", WAIT4, base, ...cmd], { env });
    let so = "";
    p.stdout.on("data", (d) => (so += d));
    p.on("close", (code) => {
      let ru = { status: code, wall: 0, cpu: 0, rss: 0 };
      try { ru = JSON.parse(so.trim().split("\n").pop()); } catch {}
      if (code === 124) ru.status = "timeout";
      const stdout = fs.existsSync(base + ".out") ? fs.readFileSync(base + ".out", "utf8") : "";
      const stderr = fs.existsSync(base + ".err") ? fs.readFileSync(base + ".err", "utf8") : "";
      resolve({ ...ru, ...parse(stdout), stderr: stderr.trim().split("\n").slice(-3).join(" | ").slice(0, 400) });
    });
  });
}

function parse(text) {
  const r = { load: null, checks: {}, bench: {}, done: null, other: [] };
  for (const line of text.split("\n")) {
    const f = line.split("\t");
    if (f[0] === "L" && f.length === 2) r.load = +f[1];
    else if (f[0] === "C" && f.length >= 3) r.checks[f[1]] = f.slice(2).join("\t");
    else if (f[0] === "B" && f.length >= 4) r.bench[f[1]] = f[2] === "ERR" ? { err: f[3] } : { ms: +f[2], n: +f[3] };
    else if (f[0] === "D" && f.length === 2) r.done = +f[1];
    else if (line.trim()) r.other.push(line.slice(0, 200));
  }
  r.other = r.other.slice(0, 5);
  return r;
}

/** Same up to the last digits of floats (libm results may differ by an ulp). */
function nearlySame(a, b) {
  const num = /-?\d+(?:\.\d+)?(?:e[+-]?\d+)?/gi;
  if (a.replace(num, "#") !== b.replace(num, "#")) return false;
  const x = a.match(num) || [], y = b.match(num) || [];
  return x.length === y.length && x.every((s, i) => s === y[i] || Math.abs(+s - +y[i]) <= 1e-9 * Math.max(1, Math.abs(+s)));
}

function compare(ref, r) {
  const diffs = [];
  let near = 0;
  for (const k of new Set([...Object.keys(ref.checks), ...Object.keys(r.checks)])) {
    const a = ref.checks[k], b = r.checks[k];
    if (a === b) continue;
    if (a !== undefined && b !== undefined && nearlySame(a, b)) { near++; continue; }
    diffs.push({ check: k, v8: a?.slice(0, 300), got: b?.slice(0, 300) });
  }
  return { diffs, near };
}

const geo = (xs) => (xs.length ? Math.exp(xs.reduce((s, x) => s + Math.log(x), 0) / xs.length) : null);
const results = [];
for (const name of cases) {
  let file;
  try { file = await bundle(name, target); } catch (e) { console.log(`${name.padEnd(14)} BUNDLE FAILED ${String(e.message).split("\n")[0]}`); continue; }
  const row = { name, size: fs.statSync(file).size, runs: {} };
  for (const e of engines) {
    let r = await runOne(e, file);
    if (e !== "node" && e !== "jitless" && r.done === null && /SyntaxError/.test(r.stderr + r.other.join(" ")) && !flag("no-lower") && target === "esnext") {
      r.lowered = { stderr: r.stderr };
      const low = await bundle(name, "es2017");
      const r2 = await runOne(e, low);
      r = { ...r2, lowered: { syntax: r.stderr } };
    }
    row.runs[e] = r;
  }
  const ref = row.runs[engines[0]];
  for (const e of engines.slice(1)) {
    const r = row.runs[e];
    const { diffs, near } = compare(ref, r);
    const errs = Object.entries(r.checks).filter(([, v]) => v.startsWith("ERR ")).length;
    r.status = r.done === null ? (r.status === "timeout" ? "TIMEOUT" : "CRASH") : diffs.length ? "DIFF" : "PASS";
    r.diffs = diffs;
    r.near = near;
    r.errs = errs;
    const ratios = Object.keys(ref.bench).filter((k) => ref.bench[k].ms && r.bench[k]?.ms).map((k) => r.bench[k].ms / ref.bench[k].ms);
    r.ratio = geo(ratios);
  }
  results.push(row);
  const s = engines.slice(1).map((e) => {
    const r = row.runs[e];
    return `${e}: ${r.status}${r.lowered ? " (lowered)" : ""}${r.diffs.length ? " " + r.diffs.length + " diff" : ""} ${r.ratio ? r.ratio.toFixed(1) + "x" : "-"} ${r.rss.toFixed(0)}MB`;
  });
  console.log(`${name.padEnd(14)} V8 ${ref.done !== null ? "ok" : "FAIL"} ${ref.rss.toFixed(0)}MB | ${s.join(" | ")}`);
  for (const e of engines.slice(1)) {
    const r = row.runs[e];
    if (r.status === "CRASH" || r.status === "TIMEOUT") console.log(`    ${e}: ${r.stderr || r.other.join(" ")}`);
    for (const d of r.diffs.slice(0, 4)) console.log(`    ${e} ${d.check}: V8 ${d.v8} | got ${d.got}`);
  }
}
const resFile = path.join(OUT, opt("out", "results") + ".json");
fs.writeFileSync(resFile, JSON.stringify({ date: new Date().toISOString(), engines, budget, target, results }, null, 1));

// results.md: one row per library
const md = [`| library | ${engines.slice(1).map((e) => `${e} status | ${e}/V8 time | ${e} load ms | ${e} RSS MB`).join(" | ")} | V8 load ms | V8 RSS MB |`];
md.push("|" + " --- |".repeat(1 + engines.slice(1).length * 4 + 2));
for (const row of results) {
  const ref = row.runs[engines[0]];
  const cells = engines.slice(1).map((e) => {
    const r = row.runs[e];
    return `${r.status}${r.lowered ? " (ES2017)" : ""}${r.diffs.length ? ` ${r.diffs.length}/${Object.keys(ref.checks).length}` : ""} | ${r.ratio ? r.ratio.toFixed(1) + "×" : "–"} | ${r.load ?? "–"} | ${r.rss.toFixed(0)}`;
  });
  md.push(`| ${row.name} | ${cells.join(" | ")} | ${ref.load ?? "–"} | ${ref.rss.toFixed(0)} |`);
}
fs.writeFileSync(path.join(OUT, opt("out", "results") + ".md"), md.join("\n") + "\n");
console.log("wrote " + resFile);
