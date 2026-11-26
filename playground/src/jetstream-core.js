// SPDX-License-Identifier: AGPL-3.0-or-later
//
// JetStream 3 in an engine that is only a script runner: what JetStream's
// shell driver (JetStreamDriver.js, ShellScripts) does around a benchmark,
// as one script run in a fresh realm, made of these parts in order:
//
//   1. the prelude: the JetStream object (preload names, getString,
//      getBinary), performance.mark/measure, the deterministic Math.random,
//      console.assert, and a setTimeout made of promise jobs where the engine
//      has none
//   2. one script per preload, putting the file's text (or bytes, as
//      base64) where getString/getBinary find it
//   3. the benchmark's files, in JetStream's order
//   4. the runner: JetStream's runnerCode, printing the iteration times as
//      "JS3:[…]" instead of resolving the page's promise
//
// JetStream loads the parts as separate scripts of one global. They are one
// script here because the browser's engine runs a script through indirect
// eval, where a top-level class or let stays inside that eval (the
// workloads' `class Benchmark` would not reach the runner). The parts are
// joined on separate lines with `;`; a file's "use strict" is then no longer
// a directive, so every part runs sloppy.
//
// Scores as JetStream computes them: 5000 / ms per subscore (First, the
// mean of the Worst N after the first, the Average after the first; WSL:
// Stdlib, MainRun), a benchmark's score the geometric mean of its
// subscores, a group's subscores the geometric mean over its members, and
// the total the geometric mean of the benchmarks' scores.
//
// Used by the page (jetstream.js) and by jetstream/check.mjs.

export const toScore = (ms) => 5000 / Math.max(ms, 1);
export const geomean = (xs) => (xs.length ? Math.exp(xs.reduce((a, b) => a + Math.log(b), 0) / xs.length) : null);
const mean = (xs) => xs.reduce((a, b) => a + b, 0) / xs.length;

/** Iterations and worst case count for a cap (0: JetStream's own). */
export function plan(b, cap) {
  let n = b.iterations;
  let worst = b.worstCaseCount;
  if (cap && n > cap) {
    n = cap;
    if (worst) worst = Math.max(1, Math.min(worst, Math.floor((n - 1) / 4)));
  }
  if (worst && worst >= n) worst = Math.max(0, n - 2);
  return { iterations: n, worst };
}

const RANDOM = `(() => {
  const initialSeed = 49734321;
  let seed = initialSeed;
  Math.random = () => {
    seed = ((seed + 0x7ed55d16) + (seed << 12))  & 0xffff_ffff;
    seed = ((seed ^ 0xc761c23c) ^ (seed >>> 19)) & 0xffff_ffff;
    seed = ((seed + 0x165667b1) + (seed << 5))   & 0xffff_ffff;
    seed = ((seed + 0xd3a2646c) ^ (seed << 9))   & 0xffff_ffff;
    seed = ((seed + 0xfd7046c5) + (seed << 3))   & 0xffff_ffff;
    seed = ((seed ^ 0xb55a4f09) ^ (seed >>> 16)) & 0xffff_ffff;
    return (seed >>> 0) / 0x1_0000_0000;
  };
  Math.random.__resetSeed = () => { seed = initialSeed; };
})();`;

export function prelude(b, { iterations }) {
  const pre = Object.entries(b.preload || {});
  const preload = pre.map(([k, v]) => `${JSON.stringify(k)}: ${JSON.stringify(v)}`).join(", ");
  const resources = pre.map(([, v]) => `${JSON.stringify(v)}: ${JSON.stringify(v)}`).join(", ");
  return `
if (typeof self === "undefined") globalThis.self = globalThis;
var __js3Out = typeof print === "function" ? print : function () { console.log.apply(console, arguments); };
if (typeof console.assert !== "function")
  console.assert = function (c) { if (!c) __js3Out("Assertion failed: " + Array.prototype.slice.call(arguments, 1).join(" ")); };
if (typeof setTimeout === "undefined") {
  globalThis.setTimeout = function (f) { var a = Array.prototype.slice.call(arguments, 2); Promise.resolve().then(function () { f.apply(null, a); }); return 0; };
  globalThis.clearTimeout = function () {};
}
globalThis.top = {
  currentResolve: function (r) { __js3Out("JS3:" + JSON.stringify(r)); },
  currentReject: function (e) { __js3Out("JS3-ERROR:" + (e && e.stack ? e.stack : e)); },
};
globalThis.__js3Data = Object.create(null);
globalThis.__js3B64 = function (s) {
  var T = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/", v = new Int8Array(128);
  for (var i = 0; i < 64; i++) v[T.charCodeAt(i)] = i;
  var pad = s.endsWith("==") ? 2 : s.endsWith("=") ? 1 : 0, n = (s.length / 4) * 3 - pad, out = new Int8Array(n), j = 0;
  for (var i = 0; i < s.length; i += 4) {
    var x = (v[s.charCodeAt(i)] << 18) | (v[s.charCodeAt(i + 1)] << 12) | (v[s.charCodeAt(i + 2)] << 6) | v[s.charCodeAt(i + 3)];
    if (j < n) out[j++] = x >> 16; if (j < n) out[j++] = x >> 8; if (j < n) out[j++] = x;
  }
  return out;
};
globalThis.JetStream = {
  preload: { ${preload} },
  resources: { ${resources} },
  isInBrowser: false,
  isD8: false,
  getString: async function (p) { var s = __js3Data[p]; if (typeof s !== "string") throw new Error("JetStream: no text for " + p); return s; },
  getBinary: async function (p) { var s = __js3Data[p]; if (!(s instanceof Int8Array)) throw new Error("JetStream: no bytes for " + p); return s; },
  dynamicImport: async function (p) { throw new Error("JetStream.dynamicImport is not supported here: " + p); },
};
if (typeof performance === "undefined") globalThis.performance = { now: function () { return Date.now(); } };
performance.mark ??= function (name) { return { name }; };
performance.measure ??= function () {};
performance.timeOrigin ??= performance.now();
${b.deterministicRandom ? RANDOM : ""}
void ${iterations};
`;
}

