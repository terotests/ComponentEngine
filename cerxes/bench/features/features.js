// One language or library feature per case, run hard on its own, so an
// engine's slow feature shows as a ratio instead of disappearing into an
// app's total. Plain ES2017 (Hermes, QuickJS, Node and CErXes all run it).
//
//   <engine> features.js            every case
//   ONLY=name <engine> ...           (Node) one case; or edit ONLY below
//
// Each line: name, best time of REPS runs in ms, checksum (must match
// between engines).
var ONLY = (typeof process !== "undefined" && process.env && process.env.ONLY) || "";
var REPS = 3;
var out = typeof print === "function" ? print : function (s) { console.log(s); };
var now = typeof performance !== "undefined" && performance.now ? function () { return performance.now(); } : function () { return Date.now(); };

var cases = [];
function add(name, fn) { cases.push([name, fn]); }

// ---- typed arrays and binary data
add("u8-write-read", function () {
  var a = new Uint8Array(65536), s = 0;
  for (var r = 0; r < 60; r++) {
    for (var i = 0; i < a.length; i++) a[i] = (i + r) & 255;
    for (var i = 0; i < a.length; i++) s = (s + a[i]) | 0;
  }
  return s;
});
add("f64-write-read", function () {
  var a = new Float64Array(16384), s = 0;
  for (var r = 0; r < 120; r++) {
    for (var i = 0; i < a.length; i++) a[i] = i * 0.5 + r;
    for (var i = 0; i < a.length; i++) s += a[i];
  }
  return s;
});
add("i32-array-sum", function () {
  var a = new Int32Array(65536), s = 0;
  for (var i = 0; i < a.length; i++) a[i] = i * 7;
  for (var r = 0; r < 60; r++) for (var i = 0; i < a.length; i++) s = (s + a[i]) | 0;
  return s;
});
add("dataview-u32", function () {
  var dv = new DataView(new ArrayBuffer(4096)), s = 0;
  for (var r = 0; r < 600; r++) {
    for (var i = 0; i < 1024; i++) dv.setUint32(i * 4, i + r, true);
    for (var i = 0; i < 1024; i++) s = (s + dv.getUint32(i * 4, true)) | 0;
  }
  return s;
});
add("u8-alloc-small", function () {
  var s = 0;
  for (var i = 0; i < 100000; i++) { var a = new Uint8Array(32); a[i & 31] = 1; s += a.length; }
  return s;
});
add("u8-subarray", function () {
  var a = new Uint8Array(4096), s = 0;
  for (var i = 0; i < 200000; i++) { var b = a.subarray(i & 1023, (i & 1023) + 16); s += b.length; }
  return s;
});
add("u8-set-copy", function () {
  var a = new Uint8Array(1024), b = new Uint8Array(65536), s = 0;
  for (var i = 0; i < a.length; i++) a[i] = i;
  for (var r = 0; r < 20000; r++) b.set(a, (r * 64) & 32767);
  for (var i = 0; i < b.length; i += 97) s += b[i];
  return s;
});

// ---- arrays
add("array-push-pop", function () {
  var s = 0;
  for (var r = 0; r < 40; r++) {
    var a = [];
    for (var i = 0; i < 50000; i++) a.push(i);
    while (a.length) s += a.pop();
  }
  return s;
});
add("array-index-rw", function () {
  var a = new Array(10000), s = 0;
  for (var i = 0; i < a.length; i++) a[i] = i;
  for (var r = 0; r < 200; r++) for (var i = 0; i < a.length; i++) { a[i] = a[i] + 1; s += a[i]; }
  return s;
});
add("array-for-of", function () {
  var a = [], s = 0;
  for (var i = 0; i < 10000; i++) a.push(i);
  for (var r = 0; r < 200; r++) for (var x of a) s += x;
  return s;
});
add("array-map-filter-reduce", function () {
  var a = [], s = 0;
  for (var i = 0; i < 10000; i++) a.push(i);
  for (var r = 0; r < 60; r++) s += a.map(function (x) { return x * 2; }).filter(function (x) { return x % 3 === 0; }).reduce(function (p, x) { return p + x; }, 0);
  return s;
});
add("array-sort-num", function () {
  var s = 0;
  for (var r = 0; r < 20; r++) {
    var a = [], x = r + 1;
    for (var i = 0; i < 20000; i++) { x = (x * 1103515245 + 12345) & 0x7fffffff; a.push(x); }
    a.sort(function (p, q) { return p - q; });
    s += a[100];
  }
  return s;
});
add("array-join", function () {
  var a = [], n = 0;
  for (var i = 0; i < 1000; i++) a.push("item" + i);
  for (var r = 0; r < 500; r++) n += a.join(",").length;
  return n;
});
add("array-literal-small", function () {
  var s = 0;
  for (var i = 0; i < 300000; i++) { var a = [i, i + 1, i + 2]; s += a[1]; }
  return s;
});

