// chakra: stack-allocated arguments / closure escaping from an inlined function (lokihardt Chakra bugs) jit-arguments
var keep;
function inner() { keep = arguments; return arguments.length; }
function outer(a, b, c) { return inner(a, b, c); }
for (var i = 0; i < 300; i++) outer(i, i + 1, i + 2);
print(outer({ x: 1 }, 2, 3), keep.length, keep[0].x);
function capt(x) { var f = function () { return x; }; arguments[0] = 9; return f; }
for (var j = 0; j < 300; j++) capt(j);
print(capt(1)());
function leak() { return function () { return arguments; }.apply(null, arguments); }
for (var k = 0; k < 300; k++) leak(1, 2);
var l = leak(1, 2, 3);
print(l.length, l[2]);
