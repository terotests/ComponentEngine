// qjs: fast-array paths that keep a pointer while valueOf runs (QuickJS bug tracker, lastIndexOf / fill / copyWithin / with) array-reentrancy
var a = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
print(a.lastIndexOf(3, { valueOf() { a.length = 0; return 9; } }));
var b = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
print(b.includes(undefined, { valueOf() { b.length = 1; return 0; } }));
var c = new Array(20).fill(1.5);
try { print(c.with({ valueOf() { c.length = 0; return 5; } }, 9).length); } catch (e) { print(e.name); }
var d = [1, 2, 3, 4, 5];
print(String(d.copyWithin(0, { valueOf() { d.length = 2; return 3; } })));
var e = [1, 2, 3, 4, 5];
print(String(e.fill(9, { valueOf() { e.length = 100; return 2; } }, 4)), e.length);
var t = new Uint8Array(16);
try { print(t.lastIndexOf(0, { valueOf() { t.buffer.transfer(); return 15; } })); } catch (x) { print(x.name); }
try { print(t.fill(1, { valueOf() { return 0; } }).length); } catch (x) { print(x.name); }