// ---- strings
add("string-concat", function () {
  var n = 0;
  for (var r = 0; r < 40; r++) { var t = ""; for (var i = 0; i < 20000; i++) t += "ab"; n += t.length; }
  return n;
});
add("string-charcodeat", function () {
  var t = ""; for (var i = 0; i < 10000; i++) t += String.fromCharCode(97 + (i % 26));
  var s = 0;
  for (var r = 0; r < 100; r++) for (var i = 0; i < t.length; i++) s = (s + t.charCodeAt(i)) | 0;
  return s;
});
add("string-fromcharcode", function () {
  var n = 0;
  for (var r = 0; r < 30; r++) { var t = ""; for (var i = 0; i < 10000; i++) t += String.fromCharCode(32 + (i % 90)); n += t.length; }
  return n;
});
add("string-slice", function () {
  var t = "the quick brown fox jumps over the lazy dog".repeat(20), n = 0;
  for (var i = 0; i < 300000; i++) n += t.slice(i % 100, (i % 100) + 10).length;
  return n;
});
add("string-indexof", function () {
  var t = "abcdefghij".repeat(100) + "needle", n = 0;
  for (var i = 0; i < 30000; i++) n += t.indexOf("needle") + t.indexOf("j", i % 900);
  return n;
});
add("string-split", function () {
  var t = "a,bb,ccc,dddd,eeeee,ffffff,g,hh,iii,jjjj", n = 0;
  for (var i = 0; i < 60000; i++) n += t.split(",").length;
  return n;
});
add("string-template", function () {
  var n = 0;
  for (var i = 0; i < 150000; i++) n += `id-${i}-${i * 2}`.length;
  return n;
});
add("string-compare-eq", function () {
  var keys = ["alpha", "beta", "gamma", "delta", "epsilon"], n = 0;
  for (var i = 0; i < 500000; i++) { var k = keys[i % 5]; if (k === "gamma") n++; else if (k === "delta") n += 2; }
  return n;
});
add("string-switch", function () {
  var keys = ["div", "span", "p", "a", "img", "ul", "li"], n = 0;
  for (var i = 0; i < 400000; i++) {
    switch (keys[i % 7]) { case "div": n += 1; break; case "span": n += 2; break; case "p": n += 3; break; case "li": n += 4; break; default: n += 5; }
  }
  return n;
});
add("string-tolower-trim", function () {
  var t = "  Hello World ABC  ", n = 0;
  for (var i = 0; i < 150000; i++) n += t.trim().toLowerCase().length;
  return n;
});

// ---- numbers
add("number-tostring", function () {
  var n = 0;
  for (var i = 0; i < 200000; i++) n += String(i).length + (i * 0.25).toString().length;
  return n;
});
add("number-parse", function () {
  var n = 0;
  for (var i = 0; i < 200000; i++) n += parseInt("12345", 10) + parseFloat("3.25") + Number("42");
  return n;
});
add("number-tofixed", function () {
  var n = 0;
  for (var i = 0; i < 150000; i++) n += (i / 7).toFixed(2).length;
  return n;
});
add("math-funcs", function () {
  var s = 0;
  for (var i = 0; i < 400000; i++) s += Math.floor(i / 3) + Math.sqrt(i) + Math.abs(-i) + Math.max(i, 5) + Math.min(i, 5);
  return s;
});
add("int-arith", function () {
  var x = 0, y = 1, z = 2;
  for (var i = 0; i < 2000000; i++) { x = x + y; y = z - x; z = x ^ y; }
  return x + y + z;
});
add("float-arith", function () {
  var x = 0.5, s = 0;
  for (var i = 0; i < 2000000; i++) { x = x * 1.0000001 + 0.25; s += x / 3; }
  return Math.round(s);
});

