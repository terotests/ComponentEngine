// sm: CVE-2024-29943 (Pwn2Own 2024, range analysis of bounds check) jit-bounds
function f(a, x) {
  var i = x >>> 0;
  var j = Math.min(i, 7);
  var k = (j + 0x7ffffffd) | 0;
  k = (k - 0x7ffffffd) | 0;
  return a[k];
}
var a = [0, 1, 2, 3, 4, 5, 6, 7];
for (var n = 0; n < 300; n++) f(a, n & 7);
print(f(a, -1), f(a, 2 ** 32 - 1), f(a, -2147483648), f(a, 4));
function g(a, x) { var i = (x | 0) % 8; if (i >= 0) return a[i]; return a[-i - 1]; }
for (var m = 0; m < 300; m++) g(a, m);
print(g(a, -2147483648), g(a, -0), g(a, 2 ** 31));
