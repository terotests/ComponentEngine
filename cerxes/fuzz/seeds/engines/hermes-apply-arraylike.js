// hermes: Function.prototype.apply / spread of an array-like whose length getter changes (Hermes, Chakra) size-overflow
function count() { return arguments.length; }
var n = 0;
var al = { get length() { return n++ === 0 ? 3 : 1e9; }, 0: 1, 1: 2, 2: 3 };
try { print(count.apply(null, al)); } catch (e) { print(e.name); }
try { print(count.apply(null, { length: 2 ** 32 })); } catch (e) { print(e.name); }
try { print(count(...{ [Symbol.iterator]: function* () { for (var i = 0; i < 70000; i++) yield i; } })); } catch (e) { print(e.name); }
try { print(Reflect.apply(count, null, { length: -1 })); } catch (e) { print(e.name); }
try { print(new (Function.prototype.bind.apply(Array, [null].concat(new Array(100000).fill(0))))().length); } catch (e) { print(e.name); }