// ---- objects
add("prop-get-set", function () {
  var o = { a: 1, b: 2, c: 3 }, s = 0;
  for (var i = 0; i < 1000000; i++) { o.a = o.b + o.c; o.b = i & 7; s += o.a; }
  return s;
});
add("object-literal", function () {
  var s = 0;
  for (var i = 0; i < 300000; i++) { var o = { x: i, y: i + 1, z: 0 }; s += o.y; }
  return s;
});
add("class-new-method", function () {
  class P { constructor(x, y) { this.x = x; this.y = y; } len() { return this.x + this.y; } }
  var s = 0;
  for (var i = 0; i < 300000; i++) s += new P(i, 1).len();
  return s;
});
add("closure-call", function () {
  function mk(k) { return function (x) { return x + k; }; }
  var f = mk(3), s = 0;
  for (var i = 0; i < 1000000; i++) s = f(s) & 0xffffff;
  return s;
});
add("dict-string-keys", function () {
  var d = {}, s = 0;
  for (var i = 0; i < 2000; i++) d["k" + i] = i;
  var ks = Object.keys(d);
  for (var r = 0; r < 100; r++) for (var i = 0; i < ks.length; i++) s += d[ks[i]];
  return s;
});
add("dict-add-delete", function () {
  var s = 0;
  for (var r = 0; r < 50; r++) {
    var d = {};
    for (var i = 0; i < 2000; i++) d["k" + i] = i;
    for (var i = 0; i < 2000; i += 2) delete d["k" + i];
    for (var k in d) s += d[k];
  }
  return s;
});
add("for-in", function () {
  var o = { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6 }, s = 0;
  for (var i = 0; i < 100000; i++) for (var k in o) s += o[k];
  return s;
});
add("object-keys-entries", function () {
  var o = { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6 }, s = 0;
  for (var i = 0; i < 60000; i++) { s += Object.keys(o).length; var e = Object.entries(o); s += e[2][1]; }
  return s;
});
add("object-assign-spread", function () {
  var base = { a: 1, b: 2, c: 3 }, s = 0;
  for (var i = 0; i < 100000; i++) { var o = Object.assign({}, base, { d: i }); var p = { ...o, e: 1 }; s += p.d + p.e; }
  return s;
});
add("getter-setter", function () {
  var o = { _v: 0, get v() { return this._v; }, set v(x) { this._v = x; } }, s = 0;
  for (var i = 0; i < 500000; i++) { o.v = i; s += o.v; }
  return s;
});
add("proto-chain-method", function () {
  class A { f() { return 1; } } class B extends A {} class C extends B {}
  var o = new C(), s = 0;
  for (var i = 0; i < 1000000; i++) s += o.f();
  return s;
});
add("polymorphic-prop", function () {
  var objs = [{ x: 1 }, { y: 2, x: 2 }, { z: 3, y: 1, x: 3 }, { w: 0, x: 4 }], s = 0;
  for (var i = 0; i < 1000000; i++) s += objs[i & 3].x;
  return s;
});
add("destructuring", function () {
  var s = 0;
  for (var i = 0; i < 300000; i++) { var { a, b } = { a: i, b: 2 }; var [c, d] = [a, b]; s += c + d; }
  return s;
});
add("rest-args", function () {
  function f(...xs) { return xs.length; }
  function g() { return arguments.length; }
  var s = 0;
  for (var i = 0; i < 300000; i++) s += f(1, 2, 3) + g(1, 2);
  return s;
});

// ---- collections
add("map-string-keys", function () {
  var m = new Map(), s = 0;
  for (var i = 0; i < 5000; i++) m.set("k" + i, i);
  for (var r = 0; r < 60; r++) for (var i = 0; i < 5000; i++) s += m.get("k" + i);
  return s;
});
add("map-int-keys", function () {
  var m = new Map(), s = 0;
  for (var r = 0; r < 30; r++) { for (var i = 0; i < 10000; i++) m.set(i, i + r); for (var i = 0; i < 10000; i++) s += m.get(i); }
  return s;
});
add("map-object-keys", function () {
  var ks = [], m = new Map(), s = 0;
  for (var i = 0; i < 2000; i++) { var k = { i: i }; ks.push(k); m.set(k, i); }
  for (var r = 0; r < 150; r++) for (var i = 0; i < ks.length; i++) s += m.get(ks[i]);
  return s;
});
add("set-add-has", function () {
  var s = 0;
  for (var r = 0; r < 30; r++) { var st = new Set(); for (var i = 0; i < 10000; i++) st.add(i * 3); for (var i = 0; i < 10000; i++) if (st.has(i)) s++; }
  return s;
});
add("map-iterate", function () {
  var m = new Map(), s = 0;
  for (var i = 0; i < 10000; i++) m.set(i, i);
  for (var r = 0; r < 60; r++) for (var [k, v] of m) s += v;
  return s;
});

