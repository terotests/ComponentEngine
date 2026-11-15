// sm: CVE-2019-17026 (IonMonkey StoreElementHole ignored an index setter on the prototype) jit-proto
function fill(arr, n) { for (var i = 0; i < n; i++) arr[i] = i + 0.5; return arr; }
for (var k = 0; k < 300; k++) fill([], 8);
var hit = [];
Object.defineProperty(Array.prototype, 5, { set(v) { hit.push(v); this.length = 0; }, configurable: true });
var r = fill([], 8);
print(r.length, String(r), hit.join());
delete Array.prototype[5];
Object.defineProperty(Object.prototype, 3, { get() { return 'proto'; }, configurable: true });
var holes = [0, 1, 2, , 4];
print(holes[3], holes.indexOf('proto'), holes.includes('proto'), String(holes.map(x => x)));
delete Object.prototype[3];
