// qjs: TypedArray.prototype.set / subarray / DataView on a detached or shrunk buffer (QuickJS, JSC, V8) typedarray-detach
var buf = new ArrayBuffer(64, { maxByteLength: 64 });
var t = new Int32Array(buf);
var src = { length: 16, get 3() { buf.resize(8); return 3; } };
try { t.set(src); print(t.length, t[0], t[1]); } catch (e) { print(e.name); }
var buf2 = new ArrayBuffer(32);
var u = new Uint8Array(buf2);
try { u.set({ length: 4, get 0() { buf2.transfer(); return 1; } }); print(u.length); } catch (e) { print(e.name); }
var buf3 = new ArrayBuffer(32, { maxByteLength: 32 });
var dv = new DataView(buf3, 8);
try { print(dv.getInt32({ valueOf() { buf3.resize(4); return 0; } })); } catch (e) { print(e.name); }
var w = new Float64Array(8);
try { print(w.subarray({ valueOf() { w.buffer.transfer(); return 1; } }).length); } catch (e) { print(e.name); }
var buf4 = new ArrayBuffer(16, { maxByteLength: 32 });
var x = new Uint16Array(buf4);
try { print(Atomics.add(x, { valueOf() { buf4.resize(0); return 3; } }, 1)); } catch (e) { print(e.name); }