const args = (b, iterations) => JSON.stringify({ ...(b.args || {}), iterationCount: iterations });

export function runner(b, { iterations }) {
  const reset = b.deterministicRandom ? "Math.random.__resetSeed();" : "";
  if (b.kind === "WSL")
    return `{
  const benchmark = new Benchmark(${args(b, iterations)});
  const results = [];
  let start = performance.now();
  benchmark.buildStdlib();
  results.push(performance.now() - start);
  start = performance.now();
  benchmark.run();
  results.push(performance.now() - start);
  top.currentResolve(results);
}`;
  if (b.kind === "Async")
    return `(async function () {
  const benchmark = new Benchmark(${args(b, iterations)});
  await benchmark.init?.();
  const results = [];
  for (let i = 0; i < ${iterations}; i++) {
    await benchmark.prepareForNextIteration?.();${reset}
    const start = performance.now();
    await benchmark.runIteration(i);
    const end = performance.now();
    results.push(Math.max(1, end - start));
  }
  benchmark.validate?.(${iterations});
  top.currentResolve(results);
})().catch(function (e) { top.currentReject(e); });`;
  return `{
  const benchmark = new Benchmark(${args(b, iterations)});
  const results = [];
  for (let i = 0; i < ${iterations}; i++) {
    benchmark.prepareForNextIteration?.();${reset}
    const start = performance.now();
    benchmark.runIteration(i);
    const end = performance.now();
    results.push(Math.max(1, end - start));
  }
  benchmark.validate?.(${iterations});
  top.currentResolve(results);
}`;
}

function base64(bytes) {
  const T = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let s = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i], b = bytes[i + 1], c = bytes[i + 2];
    const x = (a << 16) | ((b || 0) << 8) | (c || 0);
    s += T[x >> 18] + T[(x >> 12) & 63] + (b === undefined ? "=" : T[(x >> 6) & 63]) + (c === undefined ? "=" : T[x & 63]);
  }
  return s;
}

/** The script of one benchmark (not a group): { src, async }.
 * `load(path, binary)` gives a file's text, or its bytes (Uint8Array) when
 * binary. async: the answer of an Async benchmark comes from promise jobs. */
export async function script(b, cap, load) {
  const parts = await parts_(b, cap, load);
  const src = parts.map((p) => p.src).join("\n;\n");
  return { src, async: b.kind === "Async" };
}

async function parts_(b, cap, load) {
  const p = plan(b, cap);
  const out = [{ label: "prelude", src: prelude(b, p) }];
  const bin = new Set(b.binary || []);
  const seen = new Set();
  for (const f of Object.values(b.preload || {})) {
    if (seen.has(f)) continue;
    seen.add(f);
    const data = await load(f, bin.has(f));
    const value = bin.has(f) ? `__js3B64(${JSON.stringify(base64(data))})` : JSON.stringify(data);
    out.push({ label: "preload " + f, src: `__js3Data[${JSON.stringify(f)}] = ${value};` });
  }
  for (const f of b.files) out.push({ label: f, src: await load(f, false) });
  out.push({ label: "runner", src: runner(b, p), async: b.kind === "Async" });
  return out;
}

/** "JS3:[…]" or "JS3-ERROR:…" in an engine's output. */
export function parse(lines) {
  for (const l of lines || []) {
    if (l.startsWith("JS3:")) return { times: JSON.parse(l.slice(4)) };
    if (l.startsWith("JS3-ERROR:")) return { error: l.slice(10) };
  }
  return null;
}

/** Subscores of one benchmark's iteration times, as JetStream's
 * DefaultBenchmark / WSLBenchmark processResults. */
export function subScores(b, times, cap) {
  if (b.kind === "WSL") return { Stdlib: toScore(times[0]), MainRun: toScore(times[1]) };
  const { iterations, worst } = plan(b, cap);
  const s = { First: toScore(times[0]) };
  const rest = times.slice(1).sort((x, y) => y - x);
  if (worst && rest.length) s.Worst = toScore(mean(rest.slice(0, worst)));
  if (iterations > 1 && rest.length) s.Average = toScore(mean(rest));
  return s;
}

/** A group's subscores from its members' (each a subScores object). */
export function groupScores(members) {
  const acc = {};
  for (const m of members) for (const [k, v] of Object.entries(m)) (acc[k] ||= []).push(v);
  return Object.fromEntries(Object.entries(acc).map(([k, v]) => [k, geomean(v)]));
}

export const scoreOf = (subs) => geomean(Object.values(subs));
