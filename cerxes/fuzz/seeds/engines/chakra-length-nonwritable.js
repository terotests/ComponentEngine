// chakra: arrays with a non-writable length or frozen elements in push / unshift / splice / sort (Chakra, JSC) array-length
var a = [1, 2, 3];
Object.defineProperty(a, 'length', { writable: false });
['push', 'pop', 'shift', 'unshift', 'splice', 'fill', 'reverse', 'copyWithin'].forEach(function (m) {
  try { a[m](0, 1); print(m, 'ok', a.length); } catch (e) { print(m, e.name, a.length); }
});
var f = Object.freeze([3, 1, 2]);
try { f.sort(); print('sorted'); } catch (e) { print(e.name); }
var s = Object.seal([1, 2, 3]);
try { s.length = 1; print(s.length); } catch (e) { print(e.name); }
var g = [1, 2, 3];
try { g.length = { valueOf() { Object.defineProperty(g, 'length', { writable: false }); return 0; } }; print(g.length); } catch (e) { print(e.name, g.length); }
