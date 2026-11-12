// SPDX-License-Identifier: AGPL-3.0-or-later
// What every case runs through (libs.mjs bundles case + runtime + shim).
// A case is `export default { check, bench }`:
//   check: { name: () => value }   run once; the value is printed in a
//          canonical form (ser) and compared between engines
//   bench: { name: () => value }   timed: one warm-up call, then calls until
//          BUDGET ms have passed (at least 1, at most 10000)
// Either may return a promise. Output lines (tab-separated):
//   L <ms>                  library load (module init) after the banner
//   C <name> <value>        a check (ERR <message> when it threw)
//   B <name> <ms/call> <calls>
//   D <ms>                  done, time since the banner
const out = (s) => console.log(s);
const now = () => (typeof performance === "object" && performance && performance.now ? performance.now() : Date.now());

/** Canonical text of a value: types are kept apart (1 vs "1"), -0 and NaN
 * written, Map/Set/typed arrays/Date/RegExp/Error by content, cycles marked,
 * functions by name. Object keys keep their order (that order is part of
 * what an engine must get right). */
export function ser(v, seen = []) {
  switch (typeof v) {
    case "undefined": return "undefined";
    case "number": return Object.is(v, -0) ? "-0" : String(v);
    case "bigint": return v + "n";
    case "string": return JSON.stringify(v);
    case "boolean": return String(v);
    case "symbol": return "Symbol(" + (v.description ?? "") + ")";
    case "function": return "fn " + (v.name || "anon");
  }
  if (v === null) return "null";
  if (seen.includes(v)) return "<cycle>";
  seen.push(v);
  try {
    if (Array.isArray(v)) {
      const a = [];
      for (let i = 0; i < v.length; i++) a.push(i in v ? ser(v[i], seen) : "<hole>");
      return "[" + a.join(",") + "]";
    }
    if (ArrayBuffer.isView(v)) return Object.prototype.toString.call(v).slice(8, -1) + "[" + Array.from(v, (x) => ser(x)).join(",") + "]";
    if (v instanceof ArrayBuffer) return "ArrayBuffer[" + Array.from(new Uint8Array(v)).join(",") + "]";
    if (v instanceof Map) return "Map{" + [...v].map(([k, x]) => ser(k, seen) + "=>" + ser(x, seen)).join(",") + "}";
    if (v instanceof Set) return "Set{" + [...v].map((x) => ser(x, seen)).join(",") + "}";
    if (v instanceof Date) return "Date(" + (isNaN(v) ? "Invalid" : v.toISOString()) + ")";
    if (v instanceof RegExp) return String(v);
    if (v instanceof Error) return v.name + "(" + JSON.stringify(v.message) + ")";
    if (typeof v.then === "function") return "<thenable>";
    const ks = Object.keys(v);
    return "{" + ks.map((k) => JSON.stringify(k) + ":" + ser(v[k], seen)).join(",") + "}";
  } finally {
    seen.pop();
  }
}

/** A short hash of a long string (FNV-1a 32), so big outputs stay one line. */
export function hash(s) {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193);
  return (h >>> 0).toString(16).padStart(8, "0") + ":" + s.length;
}

const isP = (x) => x && typeof x.then === "function";

/** Microtask hops between two virtual timers: a timer runs only when the
 * queue has been idle this long (async code settles in far fewer). */
const HOPS = 64;
let driving = false;
function drive() {
  if (driving) return;
  driving = true;
  let idle = 0;
  const step = () => {
    if (idle++ < HOPS) return Promise.resolve().then(step);
    idle = 0;
    if (!__vt.length) { driving = false; return; }
    let k = 0;
    for (let i = 1; i < __vt.length; i++) if (__vt[i].at < __vt[k].at) k = i;
    const t = __vt.splice(k, 1)[0];
    if (t.at > __vNow) __vNow = t.at;
    t.fn.apply(null, t.a);
    return Promise.resolve().then(step);
  };
  Promise.resolve().then(step);
}

/** Awaits a value, keeping timers running meanwhile. */
async function settle(v) {
  if (!isP(v)) return v;
  drive();
  return await v;
}

export async function run(c) {
  out("L\t" + (now() - __t0).toFixed(2));
  const BUDGET = globalThis.__BUDGET ?? 400;
  for (const [name, f] of Object.entries(c.check || {})) {
    let line;
    try {
      const v = await settle(f());
      line = ser(v);
      if (line.length > 2000) line = "#" + hash(line);
    } catch (e) {
      line = "ERR " + String(e && e.message ? (e.name || "Error") + ": " + e.message : e).split("\n")[0].slice(0, 300);
    }
    out("C\t" + name + "\t" + line.replace(/[\t\n]/g, " "));
  }
  for (const [name, f] of Object.entries(c.bench || {})) {
    try {
      await settle(f());
      let n = 0, t = now(), dt = 0;
      do {
        await settle(f());
        n++;
        dt = now() - t;
      } while (dt < BUDGET && n < 10000);
      out("B\t" + name + "\t" + (dt / n).toFixed(4) + "\t" + n);
    } catch (e) {
      out("B\t" + name + "\tERR\t" + String(e && e.message ? e.message : e).split("\n")[0].slice(0, 200));
    }
  }
  out("D\t" + (now() - __t0).toFixed(1));
}
