// cerxes: Proxy revoked from inside its own trap (Proxy UAF class: V8, JSC, QuickJS) refcount
var r = Proxy.revocable({ a: 1, b: 2 }, { get(t, k) { r.revoke(); r = null; gc(); return t[k]; }, ownKeys(t) { r && r.revoke(); return Reflect.ownKeys(t); } });
var p = r.proxy;
print(p.a);
try { print(p.b); } catch (e) { print(e.name); }
var r2 = Proxy.revocable([1, 2, 3], { get(t, k) { if (k === 'length') { r2.revoke(); gc(); } return t[k]; } });
try { print(Array.prototype.join.call(r2.proxy)); } catch (e) { print(e.name); }
var r3 = Proxy.revocable({}, { ownKeys(t) { r3.revoke(); gc(); return ['a', 'b']; }, getOwnPropertyDescriptor() { return { value: 1, enumerable: true, configurable: true }; } });
try { print(Object.keys(r3.proxy)); } catch (e) { print(e.name); }
