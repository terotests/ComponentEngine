// SPDX-License-Identifier: AGPL-3.0-or-later
// Banner of every libs.mjs bundle: the same host on every engine. A CLI
// host has no event loop and no Web APIs, so both engines get these
// (only where the global is missing, so an engine's own wins):
// virtual timers, queueMicrotask, a seeded Math.random, TextEncoder /
// TextDecoder (UTF-8), crypto.getRandomValues (seeded), AbortController. Timers run in due order from the microtask queue
// (__drive in runtime.js).
var __t0 = typeof performance === "object" && performance && performance.now ? performance.now() : Date.now();
var __vt = [], __vtId = 1, __vNow = 0;
globalThis.setTimeout = function (fn, ms) {
  var a = Array.prototype.slice.call(arguments, 2);
  var id = __vtId++;
  __vt.push({ id: id, at: __vNow + (+ms || 0), fn: fn, a: a });
  return id;
};
globalThis.clearTimeout = function (id) {
  for (var i = 0; i < __vt.length; i++) if (__vt[i].id === id) { __vt.splice(i, 1); return; }
};
// an interval runs once more each time it fires
globalThis.setInterval = function (fn, ms) {
  var a = Array.prototype.slice.call(arguments, 2), box = { id: 0 };
  function again() { box.id = setTimeout(again, ms); fn.apply(null, a); }
  box.id = setTimeout(again, ms);
  return box;
};
globalThis.clearInterval = function (b) { if (b && typeof b === "object") clearTimeout(b.id); else clearTimeout(b); };
globalThis.setImmediate = function (fn) { return setTimeout(fn, 0); };
if (typeof queueMicrotask !== "function") globalThis.queueMicrotask = function (fn) { Promise.resolve().then(fn); };
(function () {
  var s = 0x9e3779b9;
  Math.random = function () {
    s |= 0; s = (s + 0x6d2b79f5) | 0;
    var t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
})();
if (typeof TextEncoder !== "function") {
  globalThis.TextEncoder = function TextEncoder() {};
  TextEncoder.prototype.encoding = "utf-8";
  TextEncoder.prototype.encode = function (str) {
    str = str === undefined ? "" : String(str);
    var out = [];
    for (var i = 0; i < str.length; i++) {
      var c = str.charCodeAt(i);
      if (c >= 0xd800 && c < 0xdc00 && i + 1 < str.length) {
        var d = str.charCodeAt(i + 1);
        if (d >= 0xdc00 && d < 0xe000) { c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00); i++; } else c = 0xfffd;
      } else if (c >= 0xd800 && c < 0xe000) c = 0xfffd;
      if (c < 0x80) out.push(c);
      else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
      else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    }
    return new Uint8Array(out);
  };
  TextEncoder.prototype.encodeInto = function (str, dest) {
    var b = this.encode(str), n = Math.min(b.length, dest.length);
    dest.set(b.subarray(0, n));
    return { read: str.length, written: n };
  };
}
if (typeof TextDecoder !== "function") {
  globalThis.TextDecoder = function TextDecoder() {};
  TextDecoder.prototype.encoding = "utf-8";
  TextDecoder.prototype.decode = function (buf) {
    if (buf === undefined) return "";
    var b = buf instanceof Uint8Array ? buf : ArrayBuffer.isView(buf) ? new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength) : new Uint8Array(buf);
    var s = "", parts = [];
    for (var i = 0; i < b.length; ) {
      var c = b[i++], cp;
      if (c < 0x80) cp = c;
      else if (c >= 0xc0 && c < 0xe0) cp = ((c & 31) << 6) | (b[i++] & 63);
      else if (c >= 0xe0 && c < 0xf0) { cp = ((c & 15) << 12) | ((b[i] & 63) << 6) | (b[i + 1] & 63); i += 2; }
      else if (c >= 0xf0) { cp = ((c & 7) << 18) | ((b[i] & 63) << 12) | ((b[i + 1] & 63) << 6) | (b[i + 2] & 63); i += 3; }
      else cp = 0xfffd;
      s += String.fromCodePoint(cp);
      if (s.length > 4096) { parts.push(s); s = ""; }
    }
    parts.push(s);
    return parts.join("");
  };
}
if (typeof crypto === "undefined") globalThis.crypto = { getRandomValues: function (a) { for (var i = 0; i < a.length; i++) a[i] = Math.floor(Math.random() * 4294967296); return a; } };
if (typeof AbortController !== "function") {
  globalThis.AbortSignal = function AbortSignal() { this.aborted = false; this.reason = undefined; this.onabort = null; this._l = []; };
  AbortSignal.prototype.addEventListener = function (t, f) { if (t === "abort") this._l.push(f); };
  AbortSignal.prototype.removeEventListener = function (t, f) { this._l = this._l.filter(function (g) { return g !== f; }); };
  AbortSignal.prototype.throwIfAborted = function () { if (this.aborted) throw this.reason; };
  globalThis.AbortController = function AbortController() { this.signal = new AbortSignal(); };
  AbortController.prototype.abort = function (reason) {
    var s = this.signal;
    if (s.aborted) return;
    s.aborted = true;
    s.reason = reason === undefined ? new Error("This operation was aborted") : reason;
    var ev = { type: "abort", target: s };
    if (s.onabort) s.onabort(ev);
    s._l.slice().forEach(function (f) { f(ev); });
  };
}
