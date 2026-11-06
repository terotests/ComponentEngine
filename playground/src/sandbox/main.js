// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The sandbox demo's page: four cards, each a CErXes engine running one of
// programs.js's endless fractal programs. Where an engine runs is the card's
// host:
//
//   main    core.js on this page: one WebAssembly instance, one engine per
//           card, the cards' slices taken in turn between the page's own tasks
//   worker  core.js in a Web Worker of the card's own (a thread of its own)
//   frame   core.js in a hidden iframe of the card's own (a document of its
//           own on the page's thread)
//
// All three take the same messages (core.js). Moving a card to another host
// is a freeze in the old one (the engine's state as text) and a thaw in the
// new one: the program goes on where it was, mid-loop.

import { PROGRAMS } from "./programs.js";
import { loadCerxes, serve, timeoutYield } from "./core.js";

const W = 320;
const H = 240;
const $ = (id) => document.getElementById(id);
const statusEl = $("status");
const params = new URLSearchParams(location.search);
const HOST_NAMES = { main: "this page", worker: "Web Worker", frame: "iframe" };

let sliceMs = Math.max(1, Math.min(60, +(params.get("slice") || 4)));
$("slice").value = String(sliceMs);
$("slice-out").textContent = sliceMs + " ms";

// ---- hosts ------------------------------------------------------------------

const replies = new Map(); // "type:id" -> resolve
function dispatch(m) {
  const key = m.type + ":" + m.id;
  if (replies.has(key)) {
    replies.get(key)(m);
    replies.delete(key);
  }
  const s = boxes[m.id];
  if (s && m.type === "tick") onTick(s, m);
}

// The page's own host, shared by every card that runs here.
let mainHandle = null;
function mainHost() {
  if (!mainHandle) mainHandle = serve(loadCerxes(new URL("../wasm/cerxes.wasm", import.meta.url)), dispatch, timeoutYield);
  return { kind: "main", send: (m) => mainHandle(m), close() {} };
}

function workerHost() {
  const w = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
  w.onmessage = (ev) => dispatch(ev.data);
  w.onerror = (ev) => statusEl.textContent = "worker: " + (ev.message || "failed");
  return { kind: "worker", send: (m) => w.postMessage(m), close: () => w.terminate() };
}

const frames = new Map(); // contentWindow -> host
function frameHost() {
  const f = document.createElement("iframe");
  f.src = new URL("./frame.html", import.meta.url).href;
  f.title = "sandbox host";
  const queue = [];
  const host = {
    kind: "frame",
    ready: false,
    send(m) {
      if (host.ready) f.contentWindow.postMessage(m, location.origin);
      else queue.push(m);
    },
    close() {
      frames.delete(f.contentWindow);
      f.remove();
    },
    onReady() {
      host.ready = true;
      for (const m of queue.splice(0)) f.contentWindow.postMessage(m, location.origin);
    },
  };
  $("frames").append(f);
  frames.set(f.contentWindow, host);
  return host;
}
addEventListener("message", (ev) => {
  if (ev.origin !== location.origin) return;
  const host = frames.get(ev.source);
  if (!host) return;
  if (ev.data && ev.data.type === "ready") host.onReady();
  else dispatch(ev.data);
});

const makeHost = (kind) => (kind === "worker" ? workerHost() : kind === "frame" ? frameHost() : mainHost());

function request(s, m, type) {
  return new Promise((resolve) => {
    replies.set(type + ":" + s.id, resolve);
    s.host.send({ ...m, id: s.id });
  });
}

// ---- the cards ---------------------------------------------------------------

const boxes = {};

// iteration count -> colour, a cosine palette turned by the program's hue
function palette(hue) {
  const lut = new Uint32Array(4096);
  for (let n = 0; n < 4096; n++) {
    const t = Math.sqrt(n) * 0.09 + hue;
    const c = (k) => Math.round(255 * (0.5 + 0.5 * Math.cos(6.2832 * (t + k))));
    lut[n] = (255 << 24) | (c(0.2) << 16) | (c(0.1) << 8) | c(0.0);
  }
  return lut;
}

