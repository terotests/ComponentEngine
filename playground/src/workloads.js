// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The benchmark's scripts. Each prints its answer; every engine's answer is
// compared with the browser's. `SCALE` is replaced by the scale the page
// asks for. The first seven are ComponentEngine's micro benchmark
// (cer/bench/micro.mjs), the rest cover what those leave out.

export const PRELUDE =
  'var print = typeof print === "function" ? print : function () {\n' +
  '  var s = ""; for (var i = 0; i < arguments.length; i++) { if (i) s += " "; s += String(arguments[i]); }\n' +
  "  console.log(s);\n" +
  "};\n";

/** What an engine runs: the prelude, then the script in a function of its
 * own, so a second run in the same engine redeclares nothing. */
export function wrap(code, scale) {
  return PRELUDE + "(function () {\n" + code.replace(/\bSCALE\b/g, String(scale)) + "\n})();\n";
}

export const WORKLOADS = [
  {
    id: "loop",
    name: "loop",
    about: "a counting for-loop: the interpreter's dispatch and number arithmetic",
    code: "var s = 0;\nfor (var i = 0; i < 50000 * SCALE; i++) { s += i; }\nprint(s);",
  },
  {
    id: "fib",
    name: "fib",
    about: "recursive calls: frames, arguments, returns",
    code: "function fib(k) { return k < 2 ? k : fib(k - 1) + fib(k - 2); }\nvar n = 0;\nfor (var r = 0; r < SCALE; r++) n = fib(20);\nprint(n);",
  },
  {
    id: "strcat",
    name: "strcat",
    about: "appending to a string",
    code: 'var s = "";\nfor (var i = 0; i < 20000 * SCALE; i++) { s += "ab"; }\nprint(s.length);',
  },
  {
    id: "array",
    name: "array",
    about: "push and indexed reads",
    code: "var a = [];\nfor (var i = 0; i < 20000 * SCALE; i++) { a.push(i * 2); }\nvar t = 0;\nfor (var j = 0; j < a.length; j++) { t += a[j]; }\nprint(t + a.length);",
  },
  {
    id: "object",
    name: "object",
    about: "string-keyed property writes and for-in",
    code: 'var o = {};\nfor (var i = 0; i < 20000 * SCALE; i++) { o["k" + (i % 50)] = i; }\nvar t = 0;\nfor (var k in o) { t += o[k]; }\nprint(t);',
  },
  {
    id: "method",
    name: "method",
    about: "built-in string methods",
    code: 'var s = "The quick brown fox jumps over the lazy dog";\nvar t = 0;\nfor (var i = 0; i < 20000 * SCALE; i++) { t += s.slice(i % 10, 20).indexOf("o") + s.charCodeAt(i % 40); }\nprint(t);',
  },
  {
    id: "regex",
    name: "regex",
    about: "RegExp exec with groups",
    code: 'var re = /([a-z]+)\\s+(\\d+)/;\nvar t = 0;\nfor (var i = 0; i < 5000 * SCALE; i++) { var m = re.exec("item " + i + " qty 42"); if (m) { t += m[2].length; } }\nprint(t);',
  },
  {
    id: "closures",
    name: "closures",
    about: "creating and calling closures",
    code: "function counter() { var n = 0; return function () { n += 1; return n; }; }\nvar t = 0;\nfor (var i = 0; i < 5000 * SCALE; i++) { var c = counter(); c(); t += c(); }\nprint(t);",
  },
  {
    id: "classes",
    name: "classes",
    about: "ES2015 classes, inheritance, getters (JS-Interpreter is ES5 only)",
    code: "class Shape { constructor(w) { this.w = w; } get area() { return this.w * this.w; } }\nclass Box extends Shape { get area() { return super.area * 2; } }\nlet t = 0;\nfor (let i = 0; i < 5000 * SCALE; i++) { t += new Box(i % 7).area; }\nprint(t);",
  },
  {
    id: "sort",
    name: "sort",
    about: "Array.prototype.sort with a comparator",
    code: "var a = [];\nvar x = 1;\nfor (var i = 0; i < 5000 * SCALE; i++) { x = (x * 1103515245 + 12345) % 2147483648; a.push(x % 100000); }\na.sort(function (p, q) { return p - q; });\nprint(a[0], a[a.length >> 1], a[a.length - 1]);",
  },
  {
    id: "json",
    name: "json",
    about: "JSON.stringify and JSON.parse round trips",
    code: 'var o = { name: "x", list: [1, 2, 3, { deep: true }], n: 3.5 };\nvar t = 0;\nfor (var i = 0; i < 2000 * SCALE; i++) { o.n = i; t += JSON.parse(JSON.stringify(o)).n; }\nprint(t);',
  },
  {
    id: "map",
    name: "Map/Set",
    about: "Map and Set with number keys",
    code: "var m = new Map();\nvar s = new Set();\nfor (var i = 0; i < 10000 * SCALE; i++) { m.set(i % 1000, i); s.add(i % 777); }\nvar t = 0;\nm.forEach(function (v) { t += v; });\nprint(m.size, s.size, t);",
  },
];

export const SAMPLE = `// Anything here runs in every engine you tick; print() or console.log()
// what you want compared.
function primes(limit) {
  var sieve = [], out = [];
  for (var i = 2; i <= limit; i++) {
    if (!sieve[i]) { out.push(i); for (var j = i * i; j <= limit; j += i) sieve[j] = true; }
  }
  return out;
}
var p = primes(20000);
print(p.length, p[p.length - 1]);
`;