// ---- JSON, regex
add("json-stringify", function () {
  var o = { name: "x", list: [1, 2, 3, 4, 5], nested: { a: true, b: null, c: "str" } }, n = 0;
  for (var i = 0; i < 40000; i++) { o.list[0] = i; n += JSON.stringify(o).length; }
  return n;
});
add("json-parse", function () {
  var t = JSON.stringify({ name: "x", list: [1, 2, 3, 4, 5], nested: { a: true, b: null, c: "str" } }), n = 0;
  for (var i = 0; i < 40000; i++) n += JSON.parse(t).list.length;
  return n;
});
add("regex-test", function () {
  var re = /^[a-z]+\d{2,4}$/, xs = ["abc123", "abc", "zz9999", "Q12"], n = 0;
  for (var i = 0; i < 200000; i++) if (re.test(xs[i & 3])) n++;
  return n;
});
add("regex-replace", function () {
  var t = "hello world, hello there, hello again", n = 0;
  for (var i = 0; i < 50000; i++) n += t.replace(/hello/g, "bye").length;
  return n;
});
add("regex-exec-groups", function () {
  var re = /(\d+)-(\d+)/, n = 0;
  for (var i = 0; i < 100000; i++) { var m = re.exec("range 12-345 end"); n += m[1].length + m[2].length; }
  return n;
});

// ---- control flow
add("try-catch-throw", function () {
  var n = 0;
  for (var i = 0; i < 50000; i++) { try { if (i & 1) throw new Error("x"); n++; } catch (e) { n += 2; } }
  return n;
});
add("try-no-throw", function () {
  var n = 0;
  for (var i = 0; i < 1000000; i++) { try { n += i & 3; } catch (e) { n = 0; } }
  return n;
});
add("generator", function () {
  function* g(n) { for (var i = 0; i < n; i++) yield i; }
  var s = 0;
  for (var r = 0; r < 30; r++) for (var x of g(10000)) s += x;
  return s;
});
add("typeof-instanceof", function () {
  class A {} var xs = [1, "s", new A(), null, {}], n = 0;
  for (var i = 0; i < 500000; i++) { var x = xs[i % 5]; if (typeof x === "number") n++; if (x instanceof A) n += 2; }
  return n;
});
add("recursion-fib", function () {
  function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
  return fib(25);
});

// ---- more arrays
add("array-indexof-includes", function () {
  var a = []; for (var i = 0; i < 1000; i++) a.push(i);
  var n = 0;
  for (var i = 0; i < 20000; i++) { n += a.indexOf(i % 1000); if (a.includes(i % 1500)) n++; }
  return n;
});
add("array-slice-concat", function () {
  var a = []; for (var i = 0; i < 100; i++) a.push(i);
  var n = 0;
  for (var i = 0; i < 50000; i++) n += a.slice(10, 60).concat([1, 2]).length;
  return n;
});
add("array-splice", function () {
  var n = 0;
  for (var r = 0; r < 2000; r++) { var a = []; for (var i = 0; i < 100; i++) a.push(i); while (a.length > 10) a.splice(a.length >> 1, 3); n += a[5]; }
  return n;
});
add("array-shift-unshift", function () {
  var n = 0;
  for (var r = 0; r < 200; r++) { var a = []; for (var i = 0; i < 1000; i++) a.unshift(i); while (a.length) n += a.shift(); }
  return n;
});
add("array-foreach-find", function () {
  var a = []; for (var i = 0; i < 1000; i++) a.push(i);
  var n = 0;
  for (var r = 0; r < 300; r++) { a.forEach(function (x) { n += x & 1; }); n += a.find(function (x) { return x > 500; }); if (a.some(function (x) { return x === 999; })) n++; }
  return n;
});
add("array-from-fill", function () {
  var n = 0;
  for (var i = 0; i < 20000; i++) { var a = new Array(50).fill(i); var b = Array.from(a); n += b[3] + a.length; }
  return n;
});
add("array-sort-strings", function () {
  var n = 0;
  for (var r = 0; r < 20; r++) { var a = []; for (var i = 0; i < 5000; i++) a.push("s" + ((i * 7919) % 5000)); a.sort(); n += a[100].length; }
  return n;
});
add("array-spread-call", function () {
  function f(a, b, c, d) { return a + b + c + d; }
  var arr = [1, 2, 3, 4], n = 0;
  for (var i = 0; i < 200000; i++) { n += f(...arr); var b = [...arr, i]; n += b.length; }
  return n;
});
add("array-holes", function () {
  var n = 0;
  for (var r = 0; r < 100; r++) { var a = []; for (var i = 0; i < 2000; i += 2) a[i] = i; for (var i = 0; i < a.length; i++) if (a[i] !== undefined) n += a[i]; }
  return n;
});
add("array-numeric-object", function () {
  var o = {}, n = 0;
  for (var i = 0; i < 1000; i++) o[i] = i;
  for (var r = 0; r < 300; r++) for (var i = 0; i < 1000; i++) n += o[i];
  return n;
});

