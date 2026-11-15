// jsc: species constructors returning exotic objects to map / filter / splice (JSC, V8, Chakra) species
function test(m, ret) {
  var a = [1, 2, 3, 4];
  a.constructor = function () {};
  a.constructor[Symbol.species] = function (n) { return ret(a, n); };
  try { var r = a[m](function (x) { return x; }, 0); print(m, JSON.stringify(r), a.length); } catch (e) { print(m, e.name); }
}
var rets = [
  function (a, n) { a.length = 0; return []; },
  function (a, n) { return Object.freeze([]); },
  function (a, n) { return new Proxy([], { defineProperty() { a.length = 100; return true; } }); },
  function (a, n) { return a; },
  function (a, n) { return new Uint8Array(1); },
];
['map', 'filter', 'slice', 'splice', 'flatMap'].forEach(function (m) { rets.forEach(function (r) { test(m, r); }); });
