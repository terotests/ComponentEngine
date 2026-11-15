// chakra: native int / float / var array conversions while a builtin runs (lokihardt Chakra JIT and runtime bugs) array-kind
var a = [1, 2, 3, 4, 5, 6, 7, 8];
print(String(a.reverse.call(new Proxy(a, { get(t, k) { if (k === '3') t[0] = 1.5; if (k === '5') t[1] = {}; return t[k]; } }))));
var b = [1.5, 2.5, 3.5];
print(String(b.sort(function (x, y) { b[5] = 'x'; return x - y; })), b.length);
var c = [1, 2, 3];
print(c.join({ toString() { c[0] = { toString() { c.length = 0; return 'o'; } }; return ','; } }));
var d = [1, 2, 3, 4];
print(String(d.splice(1, { valueOf() { d[0] = 1.5; d.length = 1; return 2; } }, 'n')), String(d));
var e = [0, 1, 2];
print(String(e.map(function (v, i) { if (i === 0) e[2] = {}; return typeof v; })));
