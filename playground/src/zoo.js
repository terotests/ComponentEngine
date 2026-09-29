// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The "Zoo ranking" tab: zoo.js.org's two measurements, run in this browser
// on every ticked engine, and ranked among the engines zoo.js.org publishes.
//
//   conformance  ivankra/javascript-zoo's es1, es3, es5 tests (plain pass
//                rate) and its compat-table ports for ES6 and ES2016+
//                (compat-table's weighted pass rate), each test in a fresh
//                realm; a test passes when it prints "<file>: OK".
//   speed        the eight Octane v9 suites of zoo.js.org's Score, each in a
//                fresh realm, prepared as cer/bench/octane.mjs prepares them
//                (print provided, Measure timed with performance.now).
//
// zoo.js.org measures native builds on an amd64 server; this page measures
// WebAssembly builds in a tab. "Calibrated" multiplies each suite's score by
// zoo's V8 score over this browser's own score for that suite, so the
// browser's JIT stands in for zoo's V8 and the other engines are placed as if
// they had run on zoo's machine.

const $ = (id) => document.getElementById(id);

export const SCORES = ["Richards", "DeltaBlue", "Crypto", "RayTrace", "EarleyBoyer", "RegExp", "Splay", "NavierStokes"];
const SUITE_FILE = {
  Richards: "richards", DeltaBlue: "deltablue", Crypto: "crypto", RayTrace: "raytrace",
  EarleyBoyer: "earley-boyer", RegExp: "regexp", Splay: "splay", NavierStokes: "navier-stokes",
};
const GROUPS = [
  { key: "es15", label: "ES1–5", test: (p) => /^es[135]\//.test(p), weighted: false },
  { key: "es6", label: "ES6", test: (p) => p.startsWith("compat-table/es6/"), weighted: true },
  { key: "es2016", label: "ES2016+", test: (p) => /^compat-table\/es20\d\d\//.test(p), weighted: true },
];
const DIRS = (p) => p.slice(0, p.lastIndexOf("/"));

export function prepareOctane(src) {
  let s = src.replace(/if \(typeof print == "undefined" && typeof console != "undefined"\) \{[\s\S]*?\n\}\n/, "/* print provided */\n");
  s = s.replace(
    /Object\.defineProperty\(Object\.prototype,\s*["']inheritsFrom["']\s*,\s*\{[\s\S]*?\}\);/,
    `Function.prototype.inheritsFrom = function (shuper) {
  function Inheriter() { }
  Inheriter.prototype = shuper.prototype;
  this.prototype = new Inheriter();
  this.superConstructor = shuper;
};`,
  );
  s = s.replace(/function Measure\(data\) \{\s*var elapsed = 0;\s*var start = new Date\(\);\s*/m, `function Measure(data) {
    var elapsed = 0;
    var start = performance.now();
  `);
  s = s.replace(/elapsed = new Date\(\) - start;/g, "elapsed = performance.now() - start;");
  return s;
}

export function parseOctane(lines) {
  const r = { scores: {}, error: null };
  for (const l of lines) {
    const t = l.trim();
    const m = /^([A-Za-z]+): ([0-9.e+-]+)$/.exec(t);
    if (m) r.scores[m[1]] = Number(m[2]);
    else if (/^[A-Za-z]+: /.test(t) && /error|wrong|alert|undefined|fail/i.test(t)) r.error = t;
  }
  return r;
}

/** The zoo's verdict: "<file>: OK" printed and no "<file>: failed|exception". */
export function verdict(path, lines) {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const text = lines.join("\n");
  if (text.includes(name + ": failed") || text.includes(name + ": exception")) return false;
  return text.includes(name + ": OK");
}

const geo = (xs) => (xs.length ? Math.exp(xs.reduce((a, b) => a + Math.log(b), 0) / xs.length) : null);
const fmtScore = (v) => (v == null ? "" : v >= 100 ? String(Math.round(v)) : v >= 10 ? v.toFixed(1) : v.toPrecision(2));
const fmtPct = (v) => (v == null ? "" : v >= 99.95 || v === 0 ? v.toFixed(0) + "%" : v.toFixed(1) + "%");
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

/**
 * ctx: { chosen() -> engines, runnerFor(e), store, rev, setEngineState }
 */
export function setupZoo(ctx) {
  let reference = null;
  let conformance = null;
  const octaneSrc = {};
  let stop = false;
  // measured[engineId] = { name, octane: {Suite: score}, octaneErr: {Suite: text},
  //   conf: { es15, es6, es2016, dirs: {dir: [pass, total]}, failed: [[path, line]] } }
  let measured = ctx.store.get("zoo-results", null);
  if (!measured || measured.rev !== ctx.rev) measured = { rev: ctx.rev, engines: {} };
  let sort = ctx.store.get("zoo-sort", { col: "score", dir: -1 });

  const save = () => ctx.store.set("zoo-results", measured);
  const getJson = async (f) => (await fetch(f)).json();

  async function loadReference() {
    if (!reference) reference = await getJson("zoo/reference.json");
    return reference;
  }

  function v8Ref() {
    return (reference && reference.engines.find((e) => e.id === "v8")) || null;
  }

  /** Per-suite factor zoo V8 / this browser, when the browser has run. */
  function factors() {
    const b = measured.engines.native;
    const v8 = v8Ref();
    if (!b || !v8) return null;
    const f = {};
    for (const k of SCORES) if (b.octane && b.octane[k] > 0 && v8.octane[k] > 0) f[k] = v8.octane[k] / b.octane[k];
    return Object.keys(f).length ? f : null;
  }

  function rows() {
    const calibrate = $("zoo-calibrate").checked;
    const f = factors();
    const out = [];
    for (const e of reference.engines) {
      const vals = SCORES.map((k) => e.octane[k]).filter((v) => v > 0);
      out.push({ ...e, name: e.title || e.id, score: vals.length ? geo(vals) : null, suites: vals.length, ours: false });
    }
    for (const [id, m] of Object.entries(measured.engines)) {
      const oct = {};
      for (const k of SCORES) {
        const v = m.octane && m.octane[k];
        if (!(v > 0)) continue;
        if (!calibrate || id === "native") oct[k] = v;
        else if (f && f[k]) oct[k] = v * f[k];
      }
      const vals = Object.values(oct);
      out.push({
        id: "this:" + id,
        engineId: id,
        name: m.name + (id === "native" ? " (this browser, raw)" : calibrate ? " (calibrated)" : " (this browser)"),
        octane: oct,
        octaneErr: m.octaneErr || {},
        score: vals.length ? geo(vals) : null,
        suites: vals.length,
        es15: m.conf ? m.conf.es15 : null,
        es6: m.conf ? m.conf.es6 : null,
        es2016: m.conf ? m.conf.es2016 : null,
        ours: true,
      });
    }
    const key = (r) => (sort.col === "name" ? r.name.toLowerCase() : sort.col === "score" ? r.score : SCORES.includes(sort.col) ? r.octane[sort.col] : r[sort.col]);
    out.sort((a, b) => {
      const x = key(a), y = key(b);
      if (x == null && y == null) return 0;
      if (x == null) return 1;
      if (y == null) return -1;
      return (x < y ? -1 : x > y ? 1 : 0) * sort.dir;
    });
    return out;
  }

  function render() {
    if (!reference) return;
    const cols = [
      ["name", "Engine"], ["score", "Score"],
      ...SCORES.map((k) => [k, k]),
      ["es15", "ES1–5"], ["es6", "ES6"], ["es2016", "ES2016+"],
    ];
    const head = "<thead><tr><th>#</th>" + cols.map(([k, l]) => {
      const mark = sort.col === k ? (sort.dir < 0 ? " ▾" : " ▴") : "";
      return `<th data-col="${k}" class="sortable">${esc(l)}${mark}</th>`;
    }).join("") + "</tr></thead>";
    const list = rows();
    let rank = 0;
    const body = list.map((r) => {
      if (r.score != null || sort.col !== "score") rank++;
      const cells = SCORES.map((k) => {
        const v = r.octane[k];
        if (v > 0) return `<td>${fmtScore(v)}</td>`;
        const err = r.octaneErr && r.octaneErr[k];
        return err ? `<td class="err" title="${esc(err)}">✗</td>` : "<td></td>";
      }).join("");
      const partial = r.score != null && r.suites < SCORES.length ? ` <span class="ratio" title="geometric mean of the ${r.suites} suites that finished">(${r.suites}/8)</span>` : "";
      const title = r.ours ? "" : ` title="${esc([r.language, r.jit ? "JIT: " + r.jit : "no JIT", r.revision_date].filter(Boolean).join(" · "))}"`;
      return `<tr class="${r.ours ? "ours" : ""}"><td>${r.score != null || sort.col !== "score" ? rank : ""}</td>` +
        `<td${title}>${esc(r.name)}${r.jit && !r.ours ? ' <span class="jit">JIT</span>' : ""}</td>` +
        `<td><strong>${fmtScore(r.score)}</strong>${partial}</td>${cells}` +
        `<td>${fmtPct(r.es15)}</td><td>${fmtPct(r.es6)}</td><td>${fmtPct(r.es2016)}</td></tr>`;
    }).join("");
    const t = $("zoo-table");
    t.innerHTML = head + "<tbody>" + body + "</tbody>";
    for (const th of t.querySelectorAll("th.sortable")) {
      th.onclick = () => {
        const c = th.dataset.col;
        sort = sort.col === c ? { col: c, dir: -sort.dir } : { col: c, dir: c === "name" ? 1 : -1 };
        ctx.store.set("zoo-sort", sort);
        render();
      };
    }
    renderDetails();
    const f = factors();
    $("zoo-calibration").textContent = f
      ? "Calibration factors (zoo V8 ÷ this browser): " + SCORES.filter((k) => f[k]).map((k) => `${k} ${f[k].toFixed(2)}`).join(", ")
      : "No calibration yet: the browser's own Octane run supplies it.";
  }

  function renderDetails() {
    const box = $("zoo-details");
    const parts = [];
    for (const [id, m] of Object.entries(measured.engines)) {
      if (!m.conf) continue;
      const dirs = Object.entries(m.conf.dirs)
        .map(([d, [p, n]]) => `<li>${esc(d)}: ${p}/${n}${p < n ? ` <span class="err">(${n - p} failing)</span>` : ""}</li>`)
        .join("");
      const failed = m.conf.failed
        .map(([p, line]) => `<li><a href="https://github.com/ivankra/javascript-zoo/blob/main/conformance/${esc(p)}" target="_blank" rel="noopener">${esc(p)}</a>${line ? ` <span class="why">${esc(line)}</span>` : ""}</li>`)
        .join("");
      parts.push(
        `<details><summary>${esc(m.name)}: ES1–5 ${fmtPct(m.conf.es15)}, ES6 ${fmtPct(m.conf.es6)}, ES2016+ ${fmtPct(m.conf.es2016)}` +
          ` · ${m.conf.failed.length} failing</summary><ul class="dirs">${dirs}</ul>` +
          (failed ? `<ul class="failing">${failed}</ul>` : "") + `</details>`,
      );
    }
    box.innerHTML = parts.join("");
  }

  const status = (s) => ($("zoo-status").textContent = s);
  const progress = (done, total) => {
    const p = $("zoo-progress");
    p.hidden = !total;
    p.max = total || 1;
    p.value = done;
  };

  async function runConformance(e, runner, limit) {
    if (!conformance) {
      status("loading the conformance tests…");
      conformance = await getJson("zoo/conformance.json");
    }
    const tests = conformance.tests.filter((t) => GROUPS.some((g) => g.test(t.p)));
    const dirs = {};
    const failed = [];
    const acc = Object.fromEntries(GROUPS.map((g) => [g.key, { p: 0, n: 0, wp: 0, w: 0 }]));
    let i = 0;
    for (const t of tests) {
      if (stop) return null;
      if (i % 10 === 0) {
        status(`${e.name} · conformance ${i}/${tests.length}`);
        progress(i, tests.length);
      }
      i++;
      const m = await runner.run(t.c, 1, limit, { fresh: true, settle: true });
      const ok = !m.timeout && verdict(t.p, m.out || []);
      const d = DIRS(t.p);
      const dd = (dirs[d] ||= [0, 0]);
      dd[1]++;
      if (ok) dd[0]++;
      else {
        const why = m.timeout ? "timeout" : (m.out || []).filter((l) => l.trim()).slice(-1)[0] || "no output";
        failed.push([t.p, why.length > 200 ? why.slice(0, 200) + "…" : why]);
      }
      for (const g of GROUPS) {
        if (!g.test(t.p)) continue;
        const a = acc[g.key];
        a.n++;
        a.w += t.w || 0;
        if (ok) {
          a.p++;
          a.wp += t.w || 0;
        }
      }
    }
    const pct = (g) => {
      const a = acc[g.key];
      if (!a.n) return null;
      return g.weighted ? (100 * a.wp) / a.w : (100 * a.p) / a.n;
    };
    return { es15: pct(GROUPS[0]), es6: pct(GROUPS[1]), es2016: pct(GROUPS[2]), dirs, failed };
  }

  async function runOctane(e, runner, limit, rec) {
    rec.octane = {};
    rec.octaneErr = {};
    let i = 0;
    for (const k of SCORES) {
      if (stop) return;
      status(`${e.name} · Octane ${k} (${++i}/8)`);
      progress(i - 1, SCORES.length);
      const file = SUITE_FILE[k];
      if (!octaneSrc[file]) octaneSrc[file] = prepareOctane(await (await fetch(`zoo/octane/${file}.js`)).text());
      const m = await runner.run(octaneSrc[file], 1, limit, { fresh: true });
      const r = parseOctane(m.out || []);
      if (r.scores[k] > 0) rec.octane[k] = r.scores[k];
      else rec.octaneErr[k] = m.timeout ? `no score in ${limit / 60000} min` : r.error || (m.out || []).slice(-1)[0] || "no score";
      save();
      render();
    }
  }

  async function run() {
    await loadReference();
    const engines = ctx.chosen();
    const doConf = $("zoo-conf").checked;
    const doSpeed = $("zoo-speed").checked;
    const suiteLimit = Number($("zoo-limit").value);
    stop = false;
    $("zoo-run").disabled = true;
    $("zoo-stop").disabled = false;
    // Conformance first (minutes), then speed (longer), one engine at a
    // time so no two compete for the CPU.
    const phases = [];
    if (doConf) for (const e of engines) phases.push(["conf", e]);
    if (doSpeed) for (const e of engines) phases.push(["speed", e]);
    for (const [kind, e] of phases) {
      if (stop) break;
      const runner = ctx.runnerFor(e);
      const rec = (measured.engines[e.id] ||= { name: e.name });
      rec.name = e.name;
      ctx.setEngineState(e.id, kind === "conf" ? "conformance…" : "Octane…");
      if (kind === "conf") {
        const c = await runConformance(e, runner, 10000);
        if (c) rec.conf = c;
      } else {
        await runOctane(e, runner, suiteLimit, rec);
      }
      save();
      render();
      ctx.setEngineState(e.id, stop ? "stopped" : "done", stop ? "" : "ok");
    }
    progress(0, 0);
    status(stop ? "stopped" : "done");
    $("zoo-run").disabled = false;
    $("zoo-stop").disabled = true;
  }

  $("zoo-run").onclick = run;
  $("zoo-stop").onclick = () => {
    stop = true;
    for (const e of ctx.chosen()) ctx.runnerFor(e).cancel();
  };
  $("zoo-clear").onclick = () => {
    measured = { rev: ctx.rev, engines: {} };
    save();
    render();
  };
  $("zoo-calibrate").checked = ctx.store.get("zoo-calibrate", true);
  $("zoo-calibrate").onchange = () => {
    ctx.store.set("zoo-calibrate", $("zoo-calibrate").checked);
    render();
  };

  loadReference().then(() => {
    $("zoo-source").innerHTML =
      `Reference: <a href="https://zoo.js.org/?arch=amd64&v8=true" target="_blank" rel="noopener">zoo.js.org</a> ` +
      `(${reference.engines.length} engines, amd64, <a href="${esc(reference.source)}" target="_blank" rel="noopener">data of ${esc(reference.date)}</a>).`;
    render();
  });
}
