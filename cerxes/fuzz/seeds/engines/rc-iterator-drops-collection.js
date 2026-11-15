// cerxes: iterators whose collection or generator is dropped and collected while they run (UAF class) refcount
function mk() { var s = new Set([1, 2, 3, 4]); return s.values(); }
var it = mk();
gc();
print(String([...it]));
var m = new Map([[1, 'a'], [2, 'b']]);
var mi = m.entries();
m.clear(); m = null; gc();
print(JSON.stringify(mi.next()));
var arr = [1, 2, 3];
var ai = arr[Symbol.iterator]();
ai.next(); arr.length = 0; arr = null; gc();
print(JSON.stringify(ai.next()));
function* g() { var big = new Array(100).fill({ k: 1 }); yield big.length; g2 = null; gc(); yield big[99].k; }
var g2 = g();
var gr = g2;
print(gr.next().value, gr.next().value);
var w = new WeakMap(); (function () { var k = {}; w.set(k, { v: 1 }); })(); gc(); print(w.has({}));
