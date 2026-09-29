// SPDX-License-Identifier: AGPL-3.0-or-later
//
// javascript-zoo's conformance tests (playground/zoo/conformance.json)
// through CEr built by cargo, scored as zoo.js.org scores them: ES1-5 pass
// rate, compat-table's weighted ES6 and ES2016+ rates. A test passes when it
// prints "<file>: OK".
//
//   node cer/bench/zoo.mjs                 # the scores
//   node cer/bench/zoo.mjs --failing       # and every failing test with its last line
//   node cer/bench/zoo.mjs --only=es6/Map  # the tests whose path contains this
//   node cer/bench/zoo.mjs --save=f.json   # the failing set, to --diff=f.json later
//   node cer/bench/zoo.mjs --js            # CEr compiled to JavaScript by rgrc instead
import fs from "fs";
import os from "os";
import path from "path";
import { spawn } from "child_process";
import { CER, REPO, build } from "./common.mjs";

const args = process.argv.slice(2);
const opt = (k) => (args.find((a) => a.startsWith("--" + k + "=")) || "").slice(k.length + 3);
const conf = JSON.parse(fs.readFileSync(path.join(REPO, "playground/zoo/conformance.json"), "utf8"));
const GROUPS = [
  ["ES1-5", (p) => /^es[135]\//.test(p), false],
  ["ES6", (p) => p.startsWith("compat-table/es6/"), true],
  ["ES2016+", (p) => /^compat-table\/es20\d\d\//.test(p), true],
];
const only = opt("only");
const tests = conf.tests.filter((t) => GROUPS.some((g) => g[1](t.p)) && (!only || t.p.includes(only)));

const JS = args.includes("--js");
build(JS ? ["cer-js"] : ["cer-rust"]);
const CMD = JS ? [process.execPath, "--max-old-space-size=4096", path.join(CER, "bench/cer_js_runner.cjs")] : [path.join(CER, "target/release/cer")];
const TMP = fs.mkdtempSync(path.join(os.tmpdir(), "cer-zoo-"));

function runOne(t) {
  return new Promise((resolve) => {
    const f = path.join(TMP, t.p.replace(/\//g, "__"));
    fs.writeFileSync(f, t.c);
    const p = spawn(CMD[0], CMD.slice(1).concat([f]));
    let out = "";
    p.stdout.on("data", (d) => (out += d));
    p.stderr.on("data", (d) => (out += d));
    const timer = setTimeout(() => p.kill("SIGKILL"), JS ? 30000 : 10000);
    p.on("close", (code, sig) => {
      clearTimeout(timer);
      const name = path.basename(t.p);
      const ok = out.includes(name + ": OK") && !out.includes(name + ": failed") && !out.includes(name + ": exception");
      const last = sig ? "killed (" + sig + ")" : out.trim().split("\n").filter(Boolean).slice(-1)[0] || "no output";
      resolve({ t, ok, last });
    });
  });
}

const results = [];
let next = 0;
await Promise.all(
  Array.from({ length: os.cpus().length }, async () => {
    while (next < tests.length) results.push(await runOne(tests[next++]));
  }),
);
fs.rmSync(TMP, { recursive: true, force: true });

for (const [name, test, weighted] of GROUPS) {
  const rs = results.filter((x) => test(x.t.p));
  if (!rs.length) continue;
  const p = rs.filter((x) => x.ok).length;
  const w = rs.reduce((a, x) => a + (x.t.w || 0), 0);
  const wp = rs.filter((x) => x.ok).reduce((a, x) => a + (x.t.w || 0), 0);
  const pct = weighted ? (100 * wp) / w : (100 * p) / rs.length;
  console.log(`${name.padEnd(8)} ${pct.toFixed(1).padStart(5)}%  (${p}/${rs.length})`);
}
const failing = results.filter((x) => !x.ok).sort((a, b) => (a.t.p < b.t.p ? -1 : 1));
if (args.includes("--failing")) for (const x of failing) console.log(`  ${x.t.p}  ${x.last.slice(0, 160)}`);
if (opt("save")) fs.writeFileSync(opt("save"), JSON.stringify(failing.map((x) => x.t.p)));
if (opt("diff")) {
  const before = new Set(JSON.parse(fs.readFileSync(opt("diff"), "utf8")));
  const now = new Set(failing.map((x) => x.t.p));
  for (const p of before) if (!now.has(p)) console.log("  fixed  " + p);
  for (const p of now) if (!before.has(p)) console.log("  BROKE  " + p);
}
