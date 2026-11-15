// jsc: for-in structure / enumerator cache invalidation (several JSC JIT bugs) jit-shape
function sum(o) { var s = []; for (var k in o) { s.push(k + ':' + o[k]); if (k === 'b') { delete o.c; o.z = 9; } } return s.join(); }
var o;
for (var i = 0; i < 300; i++) { o = { a: 1, b: 2, c: 3 }; sum(o); }
o = { a: 1, b: 2, c: 3 };
print(sum(o));
var p = { a: 1, b: 2, c: 3 };
Object.setPrototypeOf(p, { q: 1, r: 2 });
print(sum(p));
var arr = [1, 2, 3];
print(sum(arr));
