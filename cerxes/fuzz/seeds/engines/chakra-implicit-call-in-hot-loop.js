// chakra: implicit calls (has trap, Symbol.hasInstance, getters) not checked in JIT loops (lokihardt) jit-sideeffect
var arr = [1.1, 2.2, 3.3, 4.4];
function hot(o, a) { var s = 0; for (var i = 0; i < a.length; i++) { if ('k' in o) s += a[i]; } return s; }
for (var n = 0; n < 300; n++) hot({ k: 1 }, arr);
print(hot(new Proxy({}, { has() { arr.length = 0; arr[0] = {}; return true; } }), arr));
var arr2 = [1, 2, 3, 4];
function hot2(o, a) { var s = 0; for (var i = 0; i < 4; i++) { if (o instanceof Object) s += a[i]; } return s; }
for (var m = 0; m < 300; m++) hot2({}, arr2);
var C = function () {};
Object.defineProperty(C, Symbol.hasInstance, { value() { arr2.length = 1; return true; } });
print(hot2({}, arr2), String(hot2(Object.setPrototypeOf({}, null), arr2)));
