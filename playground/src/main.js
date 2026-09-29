// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The page: which engines were built (engines.json, written by build.mjs),
// one worker per engine, the benchmark and the editor.

import { WORKLOADS, SAMPLE, wrap } from "./workloads.js";
import { setupZoo } from "./zoo.js";

const $ = (id) => document.getElementById(id);
const store = {
  get(k, d) {
    try {
      const v = localStorage.getItem("ce-playground:" + k);
      return v === null ? d : JSON.parse(v);
    } catch {
      return d;
    }
  },
  set(k, v) {
    try {
      localStorage.setItem("ce-playground:" + k, JSON.stringify(v));
    } catch {
      /* private window: nothing kept */
    }
  },
};

// ---- engines ---------------------------------------------------------------

class Runner {
  constructor(info) {
    this.info = info;
    this.worker = null;
    this.seq = 0;
    this.pending = null;
  }
  start() {
    this.worker = new Worker("workers/" + this.info.id + ".js", { type: "module" });
    this.worker.onmessage = (ev) => {
      const m = ev.data;
      if (m.ready) return;
      if (this.pending && m.id === this.pending.id) {
        clearTimeout(this.pending.timer);
        const p = this.pending;
        this.pending = null;
        p.resolve(m);
      }
    };
    this.worker.onerror = (ev) => {
      if (!this.pending) return;
      const p = this.pending;
      this.pending = null;
      clearTimeout(p.timer);
      p.resolve({ ok: false, error: true, out: [ev.message || "the worker failed"], times: [] });
      this.stop();
    };
  }
  stop() {
    if (this.worker) this.worker.terminate();
    this.worker = null;
  }
  /** Runs `src` `reps` times; a run past `limit` ms ends the worker.
   * opts.fresh: a new realm first; opts.settle: drain the job queue after. */
  run(src, reps, limit, opts = {}) {
    // The browser's engine has no second realm inside a worker: a fresh
    // worker is its fresh realm.
    if (opts.fresh && this.info.id === "native") this.stop();
    if (!this.worker) this.start();
    const id = ++this.seq;
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.pending = null;
        this.stop();
        resolve({ ok: false, timeout: true, error: true, out: ["no answer in " + limit / 1000 + " s"], times: [] });
      }, limit);
      this.pending = { id, resolve, timer };
      this.worker.postMessage({ cmd: "run", id, src, reps, fresh: !!opts.fresh, settle: !!opts.settle });
    });
  }
  cancel() {
    if (this.pending) {
      clearTimeout(this.pending.timer);
      this.pending.resolve({ ok: false, cancelled: true, error: true, out: ["stopped"], times: [] });
      this.pending = null;
    }
    this.stop();
  }
}

let ENGINES = [];
const runners = new Map();
const enabled = new Set(store.get("engines", null) || []);

function renderEngines() {
  const list = $("engine-list");
  list.innerHTML = "";
  for (const e of ENGINES) {
    const card = document.createElement("label");
    card.className = "engine" + (e.available ? "" : " off");
    const box = document.createElement("input");
    box.type = "checkbox";
    box.disabled = !e.available;
    box.checked = e.available && enabled.has(e.id);
    box.onchange = () => {
      if (box.checked) enabled.add(e.id);
      else enabled.delete(e.id);
      store.set("engines", [...enabled]);
    };
    const text = document.createElement("div");
    text.innerHTML = `<div class="name"></div><div class="kind"></div><div class="state"></div>`;
    text.querySelector(".name").textContent = e.name;
    text.querySelector(".kind").textContent = e.available
      ? e.size
        ? (e.size / 1048576).toFixed(1) + " MB to load"
        : ""
      : "not in this build: " + (e.reason || "missing");
    text.id = "state-" + e.id;
    card.append(box, text);
    list.append(card);
  }
}

function setEngineState(id, text, cls) {
  const el = document.querySelector("#state-" + CSS.escape(id) + " .state");
  if (el) {
    el.textContent = text;
    el.className = "state " + (cls || "");
  }
}

function chosen() {
  // The browser's engine is the reference: it always runs first.
  const list = ENGINES.filter((e) => e.available && enabled.has(e.id));
  const native = ENGINES.find((e) => e.id === "native");
  return [native, ...list.filter((e) => e.id !== "native")];
}

