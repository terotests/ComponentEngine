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
const out = [sum(100000), mix(1000).toFixed(6), cmps(1000), nan(200), truthy(100), whileLoop(10000), strs(1000), mixedTypes(1000), inc(1000), div(500), infs(100), nested(300), earlyReturn(1000), breaks(100)];
for (const x of out) console.log(x);
