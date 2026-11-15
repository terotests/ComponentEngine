// qjs: with + Symbol.unscopables getter that deletes the binding (QuickJS / JSC scope bugs) scope
var o = { x: 1, y: 2 };
var x = 'global';
Object.defineProperty(o, Symbol.unscopables, { get() { delete o.x; return { y: true }; } });
with (o) { print(x, y); x = 5; }
print(x, o.x);
var hits = 0;
var p = new Proxy({ z: 1 }, { has(t, k) { hits++; if (k === 'z') delete t.z; return k in t; } });
with (p) { try { print(typeof z, z); } catch (e) { print(e.name); } }
print(hits);
function f() { with ({}) { return eval('var q = 3; q'); } }
print(f(), typeof q);
