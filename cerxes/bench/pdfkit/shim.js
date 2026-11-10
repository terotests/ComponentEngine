// SPDX-License-Identifier: AGPL-3.0-or-later
//
// What PDFKit's browser build expects and an engine without a host may not
// have: print / console, performance.now, setTimeout (run at once),
// TextEncoder / TextDecoder (UTF-8), atob / btoa. Only defined when missing.
(function (g) {
  if (typeof g.print !== "function") g.print = function (s) { console.log(s); };
  if (typeof g.console === "undefined") g.console = { log: g.print, warn: g.print, error: g.print };
  if (typeof g.performance === "undefined") g.performance = { now: function () { return Date.now(); } };
  if (typeof g.setTimeout === "undefined") { g.setTimeout = function (f) { f(); return 0; }; g.clearTimeout = function () {}; }
  if (typeof g.TextEncoder === "undefined") {
    g.TextEncoder = function () {};
    g.TextEncoder.prototype.encode = function (str) {
      var out = [], i = 0;
      for (; i < str.length; i++) {
        var c = str.charCodeAt(i);
        if (c >= 0xd800 && c < 0xdc00 && i + 1 < str.length) { c = 0x10000 + ((c - 0xd800) << 10) + (str.charCodeAt(++i) - 0xdc00); }
        if (c < 0x80) out.push(c);
        else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
        else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
        else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      }
      return new Uint8Array(out);
    };
  }
  if (typeof g.TextDecoder === "undefined") {
    g.TextDecoder = function () {};
    g.TextDecoder.prototype.decode = function (b) {
      if (!b) return "";
      var s = "", i = 0;
      while (i < b.length) {
        var c = b[i++];
        if (c >= 0xf0) { c = ((c & 7) << 18) | ((b[i++] & 63) << 12) | ((b[i++] & 63) << 6) | (b[i++] & 63); c -= 0x10000; s += String.fromCharCode(0xd800 + (c >> 10), 0xdc00 + (c & 1023)); continue; }
        if (c >= 0xe0) c = ((c & 15) << 12) | ((b[i++] & 63) << 6) | (b[i++] & 63);
        else if (c >= 0xc0) c = ((c & 31) << 6) | (b[i++] & 63);
        s += String.fromCharCode(c);
      }
      return s;
    };
  }
  if (typeof g.atob === "undefined") {
    var B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    g.atob = function (a) {
      a = String(a).replace(/[^A-Za-z0-9+\/]/g, "");
      var s = "", bits = 0, n = 0;
      for (var i = 0; i < a.length; i++) {
        bits = (bits << 6) | B64.indexOf(a.charAt(i)); n += 6;
        if (n >= 8) { n -= 8; s += String.fromCharCode((bits >> n) & 255); }
      }
      return s;
    };
    g.btoa = function (s) {
      var o = "", i = 0;
      for (; i < s.length; i += 3) {
        var a = s.charCodeAt(i), b = s.charCodeAt(i + 1), c = s.charCodeAt(i + 2);
        o += B64.charAt(a >> 2) + B64.charAt(((a & 3) << 4) | (b >> 4)) + (i + 1 < s.length ? B64.charAt(((b & 15) << 2) | (c >> 6)) : "=") + (i + 2 < s.length ? B64.charAt(c & 63) : "=");
      }
      return o;
    };
  }
})(typeof globalThis !== "undefined" ? globalThis : this);
