// cerxes: re-entrant builtins suspended and resumed across slices (snapshot oracle: CErXes travelling engine) snapshot
var a = []; for (var i = 0; i < 2000; i++) a.push(i);
var calls = 0;
var r = a.reduce(function (s, x) { calls++; if (calls === 500) a.length = 1000; return s + x; }, 0);
print(r, calls);
var m = new Map(); for (var j = 0; j < 500; j++) m.set(j, j);
var seen = 0;
for (var [k] of m) { seen++; if (k % 100 === 0) m.delete(k + 1); }
print(seen);
var g = (function* () { for (var q = 0; q < 1000; q++) yield q; })();
var t = 0; for (var v of g) t += v;
print(t);
