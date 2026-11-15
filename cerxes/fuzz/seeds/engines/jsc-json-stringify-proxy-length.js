// jsc: JSON.stringify of a Proxy array whose length changes (JSC / V8 stringify bugs) proxy
var a = [1, 2, 3];
var n = 0;
var p = new Proxy(a, { get(t, k, r) { if (k === 'length') return ++n > 1 ? 1e6 : 3; return Reflect.get(t, k, r); } });
try { print(JSON.stringify(p).length); } catch (e) { print(e.name); }
var q = new Proxy([1, [2, 3]], { get(t, k) { if (k === '1') { t.length = 0; } return t[k]; } });
print(JSON.stringify(q));
print(JSON.stringify({ a: 1, b: 2 }, new Proxy(['a', 'b'], { get(t, k) { if (k === 'length') return 2; return t[k]; } })));
