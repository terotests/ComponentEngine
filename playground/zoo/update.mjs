#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Refreshes the zoo.js.org material the playground's "Zoo ranking" tab runs
// and ranks against, from two checkouts:
//
//   ivankra/javascript-zoo        the Octane suites (bench/*.js) and the
//                                 conformance tests (conformance/es1, es3,
//                                 es5, compat-table/es6, es2016…es2025)
//   ivankra/javascript-zoo-data   (branch data) the published results:
//                                 bench/amd64/*.json, es1-5/*.json,
//                                 compat-table/*.json
//
//   git clone --depth 1 https://github.com/ivankra/javascript-zoo zoo
//   git clone --depth 1 -b data https://github.com/ivankra/javascript-zoo-data zoo-data
//   node playground/zoo/update.mjs --zoo=zoo --data=zoo-data
//
// Writes playground/zoo/{octane/*.js, conformance.json, reference.json}.

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const opt = (k) => (process.argv.find((a) => a.startsWith("--" + k + "=")) || "").slice(k.length + 3);
const ZOO = path.resolve(opt("zoo") || "../zoo");
const DATA = path.resolve(opt("data") || "../zoo-data");
const rev = (dir) => spawnSync("git", ["-C", dir, "log", "-1", "--format=%H %cs"], { encoding: "utf8" }).stdout.trim().split(" ");

export const SUITES = ["richards", "deltablue", "crypto", "raytrace", "earley-boyer", "regexp", "splay", "navier-stokes"];
// zoo.js.org's Score: the geometric mean of these (the "v8" columns).
const SCORES = ["Richards", "DeltaBlue", "Crypto", "RayTrace", "EarleyBoyer", "RegExp", "Splay", "NavierStokes"];
const DIRS = [
  "es1", "es3", "es5", "compat-table/es6",
  ...["2016", "2017", "2018", "2019", "2020", "2021", "2022", "2023", "2024", "2025"].map((y) => "compat-table/es" + y),
];

// ---- Octane ---------------------------------------------------------------

fs.mkdirSync(path.join(HERE, "octane"), { recursive: true });
for (const s of SUITES) fs.copyFileSync(path.join(ZOO, "bench", s + ".js"), path.join(HERE, "octane", s + ".js"));

// ---- conformance ----------------------------------------------------------
// Weights as harness/run.py computes them: a compat-table test in a group
// "… (tiny|small|medium|large) > subtest" weighs 1|2|4|8 over the group's
// size, any other compat-table test 1; es1-es5 tests are unweighted.

const CONF = path.join(ZOO, "conformance");
const SIZE = { tiny: 1, small: 2, medium: 4, large: 8 };
const header = (code) => {
  for (const line of code.split("\n")) {
    const m = /^\/\/ compat-table: (.*)/.exec(line);
    if (m) return m[1];
    if (!line.startsWith("//") && !line.startsWith("#!")) return null;
  }
  return null;
};
const tests = [];
for (const dir of DIRS) {
  for (const f of fs.readdirSync(path.join(CONF, dir)).filter((f) => f.endsWith(".js")).sort()) {
    tests.push({ p: dir + "/" + f, c: fs.readFileSync(path.join(CONF, dir, f), "utf8") });
  }
}
const groups = {};
for (const t of tests) {
  if (!t.p.startsWith("compat-table/")) continue;
  const h = header(t.c);
  const m = h && /^(.*) \((tiny|small|medium|large)\) > .*/.exec(h);
  t.group = m ? m[1] : null;
  t.size = m ? SIZE[m[2]] : 1;
  if (m) groups[m[1]] = (groups[m[1]] || 0) + 1;
}
for (const t of tests) {
  if (t.p.startsWith("compat-table/")) t.w = t.group ? t.size / groups[t.group] : 1;
  delete t.group;
  delete t.size;
}
const [zooRev, zooDate] = rev(ZOO);
fs.writeFileSync(
  path.join(HERE, "conformance.json"),
  JSON.stringify({ source: "https://github.com/ivankra/javascript-zoo/tree/" + zooRev + "/conformance", date: zooDate, tests }) + "\n",
);

// ---- the published results ------------------------------------------------
// Base builds only (zoo.js.org's default view hides variants); conformance
// from the build the zoo's own pages prefer (_exp, _full, _262, then plain).

const readme = (id) => {
  const f = path.join(ZOO, "engines", id, "README.md");
  if (!fs.existsSync(f)) return {};
  const text = fs.readFileSync(f, "utf8");
  const title = (/^# (.*)$/m.exec(text) || [])[1];
  const jit = (/^\* JIT:\s+(.*)$/m.exec(text) || [])[1];
  const language = (/^\* Language:\s+(.*)$/m.exec(text) || [])[1];
  const clean = (s) => s && s.replace(/\[([^\]]*)\]\([^)]*\)/g, "$1").replace(/<[^>]+>/g, "").trim();
  return { title: clean(title), jit: clean(jit), language: clean(language) };
};
const median = (xs) => [...xs].sort((a, b) => a - b)[xs.length >> 1];
const load = (suite, id) => {
  for (const c of [id + "_exp", id + "_full", id + "_262", id]) {
    const f = path.join(DATA, suite, c + ".json");
    if (fs.existsSync(f)) return JSON.parse(fs.readFileSync(f, "utf8"));
  }
  return null;
};
const round = (x) => (x == null ? null : Math.round(x * 10) / 10);

const engines = [];
for (const f of fs.readdirSync(path.join(DATA, "bench/amd64")).sort()) {
  const b = JSON.parse(fs.readFileSync(path.join(DATA, "bench/amd64", f), "utf8"));
  const id = b.binary && b.binary.engine;
  if (!id || b.binary.binary_name !== id) continue;
  const octane = {};
  for (const k of SCORES) {
    const e = b.benchmarks && b.benchmarks[k];
    if (e && e.score && e.score.length) octane[k] = median(e.score);
  }
  const es15 = load("es1-5", id);
  const ct = load("compat-table", id);
  const s = (r, k, field) => (r && r.summary && r.summary[k] ? r.summary[k][field] : null);
  engines.push({
    id,
    ...readme(id),
    revision_date: b.binary.revision_date,
    octane,
    es15: round(s(es15, "es1-5", "pass_percent")),
    es6: round(s(ct, "es6", "weighted_pass_percent")),
    es2016: round(s(ct, "es2016+", "weighted_pass_percent")),
  });
}
const [dataRev, dataDate] = rev(DATA);
fs.writeFileSync(
  path.join(HERE, "reference.json"),
  JSON.stringify({ source: "https://github.com/ivankra/javascript-zoo-data/tree/" + dataRev, date: dataDate, arch: "amd64", engines }, null, 0).replace(/\},\{"id"/g, '},\n{"id"') + "\n",
);
console.log(`${tests.length} conformance tests, ${engines.length} engines, octane: ${SUITES.length} suites`);
