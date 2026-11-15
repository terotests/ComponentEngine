// jsc: lokihardt 2017 concat / spread length overflow (Project Zero) size-overflow
try { [].concat({ length: 2 ** 53 - 1, [Symbol.isConcatSpreadable]: true }); print('no throw'); } catch (e) { print(e.name); }
try { Array.prototype.push.call({ length: 2 ** 53 - 1 }, 1); print('no throw'); } catch (e) { print(e.name); }
try { Array.prototype.unshift.call({ length: 2 ** 53 - 1 }, 1); print('no throw'); } catch (e) { print(e.name); }
try { Array.prototype.splice.call({ length: 2 ** 53 - 1 }, 0, 0, 1); print('no throw'); } catch (e) { print(e.name); }
var big = new Array(2 ** 32 - 1);
try { big.push(1); print('no throw', big.length); } catch (e) { print(e.name, big.length); }
try { print([].concat(big, [1]).length); } catch (e) { print(e.name); }