function makeCard(prog, i) {
  const el = $("card").content.firstElementChild.cloneNode(true);
  const canvas = el.querySelector("canvas");
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  const s = {
    id: i,
    prog,
    el,
    canvas,
    ctx,
    img: ctx.createImageData(W, H),
    lut: palette(prog.hue),
    code: prog.code,
    view: { gen: 1, cx: prog.view[0], cy: prog.view[1], scale: prog.view[2] },
    host: null,
    kind: "main",
    paused: false,
    frozen: null, // { snapshot, state } while frozen
    progress: "",
    tick: null,
    dirty: false,
    failed: false,
  };
  new Uint32Array(s.img.data.buffer).fill(0xff000000);
  el.querySelector(".sbx-name").textContent = prog.name;
  el.querySelector("textarea").value = s.code;
  el.querySelector(".sbx-host").onchange = (ev) => move(s, ev.target.value);
  el.querySelector(".b-pause").onclick = () => setPaused(s, !s.paused);
  el.querySelector(".b-freeze").onclick = () => (s.frozen ? thaw(s) : freeze(s));
  el.querySelector(".b-home").onclick = () => setView(s, prog.view[0], prog.view[1], prog.view[2]);
  el.querySelector(".b-fetch").onclick = () => fetchOne(s).then((r) => show(s, r));
  el.querySelector(".b-code").onclick = () => {
    const box = el.querySelector(".sbx-code");
    box.hidden = !box.hidden;
  };
  el.querySelector(".b-run").onclick = () => {
    s.code = el.querySelector("textarea").value;
    start(s);
  };
  el.querySelector(".b-reset").onclick = () => {
    el.querySelector("textarea").value = prog.code;
  };
  wireZoom(s);
  $("grid").append(el);
  boxes[i] = s;
  return s;
}

const viewText = (v) => [v.gen, v.cx, v.cy, v.scale].join(",");

function start(s) {
  if (s.host) s.host.close();
  s.frozen = null;
  s.failed = false;
  s.paused = false;
  s.view.gen++;
  s.host = makeHost(s.kind);
  s.host.send({ type: "start", id: s.id, code: s.code, view: viewText(s.view), slice: sliceMs });
  s.el.querySelector(".sbx-out").hidden = true;
  refreshButtons(s);
}

function setPaused(s, paused) {
  s.paused = paused;
  if (s.host && !s.frozen) s.host.send({ type: paused ? "pause" : "unpause", id: s.id });
  refreshButtons(s);
}

function refreshButtons(s) {
  s.el.querySelector(".b-pause").textContent = s.paused ? "Go on" : "Pause";
  s.el.querySelector(".b-pause").disabled = !!s.frozen;
  s.el.querySelector(".b-freeze").textContent = s.frozen ? "Thaw" : "Freeze";
  s.el.querySelector(".sbx-host").value = s.kind;
  s.el.classList.toggle("failed", s.failed);
  stats(s);
}

// The engine as text; the sandbox is gone from its host until a thaw.
async function freeze(s) {
  const r = await request(s, { type: "freeze" }, "frozen");
  if (!r.snapshot) {
    note(s, "Freeze failed: " + r.error);
    return null;
  }
  s.frozen = { snapshot: r.snapshot, state: r.state };
  note(s, `Frozen in ${r.ms.toFixed(1)} ms: ${(r.snapshot.length / 1024).toFixed(0)} KB of text (objects, compiled code, the running loop's frames).`);
  refreshButtons(s);
  return r;
}

async function thaw(s) {
  if (!s.frozen) return;
  if (!s.host || s.host.kind !== s.kind) {
    if (s.host) s.host.close();
    s.host = makeHost(s.kind);
  }
  const f = s.frozen;
  const r = await request(s, { type: "thaw", snapshot: f.snapshot, state: f.state, slice: sliceMs }, "thawed");
  if (!r.ok) {
    note(s, "Thaw failed: " + r.error);
    return;
  }
  s.frozen = null;
  // the view may have changed while frozen
  s.host.send({ type: "view", id: s.id, view: viewText(s.view) });
  s.paused = !!(f.state && f.state.paused);
  note(s, `Thawed on ${HOST_NAMES[s.kind]} in ${r.ms.toFixed(1)} ms; the program goes on where it stopped.`);
  refreshButtons(s);
}

// Freeze where it runs, thaw where it goes.
async function move(s, kind) {
  if (kind === s.kind) return;
  const from = s.kind;
  if (s.frozen) {
    s.kind = kind;
    refreshButtons(s);
    return;
  }
  const t = performance.now();
  const r = await freeze(s);
  if (!r) {
    refreshButtons(s);
    return;
  }
  s.host.close();
  s.host = null;
  s.kind = kind;
  await thaw(s);
  note(s, `Moved from ${HOST_NAMES[from]} to ${HOST_NAMES[kind]} mid-run in ${(performance.now() - t).toFixed(0)} ms (snapshot ${(r.snapshot.length / 1024).toFixed(0)} KB).`);
}

