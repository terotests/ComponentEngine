// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The EVG demo page: two editors (TSX, CSS), a view, and the frame loop.
//
//   CErXes (worker.js, WASM)  runs the script: __frame(input) -> element tree
//   tree.js                   the tree as EVGElements
//   EvgGameHost (Ranger->JS)  stylesheet, layout, display list, hit test
//   evg-html.js               the display list painted as <svg>
//
// One frame is in flight at a time: the next is asked for when the last one
// has been painted, on the next animation frame. A script that takes longer
// than LIMIT_MS to answer has its worker ended.

import { RUNTIME } from "./runtime.js";
import { buildTree, countNodes } from "./tree.js";
import { renderDisplayList } from "./vendor/evg-html.js";
import { installCanvasMeasurer } from "./vendor/evg-measure.js";
import BREAKOUT_TSX from "./games/breakout.tsx";
import BREAKOUT_CSS from "./games/breakout.css";
import SCAFFOLD_TSX from "./games/scaffold.tsx";
import SCAFFOLD_CSS from "./games/scaffold.css";

// The examples; `?game=<id>` picks one, and so does the menu.
const GAMES = {
  scaffold: { name: "Scaffold Scramble (climber)", tsx: SCAFFOLD_TSX, css: SCAFFOLD_CSS },
  breakout: { name: "Breakout", tsx: BREAKOUT_TSX, css: BREAKOUT_CSS },
};

const VIEW_W = 640;
const VIEW_H = 480;
const LIMIT_MS = 3000;
const STORE = "evg-demo-v2";

const $ = (id) => document.getElementById(id);
const stage = $("stage");
const status = $("status");
const stats = $("stats");
const consoleEl = $("console");
const tsxEl = $("src-tsx");
const cssEl = $("src-css");
const pauseBtn = $("pause");

// ---- the editors, kept in localStorage per example --------------------------

function storeGet(key) {
  try {
    return localStorage.getItem(key);
  } catch (e) {
    return null;
  }
}
function storeSet(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch (e) {}
}
const picker = $("example");
const asked = new URLSearchParams(location.search).get("game");
let game = GAMES[asked] ? asked : GAMES[storeGet(STORE + ":game")] ? storeGet(STORE + ":game") : "scaffold";
for (const id in GAMES) {
  const o = document.createElement("option");
  o.value = id;
  o.textContent = GAMES[id].name;
  picker.appendChild(o);
}
function loadGame(id) {
  game = id;
  picker.value = id;
  storeSet(STORE + ":game", id);
  let saved = null;
  try {
    saved = JSON.parse(storeGet(STORE + ":" + id) || "null");
  } catch (e) {}
  const ok = saved && typeof saved.tsx === "string" && typeof saved.css === "string";
  tsxEl.value = ok ? saved.tsx : GAMES[id].tsx;
  cssEl.value = ok ? saved.css : GAMES[id].css;
}
function save() {
  storeSet(STORE + ":" + game, JSON.stringify({ tsx: tsxEl.value, css: cssEl.value }));
}
loadGame(game);
picker.addEventListener("change", () => {
  loadGame(picker.value);
  run();
  showTab("game");
});
for (const el of [tsxEl, cssEl]) {
  el.addEventListener("input", save);
  // Tab indents rather than leaving the editor
  el.addEventListener("keydown", (e) => {
    if (e.key === "Tab" && !e.shiftKey && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      el.setRangeText("  ", el.selectionStart, el.selectionEnd, "end");
      save();
    }
  });
}

// ---- tabs -------------------------------------------------------------------

const TABS = ["game", "tsx", "css"];
function showTab(name) {
  for (const t of TABS) {
    $("tab-" + t).setAttribute("aria-selected", String(t === name));
    $("panel-" + t).hidden = t !== name;
  }
  if (name === "game") stage.focus({ preventScroll: true });
}
for (const t of TABS) $("tab-" + t).addEventListener("click", () => showTab(t));

// ---- console ----------------------------------------------------------------

const MAX_LINES = 200;
function log(text, isError = false) {
  if (!text) return;
  for (const line of String(text).split("\n")) {
    const div = document.createElement("div");
    if (isError) div.className = "err";
    div.textContent = line;
    consoleEl.appendChild(div);
  }
  while (consoleEl.childNodes.length > MAX_LINES) consoleEl.removeChild(consoleEl.firstChild);
  consoleEl.scrollTop = consoleEl.scrollHeight;
}

// ---- EVG --------------------------------------------------------------------

const Host = globalThis.EvgGameHost;
installCanvasMeasurer([globalThis.EvgGameModule]);
const evg = new Host();
evg.setViewport(VIEW_W, VIEW_H);

function applyCss() {
  evg.setStylesheet(cssEl.value);
  const n = evg.cssErrorCount();
  for (let i = 0; i < n; i++) log("CSS: " + evg.cssError(i), true);
}

// ---- input ------------------------------------------------------------------

const keys = {};
const pointer = { x: 0, y: 0, down: false, inside: false };
let events = [];

