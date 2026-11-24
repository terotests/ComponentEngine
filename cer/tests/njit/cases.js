// Loops the native JIT (src/njit.rs) compiles, with answers Node gives:
// tests/njit/run.sh runs them with the JIT on and off.
function sum(n) { let s = 0; for (let i = 0; i < n; i++) s += i; return s; }
function mix(n) { let s = 0.5; for (let i = n; i > 0; i--) { s = s * 1.0001 - i / 3; } return s; }
function cmps(n) {
  let a = 0, b = 0, c = 0, d = 0, e = 0, f = 0;
  for (let i = 0; i <= n; i++) {
    if (i < 10) a++;
    if (i <= 10) b++;
    if (i > n - 5) c++;
    if (i >= n - 5) d++;
    if (i === 7) e++;
    if (i !== 7) f++;
  }
  return [a, b, c, d, e, f].join(",");
}
function nan(n) { let x = NaN, k = 0; for (let i = 0; i < n; i++) { if (x < i) k++; if (x === x) k += 10; if (x !== x) k += 100; } return k; }
function truthy(n) {
  const vals = [0, 1, -0, NaN, "", "a", null, undefined, true, false, {}, 0.5];
  let k = 0;
  for (let j = 0; j < n; j++) for (let i = 0; i < vals.length; i++) { if (vals[i]) k++; else k += 1000; }
  return k;
}
function whileLoop(n) { let i = 0, s = 0; while (i < n) { s = s + (i % 3 === 0 ? 2 : 1); i = i + 1; } return s; }
function strs(n) { let s = ""; for (let i = 0; i < n; i++) s += i; return s.length; }
function mixedTypes(n) { let x = 0; for (let i = 0; i < n; i++) { x = x + 1; if (i === n / 2) x = "s"; } return x; }
function inc(n) { let a = 0, b = 0, c = 10; for (let i = 0; i < n; i++) { a = b++; c--; } return [a, b, c].join(","); }
function div(n) { let s = 1; for (let i = 1; i < n; i++) s = s / 1.5 + i / i; return s.toFixed(10); }
function infs(n) { let x = 1, k = 0; for (let i = 0; i < n; i++) { x = x * 1e10; if (x > 1e300) k++; } return [x, k].join(","); }
function nested(n) { let s = 0; for (let i = 0; i < n; i++) for (let j = 0; j < i; j++) s += j - i; return s; }
function earlyReturn(n) { for (let i = 0; i < n; i++) { if (i * i > 5000) return i; } return -1; }
function breaks(n) { let s = 0; outer: for (let i = 0; i < n; i++) { for (let j = 0; j < n; j++) { if (j > i) continue outer; if (i > 40) break outer; s += j; } } return s; }
function Vec2(x, y) { this.x = x; this.y = y; }
Vec2.prototype.len2 = function () { return this.x * this.x + this.y * this.y; };
function props(n) { const v = new Vec2(1, 2); let s = 0; for (let i = 0; i < n; i++) { v.x = v.x + 1; s += v.len2() - v.y; } return s; }
function manySlots(n) { const o = { a: 1, b: 2, c: 3, d: 4, e: 5 }; let s = 0; for (let i = 0; i < n; i++) { o.d = o.d + o.a; o.e = o.c * 2; s += o.d + o.e + o.b; } return s; }
function shapes(n) { const os = [{ x: 1 }, { y: 2, x: 3 }, { x: 5, z: 1 }]; let s = 0; for (let i = 0; i < n; i++) { const o = os[i % 3]; s += o.x; } return s; }
function objVals(n) { const o = { v: 1, w: 2 }; const keep = []; for (let i = 0; i < n; i++) { o.v = i % 2 ? { k: i } : i; if (i % 100 === 0) keep.push(o.v); o.w = "s" + (i % 3); } return [typeof o.v, o.w, keep.length, keep[1].k].join(","); }
function pt(x, y) { return { x: x, y: y }; }
function addPt(a, b) { const r = pt(a.x + b.x, a.y + b.y); r.x = r.x * 1; return r; }
function noLoop(n) { let p = pt(0, 0); for (let i = 0; i < n; i++) p = addPt(p, pt(1, 2)); return p.x + "," + p.y; }
function Counter() { this.n = 0; this.step = 2; }
Counter.prototype.tick = function () { this.n = this.n + this.step; return this.n; };
function methods(n) { const c = new Counter(); let last = 0; for (let i = 0; i < n; i++) { last = c.tick(); if (i === n - 10) c.step = 3; } return last; }
function missing(n) { const o = { a: 1 }; let s = 0; for (let i = 0; i < n; i++) { if (i === 500) o.b = 2; s += o.a + (o.b || 0); } return s; }
function frozen(n) { const o = Object.freeze({ a: 1 }); let s = 0; for (let i = 0; i < n; i++) { try { o.a = i; } catch (e) {} s += o.a; } return s; }
const out = [sum(100000), mix(1000).toFixed(6), cmps(1000), nan(200), truthy(100), whileLoop(10000), strs(1000), mixedTypes(1000), inc(1000), div(500), infs(100), nested(300), earlyReturn(1000), breaks(100), props(2000), manySlots(1000), shapes(3000), objVals(1000), noLoop(500), methods(1000), missing(1000), frozen(1000)];
for (const x of out) console.log(x);