function runnerFor(e) {
  if (!runners.has(e.id)) runners.set(e.id, new Runner(e));
  return runners.get(e.id);
}

// ---- benchmark -------------------------------------------------------------

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  const n = s.length;
  return n ? (n % 2 ? s[(n - 1) >> 1] : (s[n / 2 - 1] + s[n / 2]) / 2) : NaN;
};
const fmtMs = (ms) => (ms >= 100 ? ms.toFixed(0) : ms >= 10 ? ms.toFixed(1) : ms >= 1 ? ms.toFixed(2) : ms.toFixed(3));
const fmtX = (x) => (x >= 100 ? x.toFixed(0) : x >= 10 ? x.toFixed(1) : x.toFixed(2)) + "×";

let stopRequested = false;
let results = {}; // results[workload][engine] = { ms, out, error, wrong, note }

function checkedWorkloads() {
  const off = new Set(store.get("workloads-off", []));
  return WORKLOADS.filter((w) => !off.has(w.id));
}

function renderWorkloadList() {
  const list = $("workload-list");
  const off = new Set(store.get("workloads-off", []));
  list.innerHTML = "";
  for (const w of WORKLOADS) {
    const l = document.createElement("label");
    const b = document.createElement("input");
    b.type = "checkbox";
    b.checked = !off.has(w.id);
    b.onchange = () => {
      if (b.checked) off.delete(w.id);
      else off.add(w.id);
      store.set("workloads-off", [...off]);
      $("wl-count").textContent = checkedWorkloads().length + " of " + WORKLOADS.length;
    };
    const about = document.createElement("span");
    about.className = "about";
    about.textContent = w.about;
    l.append(b, " " + w.name, about);
    list.append(l);
  }
  $("wl-count").textContent = checkedWorkloads().length + " of " + WORKLOADS.length;
}

function renderTable(engines, workloads) {
  const t = $("results");
  const head = "<thead><tr><th>workload</th>" + engines.map((e) => `<th>${esc(e.name)}</th>`).join("") + "</tr></thead>";
  const rows = workloads.map((w) => {
    const r = results[w.id] || {};
    const base = r.native && !r.native.error ? r.native.ms : NaN;
    const cells = engines.map((e) => {
      const c = r[e.id];
      if (!c) return `<td class="pending">·</td>`;
      if (c.error) return `<td class="err" title="${esc(c.out.join("\n"))}">${c.timeout ? "timeout" : "error"}</td>`;
      const ratio = e.id !== "native" && base > 0 ? `<span class="ratio">${fmtX(c.ms / base)}</span>` : "";
      const wrong = c.wrong ? ` class="wrong" title="answered ${esc(c.out.join(" | "))}; the browser answered ${esc(r.native.out.join(" | "))}"` : "";
      return `<td${wrong}>${fmtMs(c.ms)}${ratio}${c.wrong ? " ≠" : ""}</td>`;
    });
    return `<tr><td>${esc(w.name)}</td>${cells.join("")}</tr>`;
  });
  t.innerHTML = head + "<tbody>" + rows.join("") + "</tbody>";
}

function summarise(engines, workloads) {
  const out = [];
  for (const e of engines) {
    if (e.id === "native") continue;
    const logs = [];
    const detail = [];
    let failed = 0;
    for (const w of workloads) {
      const r = results[w.id] || {};
      const c = r[e.id];
      const b = r.native;
      if (!c || !b || b.error) continue;
      if (c.error || c.wrong) {
        failed++;
        continue;
      }
      const ratio = c.ms / Math.max(b.ms, 0.001);
      logs.push(Math.log(ratio));
      detail.push([w.name, ratio]);
    }
    if (logs.length) {
      out.push({ e, x: Math.exp(logs.reduce((a, b) => a + b, 0) / logs.length), n: logs.length, failed, detail });
    }
  }
  return out;
}