// ---- more strings
add("string-replaceall-pad", function () {
  var n = 0;
  for (var i = 0; i < 60000; i++) n += "a-b-c-d".replaceAll("-", "+").length + String(i).padStart(8, "0").length;
  return n;
});
add("string-startswith-includes", function () {
  var t = "https://example.com/path/to/resource?x=1", n = 0;
  for (var i = 0; i < 300000; i++) { if (t.startsWith("https")) n++; if (t.includes("resource")) n++; if (t.endsWith("=1")) n++; }
  return n;
});
add("string-upper-repeat", function () {
  var n = 0;
  for (var i = 0; i < 100000; i++) n += "hello world".toUpperCase().length + "ab".repeat(5).length;
  return n;
});
add("string-charat-index", function () {
  var t = "abcdefghijklmnopqrstuvwxyz".repeat(10), n = 0;
  for (var r = 0; r < 300; r++) for (var i = 0; i < t.length; i++) if (t[i] === "e" || t.charAt(i) === "q") n++;
  return n;
});
add("string-for-of", function () {
  var t = "abcdefghij".repeat(100), n = 0;
  for (var r = 0; r < 300; r++) for (var ch of t) n += ch.length;
  return n;
});
add("string-match-split-regex", function () {
  var t = "one two  three   four five", n = 0;
  for (var i = 0; i < 50000; i++) { n += t.split(/\s+/).length; var m = t.match(/t\w+/g); n += m.length; }
  return n;
});
add("string-localecompare", function () {
  var a = ["banana", "apple", "cherry", "date"], n = 0;
  for (var i = 0; i < 200000; i++) n += a[i & 3].localeCompare(a[(i + 1) & 3]);
  return n;
});
add("string-number-concat", function () {
  var n = 0;
  for (var i = 0; i < 300000; i++) n += ("x" + i + ":" + (i & 255)).length;
  return n;
});
add("encode-uri", function () {
  var n = 0;
  for (var i = 0; i < 50000; i++) n += encodeURIComponent("a b&c=d/é" + i).length + decodeURIComponent("a%20b%26c").length;
  return n;
});