function toView(e) {
  const r = stage.getBoundingClientRect();
  return { x: ((e.clientX - r.left) * VIEW_W) / r.width, y: ((e.clientY - r.top) * VIEW_H) / r.height };
}
const GAME_KEYS = new Set(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", " ", "Enter"]);
stage.addEventListener("keydown", (e) => {
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  if (GAME_KEYS.has(e.key)) e.preventDefault();
  if (!e.repeat) events.push({ type: "keydown", key: e.key });
  keys[e.key] = true;
});
stage.addEventListener("keyup", (e) => {
  delete keys[e.key];
  events.push({ type: "keyup", key: e.key });
});
stage.addEventListener("blur", () => {
  for (const k in keys) delete keys[k];
});
stage.addEventListener("pointermove", (e) => {
  Object.assign(pointer, toView(e), { inside: true });
});
stage.addEventListener("pointerleave", () => {
  pointer.inside = false;
});
stage.addEventListener("pointerdown", (e) => {
  stage.focus({ preventScroll: true });
  const p = toView(e);
  Object.assign(pointer, p, { down: true, inside: true });
  // EVG's hit test, on the tree it laid out last: the topmost element with an id
  events.push({ type: "click", id: evg.idAt(p.x, p.y), x: p.x, y: p.y });
});
window.addEventListener("pointerup", () => {
  pointer.down = false;
});

// ---- the engine and the loop -----------------------------------------------

let worker = null;
let running = false;
let paused = false;
let waiting = false; // a frame asked for, not answered
let deadline = 0;
let lastTime = 0;
let frames = 0;
let fpsStart = 0;
let fps = 0;
let timing = { cx: 0, evg: 0, paint: 0, nodes: 0, cmds: 0 };

function startWorker() {
  if (worker) worker.terminate();
  worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
  worker.onmessage = (ev) => onMessage(ev.data);
  worker.onerror = (e) => {
    log("worker: " + (e.message || e), true);
    stop("Engine failed to start");
  };
}

function stop(message) {
  running = false;
  waiting = false;
  status.textContent = message;
  pauseBtn.disabled = true;
}

function run() {
  save();
  consoleEl.textContent = "";
  applyCss();
  startWorker();
  running = false;
  waiting = true;
  deadline = performance.now() + 15000; // the first load fetches and compiles the WASM
  status.textContent = "Compiling…";
  worker.postMessage({ type: "load", runtime: RUNTIME, source: tsxEl.value });
}

function onMessage(m) {
  waiting = false;
  if (m.output) log(m.output);
  if (m.type === "loaded") {
    if (!m.ok) {
      log(m.error, true);
      stop("The script did not run: see the console");
      return;
    }
    running = true;
    paused = false;
    pauseBtn.disabled = false;
    pauseBtn.textContent = "Pause";
    status.textContent = `Running (compiled in ${m.ms.toFixed(0)} ms)`;
    lastTime = performance.now();
    fpsStart = lastTime;
    frames = 0;
    return;
  }
  if (!m.ok) {
    log(m.error, true);
    stop("Stopped: the script threw");
    return;
  }
  const t0 = performance.now();
  let tree;
  try {
    tree = JSON.parse(m.tree);
    const doc = JSON.parse(evg.render(buildTree(tree, Host)));
    const t1 = performance.now();
    renderDisplayList(stage, doc);
    timing = { cx: m.ms, evg: t1 - t0, paint: performance.now() - t1, nodes: countNodes(tree), cmds: doc.list.cmds.length };
  } catch (e) {
    log("render: " + String((e && e.stack) || e), true);
    stop("Stopped: the view could not be drawn");
    return;
  }
  frames++;
  const now = performance.now();
  if (now - fpsStart >= 500) {
    fps = (frames * 1000) / (now - fpsStart);
    frames = 0;
    fpsStart = now;
  }
  stats.textContent =
    `${fps.toFixed(0)} fps · CErXes ${timing.cx.toFixed(1)} ms · EVG layout ${timing.evg.toFixed(1)} ms · ` +
    `SVG ${timing.paint.toFixed(1)} ms · ${timing.nodes} elements · ${timing.cmds} draw commands`;
  window.__evgDemo = { frames: (window.__evgDemo ? window.__evgDemo.frames : 0) + 1, ...timing };
}

function loop(now) {
  requestAnimationFrame(loop);
  if (waiting) {
    if (now > deadline) {
      worker.terminate();
      worker = null;
      log(`The script did not answer within ${LIMIT_MS / 1000} s (an endless loop?); its engine was stopped.`, true);
      stop("Stopped: no answer");
    }
    return;
  }
  if (!running || paused) return;
  const dt = Math.min(0.1, (now - lastTime) / 1000);
  lastTime = now;
  const arg = JSON.stringify({ w: VIEW_W, h: VIEW_H, dt, time: now / 1000, keys, pointer, events });
  events = [];
  waiting = true;
  deadline = now + LIMIT_MS;
  worker.postMessage({ type: "frame", arg });
}
requestAnimationFrame(loop);

// ---- controls ---------------------------------------------------------------

$("run").addEventListener("click", () => {
  run();
  showTab("game");
});
pauseBtn.addEventListener("click", () => {
  if (!running) return;
  paused = !paused;
  pauseBtn.textContent = paused ? "Resume" : "Pause";
  status.textContent = paused ? "Paused" : "Running";
  lastTime = performance.now();
});
$("reset").addEventListener("click", () => {
  tsxEl.value = GAMES[game].tsx;
  cssEl.value = GAMES[game].css;
  save();
  run();
});
document.addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
    e.preventDefault();
    run();
    showTab("game");
  }
});
// CSS edits restyle the running game without restarting it
cssEl.addEventListener("input", () => {
  if (running) {
    consoleEl.querySelectorAll(".err").forEach((n) => n.remove());
    applyCss();
  }
});

run();