function note(s, text) {
  const out = s.el.querySelector(".sbx-out");
  out.hidden = false;
  out.textContent = text;
}

function fetchOne(s) {
  if (s.frozen || !s.host) return Promise.resolve({ progress: s.progress, result: "" });
  return request(s, { type: "fetch" }, "fetched");
}

function show(s, r) {
  let text = "progress: " + (r.progress || "-") + "\n";
  try {
    text += "result: " + JSON.stringify(JSON.parse(r.result), null, 2);
  } catch (e) {
    text += "result: " + r.result;
  }
  note(s, text);
}

// ---- zoom ----------------------------------------------------------------------

// Past this width a double has too few bits left for the pixels' steps.
const MIN_SCALE = 1e-13;

// Pointer input on a card's picture: a click zooms in there (Shift or the
// right button: out), the wheel zooms about the pointer, a drag with the
// mouse pans (the picture follows the pointer, the program gets the new
// view when the button is let go), the + / − buttons zoom about the centre.
function wireZoom(s) {
  const canvas = s.canvas;
  const at = (ev) => {
    const rect = canvas.getBoundingClientRect();
    return [((ev.clientX - rect.left) / rect.width) * W, ((ev.clientY - rect.top) / rect.height) * H, rect.width / W];
  };
  let drag = null;
  canvas.addEventListener("pointerdown", (ev) => {
    if (ev.button !== 0 || ev.pointerType === "touch") return;
    drag = { x: ev.clientX, y: ev.clientY, moved: false };
    canvas.setPointerCapture(ev.pointerId);
  });
  canvas.addEventListener("pointermove", (ev) => {
    if (!drag) return;
    const dx = ev.clientX - drag.x, dy = ev.clientY - drag.y;
    if (!drag.moved && Math.hypot(dx, dy) < 4) return;
    drag.moved = true;
    canvas.classList.add("dragging");
    canvas.style.transform = `translate(${dx}px, ${dy}px)`;
  });
  const endDrag = (ev) => {
    if (!drag) return;
    const d = drag;
    drag = null;
    canvas.classList.remove("dragging");
    canvas.style.transform = "";
    if (!d.moved) return;
    const k = at(ev)[2];
    const v = s.view;
    const step = v.scale / W;
    setView(s, v.cx - ((ev.clientX - d.x) / k) * step, v.cy - ((ev.clientY - d.y) / k) * step, v.scale);
    s.skipClick = true;
  };
  canvas.addEventListener("pointerup", endDrag);
  canvas.addEventListener("pointercancel", endDrag);
  canvas.addEventListener("click", (ev) => {
    if (s.skipClick) {
      s.skipClick = false;
      return;
    }
    const [px, py] = at(ev);
    zoomAbout(s, px, py, ev.shiftKey ? 2 : 0.5, true);
  });
  canvas.addEventListener("contextmenu", (ev) => {
    ev.preventDefault();
    const [px, py] = at(ev);
    zoomAbout(s, px, py, 2, true);
  });
  canvas.addEventListener(
    "wheel",
    (ev) => {
      ev.preventDefault();
      const [px, py] = at(ev);
      const notches = Math.max(-3, Math.min(3, ev.deltaY / (ev.deltaMode === 1 ? 3 : 100)));
      zoomAbout(s, px, py, Math.pow(1.25, notches), false);
    },
    { passive: false }
  );
  s.el.querySelector(".z-in").onclick = () => zoomAbout(s, W / 2, H / 2, 0.5, false);
  s.el.querySelector(".z-out").onclick = () => zoomAbout(s, W / 2, H / 2, 2, false);
}

// Zooms by `factor` (< 1: in) about canvas pixel (px, py): with `centre`
// that point becomes the middle of the new picture, else it stays put
// under the pointer.
function zoomAbout(s, px, py, factor, centre) {
  const v = s.view;
  const scale = Math.max(MIN_SCALE, Math.min(16, v.scale * factor));
  if (scale === v.scale) return;
  const step = v.scale / W;
  const x = v.cx - v.scale / 2 + px * step;
  const y = v.cy - (step * H) / 2 + py * step;
  if (centre) setView(s, x, y, scale);
  else {
    const f = scale / v.scale;
    setView(s, x + (v.cx - x) * f, y + (v.cy - y) * f, scale);
  }
}