// ---- objects and functions
add("call-apply-bind", function () {
  function f(a, b) { return this.k + a + b; }
  var o = { k: 1 }, g = f.bind(o, 2), n = 0;
  for (var i = 0; i < 300000; i++) n += f.call(o, 1, 2) + f.apply(o, [1, 2]) + g(3);
  return n;
});
add("define-property-freeze", function () {
  var n = 0;
  for (var i = 0; i < 50000; i++) { var o = {}; Object.defineProperty(o, "x", { value: i, enumerable: true }); Object.freeze(o); n += o.x + Object.getOwnPropertyDescriptor(o, "x").value; }
  return n;
});
add("has-own-in", function () {
  var o = { a: 1, b: 2, c: 3 }, n = 0;
  for (var i = 0; i < 500000; i++) { if (o.hasOwnProperty("b")) n++; if ("c" in o) n++; if (Object.hasOwn(o, "z")) n--; }
  return n;
});
add("private-fields", function () {
  class P { #x = 0; inc() { this.#x++; return this.#x; } }
  var p = new P(), n = 0;
  for (var i = 0; i < 500000; i++) n += p.inc() & 1;
  return n;
});
add("let-closure-loop", function () {
  var n = 0;
  for (var r = 0; r < 300; r++) { var fs = []; for (let i = 0; i < 300; i++) fs.push(function () { return i; }); for (var j = 0; j < fs.length; j++) n += fs[j](); }
  return n;
});
add("optional-nullish", function () {
  var xs = [{ a: { b: 1 } }, { a: null }, {}, null], n = 0;
  for (var i = 0; i < 500000; i++) { var x = xs[i & 3]; n += (x?.a?.b ?? 2); }
  return n;
});
add("symbol-weakmap", function () {
  var s = Symbol("k"), wm = new WeakMap(), objs = [], n = 0;
  for (var i = 0; i < 1000; i++) { var o = {}; o[s] = i; objs.push(o); wm.set(o, i); }
  for (var r = 0; r < 100; r++) for (var i = 0; i < objs.length; i++) n += objs[i][s] + wm.get(objs[i]);
  return n;
});
add("map-delete", function () {
  var n = 0;
  for (var r = 0; r < 30; r++) { var m = new Map(); for (var i = 0; i < 5000; i++) m.set(i, i); for (var i = 0; i < 5000; i += 2) m.delete(i); n += m.size; }
  return n;
});
add("arguments-sloppy", function () {
  function f() { var s = 0; for (var i = 0; i < arguments.length; i++) s += arguments[i]; return s; }
  var n = 0;
  for (var i = 0; i < 200000; i++) n += f(1, 2, 3, 4);
  return n;
});
add("array-destructure-swap", function () {
  var a = 1, b = 2, n = 0;
  for (var i = 0; i < 500000; i++) { [a, b] = [b, a]; n += a; }
  return n;
});

// ---- numbers, BigInt, Date, JSON
add("bigint-arith", function () {
  var x = 1n, n = 0;
  for (var i = 0; i < 100000; i++) { x = (x * 3n + 1n) % 1000000007n; }
  return Number(x);
});
add("date-ops", function () {
  var n = 0;
  for (var i = 0; i < 50000; i++) { var d = new Date(2020, 0, 1 + (i % 365)); n += d.getMonth() + d.getDate() + d.getDay(); }
  return n;
});
add("date-iso", function () {
  var n = 0;
  for (var i = 0; i < 30000; i++) n += new Date(1600000000000 + i * 86400000).toISOString().length;
  return n;
});
add("json-parse-big", function () {
  var a = []; for (var i = 0; i < 2000; i++) a.push({ id: i, name: "item" + i, tags: ["a", "b"], v: i * 0.5 });
  var t = JSON.stringify(a), n = 0;
  for (var r = 0; r < 20; r++) n += JSON.parse(t).length;
  return n;
});
add("json-stringify-big", function () {
  var a = []; for (var i = 0; i < 2000; i++) a.push({ id: i, name: "item" + i, tags: ["a", "b"], v: i * 0.5 });
  var n = 0;
  for (var r = 0; r < 20; r++) n += JSON.stringify(a).length;
  return n;
});
add("number-isinteger-tofixed0", function () {
  var n = 0;
  for (var i = 0; i < 300000; i++) { if (Number.isInteger(i / 2)) n++; if (Number.isFinite(i)) n++; n += Math.trunc(i / 3) & 1; }
  return n;
});
add("intl-number-format", function () {
  if (typeof Intl === "undefined") return -1;
  var f = new Intl.NumberFormat("en-US"), n = 0;
  for (var i = 0; i < 20000; i++) n += f.format(i * 1234.5).length;
  return n;
});
add("tolocalestring", function () {
  var n = 0;
  for (var i = 0; i < 20000; i++) n += (i * 1234.5).toLocaleString("en-US").length;
  return n;
});

for (var c = 0; c < cases.length; c++) {
  var name = cases[c][0];
  if (ONLY && ONLY !== name) continue;
  var best = 1e9, sum;
  for (var r = 0; r < REPS; r++) {
    var t0 = now();
    sum = cases[c][1]();
    var t = now() - t0;
    if (t < best) best = t;
  }
  out(name + " " + best.toFixed(1) + " " + sum);
}

// ---- promises: timed from the first to the last callback
(function () {
  if (ONLY && ONLY !== "promise-then" && ONLY !== "async-await") return;
  var t0 = now(), p = Promise.resolve(0);
  for (var i = 0; i < 100000; i++) p = p.then(function (x) { return x + 1; });
  p.then(function (v) {
    if (!ONLY || ONLY === "promise-then") out("promise-then " + (now() - t0).toFixed(1) + " " + v);
    async function f(x) { return x + 1; }
    async function run() { var s = 0; for (var i = 0; i < 100000; i++) s = await f(s); return s; }
    var t1 = now();
    run().then(function (s) { if (!ONLY || ONLY === "async-await") out("async-await " + (now() - t1).toFixed(1) + " " + s); });
  });
})();
