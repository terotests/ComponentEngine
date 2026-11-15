// jsc: TypedArray.prototype.sort comparator detaches the buffer (JSC, QuickJS, V8 had this) typedarray-detach
var ta = new Float64Array(16);
for (var i = 0; i < 16; i++) ta[i] = 16 - i;
try { ta.sort(function (x, y) { if (ta.buffer.transfer) ta.buffer.transfer(); return x - y; }); print('sorted', ta.length); } catch (e) { print(e.name); }
var rab = new ArrayBuffer(16, { maxByteLength: 64 });
var u = new Uint32Array(rab);
u.set([5, 4, 3, 2]);
try { u.sort(function (x, y) { rab.resize(4); return x - y; }); print(u.length, String(u)); } catch (e) { print(e.name); }
var v = new Uint8Array(new ArrayBuffer(8, { maxByteLength: 8 }));
try { print(v.toSorted(function (x, y) { v.buffer.resize(0); return y - x; }).length); } catch (e) { print(e.name); }
