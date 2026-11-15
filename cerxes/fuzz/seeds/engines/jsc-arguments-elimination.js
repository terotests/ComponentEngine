// jsc: arguments elimination / mapped arguments aliasing (several JSC DFG bugs) jit-arguments
function g(a, b) { arguments[0] = 7; delete arguments[1]; arguments.length = 5; return [a, b, arguments.length, arguments[1]]; }
for (var i = 0; i < 300; i++) g(1, 2);
print(String(g(1, 2)));
function h() { return Array.prototype.slice.call(arguments, { valueOf() { arguments.length = 0; return 1; } }); }
for (var j = 0; j < 300; j++) h(1, 2, 3);
print(String(h(1, 2, 3)));
function k(x) { 'use strict'; x = 4; return arguments[0]; }
print(k(1));
function m(x) { Object.defineProperty(arguments, '0', { get() { return 'g'; } }); return x; }
print(m(1));