function setView(s, cx, cy, scale) {
  const v = s.view;
  // the old picture moved to where it lands in the new view, until the
  // program has drawn the new one over it
  const step = v.scale / W;
  const step2 = scale / W;
  const x0 = v.cx - v.scale / 2, y0 = v.cy - (step * H) / 2;
  const x1 = cx - scale / 2, y1 = cy - (step2 * H) / 2;
  const tmp = document.createElement("canvas");
  tmp.width = W;
  tmp.height = H;
  tmp.getContext("2d").putImageData(s.img, 0, 0);
  s.ctx.fillStyle = "#000";
  s.ctx.fillRect(0, 0, W, H);
  s.ctx.imageSmoothingEnabled = false;
  s.ctx.drawImage(tmp, (x0 - x1) / step2, (y0 - y1) / step2, (W * step) / step2, (H * step) / step2);
  s.img = s.ctx.getImageData(0, 0, W, H);
  s.view = { gen: v.gen + 1, cx, cy, scale };
  if (s.host && !s.frozen) s.host.send({ type: "view", id: s.id, view: viewText(s.view) });
  s.el.classList.remove("waiting");
  stats(s);
}

// ---- what the programs send -------------------------------------------------------

const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const DIGIT = new Int16Array(128).fill(-1);
for (let i = 0; i < 64; i++) DIGIT[B64.charCodeAt(i)] = i;

function onTick(s, m) {
  s.tick = m;
  if (m.progress) s.progress = m.progress;
  const px = new Uint32Array(s.img.data.buffer);
  for (const line of m.rows) {
    // row <gen> <y> <block> <maxIter> <data>
    const p = line.split(" ");
    if (+p[1] !== s.view.gen) continue;
    const y = +p[2], block = +p[3], data = p[5] || "";
    for (let i = 0, x = 0; i + 1 < data.length; i += 2, x += block) {
      const a = DIGIT[data.charCodeAt(i)];
      const color = a < 0 || data[i] === "/" && data[i + 1] === "/" ? 0xff000000 : s.lut[(a << 6) | DIGIT[data.charCodeAt(i + 1)]];
      for (let yy = y; yy < y + block && yy < H; yy++) px.fill(color, yy * W + x, yy * W + Math.min(W, x + block));
    }
    s.dirty = true;
  }
  if (m.logs.length) note(s, m.logs.slice(-20).join("\n"));
  if (m.status === 1) {
    s.failed = true;
    note(s, m.error);
    refreshButtons(s);
  }
  s.el.classList.toggle("waiting", /^\d+ done/.test(s.progress));
  if (!statsQueued) {
    statsQueued = true;
    requestAnimationFrame(paint);
  }
}

let statsQueued = false;
function paint() {
  statsQueued = false;
  for (const s of Object.values(boxes)) {
    if (s.dirty) {
      s.ctx.putImageData(s.img, 0, 0);
      s.dirty = false;
    }
    stats(s);
  }
}

function stats(s) {
  const zoom = s.prog.view[2] / s.view.scale;
  s.el.querySelector(".sbx-level").textContent = "zoom ×" + (zoom < 1e5 ? zoom.toPrecision(3) : zoom.toExponential(2)) + (s.view.scale <= MIN_SCALE ? " (precision limit)" : "");
  const el = s.el.querySelector(".sbx-stats");
  const bar = s.el.querySelector(".sbx-bar div");
  const p = s.progress.split(" ");
  const current = +p[0] === s.view.gen;
  const where = HOST_NAMES[s.kind] + (s.frozen ? " · frozen" : s.paused ? " · paused" : "");
  if (!current) {
    bar.style.width = "0";
    el.textContent = where + " · starting zoom ×" + (s.prog.view[2] / s.view.scale).toPrecision(3);
    return;
  }
  const done = p[1] === "done";
  bar.style.width = (done ? 100 : (100 * +p[2]) / +p[3]).toFixed(1) + "%";
  const t = s.tick;
  el.innerHTML = "";
  el.append(
    `${where} · zoom ×${(s.prog.view[2] / s.view.scale).toPrecision(3)} · ` +
      (done ? `finished at ${p[4]} iterations, waiting for a new view` : `pass ${+p[1] + 1}, rows ${p[2]}/${p[3]}, up to ${p[4]} iterations`),
    document.createElement("br"),
    t ? `${t.slices} slices · ${(t.cpu / 1000).toFixed(1)} s in the engine` : ""
  );
}