function renderSummary(engines, workloads) {
  const el = $("summary");
  const rows = summarise(engines, workloads);
  el.innerHTML = "";
  if (!rows.length) {
    el.innerHTML = `<div class="empty">Run the benchmark to see how far each engine is from the browser's.</div>`;
    return;
  }
  const maxX = Math.max(10, ...rows.map((r) => r.x));
  const top = Math.pow(10, Math.ceil(Math.log10(maxX)));
  const pos = (x) => (Math.log10(Math.max(1, x)) / Math.log10(top)) * 100;
  const ticks = [];
  for (let v = 1; v <= top; v *= 10) ticks.push(v);
  for (const r of rows) {
    const label = document.createElement("div");
    label.className = "label";
    label.textContent = r.e.name;
    const track = document.createElement("div");
    track.className = "track";
    for (const v of ticks) {
      const g = document.createElement("div");
      g.className = "grid";
      g.style.left = pos(v) + "%";
      track.append(g);
    }
    const bar = document.createElement("div");
    bar.className = "bar";
    bar.style.width = `calc(${pos(r.x)}% - 2px)`;
    const val = document.createElement("div");
    val.className = "value";
    val.style.left = `calc(${pos(r.x)}% + 6px)`;
    val.textContent = fmtX(r.x) + (r.failed ? `  (${r.failed} not answered)` : "");
    if (pos(r.x) > 80) {
      val.style.left = "auto";
      val.style.right = `calc(${100 - pos(r.x)}% + 6px)`;
      val.style.color = "#fff";
    }
    track.append(bar, val);
    track.onmousemove = (ev) => showTip(ev, r);
    track.onmouseleave = hideTip;
    el.append(label, track);
  }
  const axis = document.createElement("div");
  axis.className = "axis";
  for (const v of ticks) {
    const s = document.createElement("span");
    s.style.left = pos(v) + "%";
    s.textContent = v + "×";
    axis.append(s);
  }
  el.append(document.createElement("div"), axis);
}

function showTip(ev, r) {
  const tip = $("tooltip");
  const box = tip.parentElement.getBoundingClientRect();
  tip.innerHTML =
    `<strong>${esc(r.e.name)}</strong><br>${fmtX(r.x)} the browser's time (geometric mean of ${r.n})` +
    (r.failed ? `<br>${r.failed} workload(s) wrong, failed or timed out` : "") +
    "<br>" +
    r.detail.map(([n, x]) => `${esc(n)}: ${fmtX(x)}`).join("<br>");
  tip.hidden = false;
  const x = Math.min(ev.clientX - box.left + 14, box.width - 330);
  tip.style.left = Math.max(0, x) + "px";
  tip.style.top = ev.clientY - box.top + 14 + "px";
}
function hideTip() {
  $("tooltip").hidden = true;
}

async function runBenchmark() {
  const engines = chosen();
  const workloads = checkedWorkloads();
  const scale = Number($("scale").value);
  const reps = Number($("reps").value);
  const limit = Number($("limit").value);
  stopRequested = false;
  results = {};
  $("run-bench").disabled = true;
  $("stop-bench").disabled = false;
  renderTable(engines, workloads);
  renderSummary(engines, workloads);
  let n = 0;
  const total = engines.length * workloads.length;
  // One engine at a time, so no two compete for the CPU.
  outer: for (const e of engines) {
    const runner = runnerFor(e);
    setEngineState(e.id, "running…");
    for (const w of workloads) {
      if (stopRequested) break outer;
      $("bench-status").textContent = `${++n}/${total}  ${e.name} · ${w.name}`;
      const m = await runner.run(wrap(w.code, scale), reps, limit);
      const r = (results[w.id] ||= {});
      const cell = { ms: median(m.times), out: m.out || [], error: !!m.error || !m.ok, timeout: !!m.timeout };
      if (e.id !== "native" && r.native && !r.native.error && !cell.error) {
        cell.wrong = cell.out.join("\n") !== r.native.out.join("\n");
      }
      r[e.id] = cell;
      renderTable(engines, workloads);
      renderSummary(engines, workloads);
    }
    setEngineState(e.id, "done", "ok");
  }
  $("bench-status").textContent = stopRequested ? "stopped" : "done";
  $("run-bench").disabled = false;
  $("stop-bench").disabled = true;
}

