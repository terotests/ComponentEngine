// cerxes: UAF class of refcounted engines (QuickJS, Hermes handle bugs) adapted to CErXes reference counting refcount
var a = [{ v: 1 }, { v: 2 }, { v: 3 }];
a.forEach(function (x, i) { if (i === 0) { a = null; gc(); } print('each', x.v); });
var b = [3, 1, 2].map(x => ({ v: x }));
print(JSON.stringify(b.sort(function (x, y) { b = null; gc(); return x.v - y.v; })));
var m = new Map([[1, { v: 1 }], [2, { v: 2 }], [3, { v: 3 }]]);
m.forEach(function (v, k) { if (k === 1) { m.clear(); m = null; gc(); } print('map', k, v.v); });
var s = 'abcabc';
var re = /b/g;
print(s.replace(re, function () { re = null; gc(); return 'X'; }));
var o = { get x() { holder.o = null; gc(); return this.y; }, y: 'alive' };
var holder = { o: o };
o = null;
print(holder.o.x);