// ---- the page's own pulse ------------------------------------------------------------

const pulse = $("pulse").getContext("2d");
let last = performance.now();
let frames1s = [];
function beat(now) {
  const gap = now - last;
  last = now;
  frames1s.push([now, gap]);
  while (frames1s.length && frames1s[0][0] < now - 1000) frames1s.shift();
  const worst = Math.max(...frames1s.map((f) => f[1]));
  $("fps").textContent = `${frames1s.length} frames/s · longest gap ${worst.toFixed(0)} ms`;
  const c = pulse.canvas;
  pulse.clearRect(0, 0, c.width, c.height);
  pulse.fillStyle = getComputedStyle(document.body).getPropertyValue("--accent") || "#2a78d6";
  const x = ((now / 1500) % 1) * (c.width + 30) - 30;
  pulse.fillRect(x, 4, 30, c.height - 8);
  requestAnimationFrame(beat);
}
requestAnimationFrame(beat);

// ---- the controls -----------------------------------------------------------------

$("slice").oninput = () => {
  sliceMs = +$("slice").value;
  $("slice-out").textContent = sliceMs + " ms";
  const sent = new Set();
  for (const s of Object.values(boxes)) {
    if (!s.host || (s.kind === "main" && sent.has("main"))) continue;
    sent.add(s.kind);
    s.host.send({ type: "slice", ms: sliceMs });
  }
};
$("start").onclick = () => Object.values(boxes).forEach(start);
$("pause-all").onclick = () => {
  const all = Object.values(boxes);
  const pause = all.some((s) => !s.paused);
  all.forEach((s) => setPaused(s, pause));
  $("pause-all").textContent = pause ? "Go on, all" : "Pause all";
};
$("all-host").onchange = async () => {
  const kind = $("all-host").value;
  statusEl.textContent = "Moving to " + HOST_NAMES[kind] + "…";
  await Promise.all(Object.values(boxes).map((s) => move(s, kind)));
  statusEl.textContent = "Running on " + HOST_NAMES[kind] + ".";
};
$("fetch-all").onclick = fetchAll;

async function fetchAll() {
  const rows = await Promise.all(Object.values(boxes).map(async (s) => ({ s, r: await fetchOne(s) })));
  const table = $("results");
  table.innerHTML =
    "<thead><tr><th>Program</th><th>Host</th><th>Zoom</th><th>Pass</th><th>Max iterations</th><th>Inside</th><th>Mean iterations</th><th>Time</th></tr></thead>";
  const body = document.createElement("tbody");
  const out = [];
  for (const { s, r } of rows) {
    let j = null;
    try {
      j = JSON.parse(r.result);
    } catch (e) {}
    out.push({ program: s.prog.name, host: s.kind, progress: r.progress, result: j });
    const tr = document.createElement("tr");
    const cells = j && j.program
      ? [s.prog.name, HOST_NAMES[s.kind], "×" + (s.prog.view[2] / j.width).toPrecision(3), j.pass + 1, j.maxIter, (100 * j.insideShare).toFixed(2) + " %", j.meanIter.toFixed(1), (j.ms / 1000).toFixed(1) + " s"]
      : [s.prog.name, HOST_NAMES[s.kind], "", "", "", "", "", s.frozen ? "frozen" : "no pass finished yet"];
    for (const c of cells) {
      const td = document.createElement("td");
      td.textContent = String(c);
      tr.append(td);
    }
    body.append(tr);
  }
  table.append(body);
  $("results-panel").hidden = false;
  return out;
}

// ---- start ----------------------------------------------------------------------------

const firstHost = HOST_NAMES[params.get("host")] ? params.get("host") : "main";
$("all-host").value = firstHost;
PROGRAMS.forEach((p, i) => {
  const s = makeCard(p, i);
  s.kind = firstHost;
  start(s);
});
statusEl.textContent = "Running on " + HOST_NAMES[firstHost] + ".";

// for the smoke test and the console
window.sandboxDemo = { boxes, move, freeze, thaw, fetchAll, setView, zoomAbout, fetchOne };