// ---- editor ----------------------------------------------------------------

async function runEditor() {
  const engines = chosen();
  const code = $("code").value;
  store.set("code", code);
  const reps = Number($("editor-reps").value);
  const box = $("editor-results");
  box.innerHTML = "";
  $("run-editor").disabled = true;
  const cards = new Map();
  for (const e of engines) {
    const card = document.createElement("div");
    card.className = "result-card";
    card.innerHTML = `<div class="head"><span></span><span class="time">…</span></div><pre></pre>`;
    card.querySelector(".head span").textContent = e.name;
    box.append(card);
    cards.set(e.id, card);
  }
  let reference = null;
  for (const e of engines) {
    $("editor-status").textContent = "running in " + e.name;
    const m = await runnerFor(e).run(wrap(code, 1), reps, 60000);
    const card = cards.get(e.id);
    const text = (m.out || []).join("\n");
    const pre = card.querySelector("pre");
    pre.textContent = text || "(nothing printed)";
    pre.className = m.error ? "err" : "";
    let t = m.times && m.times.length ? fmtMs(median(m.times)) + " ms" : m.timeout ? "timeout" : "error";
    if (m.loadMs > 1) t += ` · loaded in ${fmtMs(m.loadMs)} ms`;
    card.querySelector(".time").textContent = t;
    if (e.id === "native") reference = text;
    else if (reference !== null && !m.error) {
      const same = text === reference;
      const tag = document.createElement("span");
      tag.className = "match " + (same ? "ok" : "bad");
      tag.textContent = same ? " same as the browser" : " differs from the browser";
      card.querySelector(".head span").append(tag);
    }
  }
  $("editor-status").textContent = "";
  $("run-editor").disabled = false;
}

// ---- page ------------------------------------------------------------------

function esc(s) {
  return String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}

function tabs() {
  const ids = ["bench", "zoo", "editor", "about"];
  const show = (id) => {
    for (const t of ids) {
      $("tab-" + t).setAttribute("aria-selected", String(t === id));
      $("panel-" + t).hidden = t !== id;
    }
    store.set("tab", id);
  };
  for (const t of ids) $("tab-" + t).onclick = () => show(t);
  show(store.get("tab", "bench"));
}

function renderAbout() {
  $("about").innerHTML = ENGINES.map(
    (e) =>
      `<div class="card"><h4>${esc(e.name)}</h4><p>${esc(e.about)}</p>` +
      (e.available ? "" : `<p><em>Not in this build: ${esc(e.reason || "missing")}</em></p>`) +
      `</div>`,
  ).join("");
}

async function main() {
  const manifest = await (await fetch("engines.json")).json();
  ENGINES = manifest.engines;
  $("build-rev").textContent = manifest.rev || "a local build";
  if (!store.get("engines", null)) for (const e of ENGINES) if (e.available) enabled.add(e.id);
  renderEngines();
  renderWorkloadList();
  renderAbout();
  tabs();
  setupZoo({ chosen, runnerFor, store, rev: manifest.rev || manifest.built, setEngineState });
  renderSummary([], []);

  const code = $("code");
  code.value = store.get("code", SAMPLE);
  code.addEventListener("keydown", (ev) => {
    if (ev.key === "Tab" && !ev.shiftKey) {
      ev.preventDefault();
      const s = code.selectionStart;
      code.setRangeText("  ", s, code.selectionEnd, "end");
    } else if (ev.key === "Enter" && (ev.ctrlKey || ev.metaKey)) {
      ev.preventDefault();
      runEditor();
    }
  });
  const sample = $("load-sample");
  for (const w of WORKLOADS) {
    const o = document.createElement("option");
    o.value = w.id;
    o.textContent = w.name;
    sample.append(o);
  }
  sample.onchange = () => {
    const w = WORKLOADS.find((x) => x.id === sample.value);
    if (w) code.value = w.code.replace(/\bSCALE\b/g, "1");
    sample.value = "";
  };

  $("run-bench").onclick = runBenchmark;
  $("stop-bench").onclick = () => {
    stopRequested = true;
    for (const r of runners.values()) r.cancel();
  };
  $("run-editor").onclick = runEditor;
}

main();
