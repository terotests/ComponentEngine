// v8: Array.prototype.sort comparator shrinks / grows / re-protos the array (V8, JSC, QuickJS) array-reentrancy
var a = []; for (var i = 0; i < 50; i++) a.push(50 - i);
var n = 0;
a.sort(function (x, y) { if (++n === 10) { a.length = 3; Object.setPrototypeOf(a, { 40: 'p' }); } if (n === 20) a.push(...new Array(100).fill(0)); return x - y; });
print(a.length, a[0], a[2]);
var b = [3, 2, 1];
print(String(b.toSorted(function (x, y) { b.length = 0; return x - y; })), b.length);
var holes = [5, , 3, , 1];
Object.defineProperty(Array.prototype, 1, { get() { holes.length = 0; return 0; }, configurable: true });
print(String(holes.sort()));
delete Array.prototype[1];
