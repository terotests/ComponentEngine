// cerxes: cycles through Error.cause, closures and bound functions dropped mid-use (refcount cycle class) refcount
var e = new Error('a', { cause: null });
e.cause = e;
var f = function () { return e; };
f.self = f;
var b = f.bind(f);
e = null; f = null; gc();
print(b().message, b().cause === b().cause.cause);
var o = {};
o.p = new Proxy(o, { get(t, k) { if (k === 'x') { o = null; gc(); } return Reflect.get(t, k); } });
var p = o.p;
print(typeof p.x, typeof p.p);
var c = { get me() { delete c.me; var t = c; c = null; gc(); return t; } };
print(typeof c.me);
