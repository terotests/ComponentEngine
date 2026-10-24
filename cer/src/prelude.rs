// SPDX-License-Identifier: AGPL-3.0-or-later
//! Built-ins written in JavaScript: the ones that are only other built-ins
//! put together (Set methods, iterator helpers, `groupBy`, the Annex B
//! accessor methods, `Promise.allSettled` / `any` / `withResolvers`, …),
//! and the host's modules (crate::module::REGISTRY, run after it).
//! `Engine::new` runs this once, before any script. Every function here is
//! defined non-enumerable, and reports `[native code]` from `toString`.

pub const PRELUDE: &str = r#"(function () {
var defineProperty = Object.defineProperty;
var getPrototypeOf = Object.getPrototypeOf;
var create = Object.create;
function hide(o, name, v) { defineProperty(o, name, { value: v, writable: true, enumerable: false, configurable: true }); }
function tag(o, name) { defineProperty(o, Symbol.toStringTag, { value: name, writable: false, enumerable: false, configurable: true }); }
function isCallable(f) { return typeof f === 'function'; }
function isObject(v) { return (typeof v === 'object' && v !== null) || typeof v === 'function'; }
function toIntegerOrInfinity(v) { var n = Number(v); if (n !== n) return 0; if (n === Infinity || n === -Infinity) return n; return n < 0 ? -Math.floor(-n) : Math.floor(n); }

tag(Math, 'Math');
tag(JSON, 'JSON');
if (typeof Reflect === 'object') tag(Reflect, 'Reflect');
[[Map, 'Map'], [Set, 'Set'], [WeakMap, 'WeakMap'], [WeakSet, 'WeakSet'], [Promise, 'Promise'], [Symbol, 'Symbol']].forEach(function (e) {
  if (!Object.prototype.hasOwnProperty.call(e[0].prototype, Symbol.toStringTag)) tag(e[0].prototype, e[1]);
});

// ---- Set methods (ES2025): the argument is any set-like: size, has, keys
function setLike(o) {
  if (!isObject(o)) throw new TypeError('The argument must be a set-like object');
  var size = Number(o.size);
  if (size !== size) throw new TypeError("The 'size' property must be a number");
  var has = o.has, keys = o.keys;
  if (!isCallable(has)) throw new TypeError("The 'has' property must be a function");
  if (!isCallable(keys)) throw new TypeError("The 'keys' property must be a function");
  return { size: size, has: function (v) { return !!has.call(o, v); }, keys: function () { return keys.call(o); } };
}
function eachKey(sl, f) {
  var it = sl.keys();
  for (;;) { var r = it.next(); if (r.done) return; if (f(r.value) === false) { if (isCallable(it.return)) it.return(); return; } }
}
var SetProto = Set.prototype;
hide(SetProto, 'union', function union(other) {
  var o = setLike(other); var out = new Set(this);
  eachKey(o, function (v) { out.add(v); }); return out;
});
hide(SetProto, 'intersection', function intersection(other) {
  var o = setLike(other); var out = new Set(); var self = this;
  if (this.size <= o.size) { this.forEach(function (v) { if (o.has(v)) out.add(v); }); }
  else { eachKey(o, function (v) { if (self.has(v)) out.add(v); }); }
  return out;
});
hide(SetProto, 'difference', function difference(other) {
  var o = setLike(other); var out = new Set(); this.forEach(function (v) { if (!o.has(v)) out.add(v); }); return out;
});
hide(SetProto, 'symmetricDifference', function symmetricDifference(other) {
  var o = setLike(other); var out = new Set(this); var self = this;
  eachKey(o, function (v) { if (self.has(v)) out.delete(v); else out.add(v); }); return out;
});
hide(SetProto, 'isSubsetOf', function isSubsetOf(other) {
  var o = setLike(other); if (this.size > o.size) return false; var ok = true;
  this.forEach(function (v) { if (ok && !o.has(v)) ok = false; }); return ok;
});
hide(SetProto, 'isSupersetOf', function isSupersetOf(other) {
  var o = setLike(other); if (this.size < o.size) return false; var ok = true; var self = this;
  eachKey(o, function (v) { if (!self.has(v)) { ok = false; return false; } }); return ok;
});
hide(SetProto, 'isDisjointFrom', function isDisjointFrom(other) {
  var o = setLike(other); var ok = true; var self = this;
  if (this.size <= o.size) { this.forEach(function (v) { if (ok && o.has(v)) ok = false; }); }
  else { eachKey(o, function (v) { if (self.has(v)) { ok = false; return false; } }); }
  return ok;
});

// ---- Iterator (ES2025): the prototype every built-in iterator shares, and
// its helpers
var ArrayIteratorProto = getPrototypeOf([][Symbol.iterator]());
var IteratorProto = getPrototypeOf(ArrayIteratorProto);
function Iterator() {
  if (new.target === undefined || new.target === Iterator) throw new TypeError('Abstract class Iterator not directly constructable');
}
defineProperty(Iterator, 'prototype', { value: IteratorProto, writable: false, enumerable: false, configurable: false });
hide(IteratorProto, 'constructor', Iterator);
tag(IteratorProto, 'Iterator');
hide(globalThis, 'Iterator', Iterator);

var HelperProto = create(IteratorProto);
tag(HelperProto, 'Iterator Helper');
function helper(src, step) {
  var h = create(HelperProto); var done = false;
  var next = src.next;
  hide(h, 'next', function () {
    if (done) return { value: undefined, done: true };
    var r = step(src, next);
    if (r.done) done = true;
    return r;
  });
  hide(h, 'return', function () {
    done = true; if (isCallable(src.return)) src.return(); return { value: undefined, done: true };
  });
  return h;
}
function need(f) { if (!isCallable(f)) throw new TypeError(String(f) + ' is not a function'); }
function nextOf(it) { return it.next(); }
hide(IteratorProto, 'map', function map(f) {
  need(f); var i = 0;
  return helper(this, function (src, next) { var r = next.call(src); if (r.done) return r; return { value: f(r.value, i++), done: false }; });
});
hide(IteratorProto, 'filter', function filter(f) {
  need(f); var i = 0;
  return helper(this, function (src, next) { for (;;) { var r = next.call(src); if (r.done) return r; if (f(r.value, i++)) return { value: r.value, done: false }; } });
});
function count(n) {
  var v = Number(n); if (v !== v) throw new RangeError(String(n) + ' must be positive');
  v = toIntegerOrInfinity(v); if (v < 0) throw new RangeError(String(n) + ' must be positive'); return v;
}
hide(IteratorProto, 'take', function take(n) {
  var left = count(n);
  return helper(this, function (src, next) {
    if (left <= 0) { if (isCallable(src.return)) src.return(); return { value: undefined, done: true }; }
    left--; var r = next.call(src); return r.done ? r : { value: r.value, done: false };
  });
});
hide(IteratorProto, 'drop', function drop(n) {
  var left = count(n);
  return helper(this, function (src, next) {
    while (left > 0) { left--; var s = next.call(src); if (s.done) return s; }
    var r = next.call(src); return r.done ? r : { value: r.value, done: false };
  });
});
hide(IteratorProto, 'flatMap', function flatMap(f) {
  need(f); var i = 0; var inner = null;
  return helper(this, function (src, next) {
    for (;;) {
      if (inner) { var ir = inner.next(); if (!ir.done) return { value: ir.value, done: false }; inner = null; }
      var r = next.call(src); if (r.done) return r;
      var m = f(r.value, i++);
      if (!isObject(m) && typeof m !== 'string') throw new TypeError('flatMap mapper must return an iterable');
      inner = m[Symbol.iterator]();
    }
  });
});
hide(IteratorProto, 'reduce', function reduce(f) {
  need(f); var acc, i = 0, r;
  if (arguments.length < 2) { r = this.next(); if (r.done) throw new TypeError('Reduce of empty iterator with no initial value'); acc = r.value; i = 1; }
  else acc = arguments[1];
  for (;;) { r = this.next(); if (r.done) return acc; acc = f(acc, r.value, i++); }
});
hide(IteratorProto, 'toArray', function toArray() {
  var out = []; for (;;) { var r = this.next(); if (r.done) return out; out.push(r.value); }
});
hide(IteratorProto, 'forEach', function forEach(f) {
  need(f); var i = 0; for (;;) { var r = this.next(); if (r.done) return undefined; f(r.value, i++); }
});
hide(IteratorProto, 'some', function some(f) {
  need(f); var i = 0; for (;;) { var r = this.next(); if (r.done) return false; if (f(r.value, i++)) { if (isCallable(this.return)) this.return(); return true; } }
});
hide(IteratorProto, 'every', function every(f) {
  need(f); var i = 0; for (;;) { var r = this.next(); if (r.done) return true; if (!f(r.value, i++)) { if (isCallable(this.return)) this.return(); return false; } }
});
hide(IteratorProto, 'find', function find(f) {
  need(f); var i = 0; for (;;) { var r = this.next(); if (r.done) return undefined; if (f(r.value, i++)) { if (isCallable(this.return)) this.return(); return r.value; } }
});
var WrapProto = create(IteratorProto);
hide(Iterator, 'from', function from(o) {
  var it;
  if (typeof o === 'string' || isObject(o)) {
    var m = o[Symbol.iterator];
    if (m !== undefined && m !== null) { need(m); it = m.call(o); } else it = o;
  } else throw new TypeError(String(o) + ' is not an object');
  if (it instanceof Iterator) return it;
  var w = create(WrapProto);
  hide(w, 'next', function () { return it.next(); });
  hide(w, 'return', function () { return isCallable(it.return) ? it.return() : { value: undefined, done: true }; });
  return w;
});

// ---- groupBy (ES2024)
hide(Map, 'groupBy', function groupBy(items, f) {
  need(f); var m = new Map(); var i = 0;
  for (var v of items) { var k = f(v, i++); if (k === 0) k = 0; var g = m.get(k); if (g === undefined) { g = []; m.set(k, g); } g.push(v); }
  return m;
});
if (typeof Object.groupBy !== 'function') hide(Object, 'groupBy', function groupBy(items, f) {
  need(f); var o = create(null); var i = 0;
  for (var v of items) { var k = f(v, i++); if (typeof k !== 'symbol') k = String(k); if (!(k in o)) o[k] = []; o[k].push(v); }
  return o;
});

// ---- Annex B accessor methods
var OP = Object.prototype;
hide(OP, '__defineGetter__', function __defineGetter__(name, f) {
  need(f); defineProperty(Object(this), name, { get: f, enumerable: true, configurable: true });
});
hide(OP, '__defineSetter__', function __defineSetter__(name, f) {
  need(f); defineProperty(Object(this), name, { set: f, enumerable: true, configurable: true });
});
function lookup(o, name, which) {
  o = Object(o);
  while (o !== null) {
    var d = Object.getOwnPropertyDescriptor(o, name);
    if (d) return d[which];
    o = getPrototypeOf(o);
  }
  return undefined;
}
hide(OP, '__lookupGetter__', function __lookupGetter__(name) { return lookup(this, name, 'get'); });
hide(OP, '__lookupSetter__', function __lookupSetter__(name) { return lookup(this, name, 'set'); });

// ---- Promise: species, subclassing, the combinators over the native core
var PP = Promise.prototype;
var nativeThen = PP.then;
var isPromiseObj = globalThis.__isPromise;
delete globalThis.__isPromise;
function isPromise(x) { return isPromiseObj(x); }
function speciesOf(o, d) {
  var C = o.constructor;
  if (C === undefined) return d;
  if (!isObject(C)) throw new TypeError('object.constructor is not an object');
  var S = C[Symbol.species];
  if (S === undefined || S === null) return d;
  if (isCallable(S)) return S;
  throw new TypeError('object.constructor[Symbol.species] is not a constructor');
}
function capability(C) {
  if (!isCallable(C)) throw new TypeError('Promise resolver is not a constructor');
  var res, rej;
  var p = new C(function (a, b) {
    if (res !== undefined || rej !== undefined) throw new TypeError('Promise executor has already been invoked');
    res = a; rej = b;
  });
  if (!isCallable(res) || !isCallable(rej)) throw new TypeError('Promise resolve or reject function is not callable');
  return { promise: p, resolve: res, reject: rej };
}
defineProperty(Promise, Symbol.species, { get: function () { return this; }, enumerable: false, configurable: true });
hide(PP, 'then', function then(ok, err) {
  if (!isPromise(this)) throw new TypeError('Promise.prototype.then called on incompatible receiver');
  var C = speciesOf(this, Promise);
  var d = nativeThen.call(this, ok, err);
  if (C === Promise) return d;
  var cap = capability(C);
  nativeThen.call(d, cap.resolve, cap.reject);
  return cap.promise;
});
hide(PP, 'catch', function (err) { return this.then(undefined, err); });
hide(PP, 'finally', function (f) {
  if (!isObject(this)) throw new TypeError('Promise.prototype.finally called on a non-object');
  var C = speciesOf(this, Promise);
  if (!isCallable(f)) return this.then(f, f);
  return this.then(
    function (v) { return C.resolve(f()).then(function () { return v; }); },
    function (e) { return C.resolve(f()).then(function () { throw e; }); });
});
hide(Promise, 'resolve', function resolve(x) {
  if (!isObject(this)) throw new TypeError('PromiseResolve called on non-object');
  if (isPromise(x) && x.constructor === this) return x;
  var cap = capability(this); cap.resolve(x); return cap.promise;
});
hide(Promise, 'reject', function reject(e) {
  var cap = capability(this); cap.reject(e); return cap.promise;
});
hide(Promise, 'all', function all(items) {
  var C = this; var cap = capability(C);
  try {
    var resolveFn = C.resolve; if (!isCallable(resolveFn)) throw new TypeError('Promise.resolve is not a function');
    var out = [], left = 1, i = 0;
    for (var v of items) {
      (function (k) {
        left++; out[k] = undefined; var called = false;
        resolveFn.call(C, v).then(function (x) { if (called) return; called = true; out[k] = x; if (--left === 0) cap.resolve(out); }, cap.reject);
      })(i++);
    }
    if (--left === 0) cap.resolve(out);
  } catch (e) { cap.reject(e); }
  return cap.promise;
});
hide(Promise, 'race', function race(items) {
  var C = this; var cap = capability(C);
  try {
    var resolveFn = C.resolve;
    for (var v of items) resolveFn.call(C, v).then(cap.resolve, cap.reject);
  } catch (e) { cap.reject(e); }
  return cap.promise;
});
hide(Promise, 'try', function (f) {
  var cap = capability(this); var args = [];
  for (var i = 1; i < arguments.length; i++) args.push(arguments[i]);
  try { cap.resolve(f.apply(undefined, args)); } catch (e) { cap.reject(e); }
  return cap.promise;
});
hide(Promise, 'allSettled', function allSettled(items) {
  var C = this;
  return new C(function (resolve, reject) {
    var out = [], left = 1, i = 0;
    for (var v of items) {
      (function (k) {
        left++;
        C.resolve(v).then(function (x) { out[k] = { status: 'fulfilled', value: x }; if (--left === 0) resolve(out); },
                          function (e) { out[k] = { status: 'rejected', reason: e }; if (--left === 0) resolve(out); });
      })(i++);
    }
    if (--left === 0) resolve(out);
  });
});
hide(Promise, 'any', function any(items) {
  var C = this;
  return new C(function (resolve, reject) {
    var errs = [], left = 1, i = 0;
    for (var v of items) {
      (function (k) {
        left++;
        C.resolve(v).then(resolve, function (e) { errs[k] = e; if (--left === 0) reject(new AggregateError(errs, 'All promises were rejected')); });
      })(i++);
    }
    if (--left === 0) reject(new AggregateError(errs, 'All promises were rejected'));
  });
});
hide(Promise, 'withResolvers', function withResolvers() {
  var out = {};
  out.promise = new this(function (res, rej) { out.resolve = res; out.reject = rej; });
  return out;
});

// ---- Array.prototype.toSpliced (ES2023)
hide(Array.prototype, 'toSpliced', function toSpliced(start, skip) {
  var o = Object(this); var len = o.length >>> 0;
  var s = toIntegerOrInfinity(start);
  s = s < 0 ? Math.max(len + s, 0) : Math.min(s, len);
  var n = arguments.length === 0 ? 0 : arguments.length === 1 ? len - s : Math.min(Math.max(toIntegerOrInfinity(skip), 0), len - s);
  var out = [];
  for (var i = 0; i < s; i++) out.push(o[i]);
  for (var j = 2; j < arguments.length; j++) out.push(arguments[j]);
  for (var k = s + n; k < len; k++) out.push(o[k]);
  return out;
});

// ---- ArrayBuffer, SharedArrayBuffer, the typed arrays, DataView, Atomics:
// the bytes and the element access are native (typed.rs), the rest is here
var bufNew = globalThis.__bufNew, bufInfo = globalThis.__bufInfo, bufGet = globalThis.__bufGet, bufSet = globalThis.__bufSet;
var bufCopy = globalThis.__bufCopy, bufDetach = globalThis.__bufDetach, taMake = globalThis.__taMake, taInfo = globalThis.__taInfo;
['__bufNew', '__bufInfo', '__bufGet', '__bufSet', '__bufCopy', '__bufDetach', '__taMake', '__taInfo'].forEach(function (k) { delete globalThis[k]; });
function toIndex(v, what) {
  if (v === undefined) return 0;
  var n = toIntegerOrInfinity(v);
  if (n < 0 || n > 9007199254740991) throw new RangeError('Invalid ' + what);
  return n;
}
function toLength(v) { var n = toIntegerOrInfinity(v); return n <= 0 ? 0 : Math.min(n, 9007199254740991); }
function relIndex(v, len, dflt) {
  if (v === undefined) return dflt;
  var n = toIntegerOrInfinity(v);
  return n < 0 ? Math.max(len + n, 0) : Math.min(n, len);
}
function protoFrom(nt, dflt) {
  if (nt === undefined) return dflt;
  var p = nt.prototype;
  return isObject(p) ? p : dflt;
}
function getter(o, name, f) { defineProperty(o, name, { get: f, enumerable: false, configurable: true }); }
function speciesSelf() { return this; }

function bufLength(b, shared, what) {
  var i = bufInfo(b);
  if (i === -1 || (i % 1 !== 0) !== shared) throw new TypeError(what + ' called on incompatible receiver');
  return i === -2 ? -1 : Math.floor(i);
}
function ArrayBuffer(length) {
  if (new.target === undefined) throw new TypeError("Constructor ArrayBuffer requires 'new'");
  return bufNew(toIndex(length, 'array buffer length'), protoFrom(new.target, ArrayBuffer.prototype), false);
}
function SharedArrayBuffer(length) {
  if (new.target === undefined) throw new TypeError("Constructor SharedArrayBuffer requires 'new'");
  return bufNew(toIndex(length, 'array buffer length'), protoFrom(new.target, SharedArrayBuffer.prototype), true);
}
function bufferSlice(Default, shared, name) {
  return function slice(start, end) {
    var len = bufLength(this, shared, name + '.prototype.slice');
    if (len < 0) throw new TypeError('Cannot perform ' + name + '.prototype.slice on a detached ArrayBuffer');
    var first = relIndex(start, len, 0), fin = relIndex(end, len, len);
    var n = Math.max(fin - first, 0);
    var C = speciesOf(this, Default);
    var out = new C(n);
    var olen = bufLength(out, shared, name + '.prototype.slice');
    if (out === this) throw new TypeError('ArrayBuffer subclass returned this from species constructor');
    if (olen < n) throw new TypeError('Species constructor returned a too small buffer');
    bufCopy(this, first, out, 0, n);
    return out;
  };
}
[[ArrayBuffer, false, 'ArrayBuffer'], [SharedArrayBuffer, true, 'SharedArrayBuffer']].forEach(function (e) {
  var C = e[0], shared = e[1], name = e[2], P = C.prototype;
  getter(P, 'byteLength', function () { var n = bufLength(this, shared, name + '.prototype.byteLength'); return n < 0 ? 0 : n; });
  hide(P, 'slice', bufferSlice(C, shared, name));
  defineProperty(C, Symbol.species, { get: speciesSelf, enumerable: false, configurable: true });
  tag(P, name);
  hide(globalThis, name, C);
});
hide(ArrayBuffer, 'isView', function isView(x) { return taInfo(x) !== undefined || dvOf(x) !== undefined; });
getter(ArrayBuffer.prototype, 'detached', function () { bufLength(this, false, 'ArrayBuffer.prototype.detached'); return bufInfo(this) === -2; });
getter(ArrayBuffer.prototype, 'resizable', function () { bufLength(this, false, 'ArrayBuffer.prototype.resizable'); return false; });
getter(ArrayBuffer.prototype, 'maxByteLength', function () { var n = bufLength(this, false, 'ArrayBuffer.prototype.maxByteLength'); return n < 0 ? 0 : n; });
function transfer(newLength) {
  var len = bufLength(this, false, 'ArrayBuffer.prototype.transfer');
  if (len < 0) throw new TypeError('Cannot perform ArrayBuffer.prototype.transfer on a detached ArrayBuffer');
  var n = newLength === undefined ? len : toIndex(newLength, 'array buffer length');
  var out = bufNew(n, ArrayBuffer.prototype, false);
  bufCopy(this, 0, out, 0, Math.min(n, len));
  bufDetach(this);
  return out;
}
hide(ArrayBuffer.prototype, 'transfer', transfer);
hide(ArrayBuffer.prototype, 'transferToFixedLength', function transferToFixedLength(newLength) { return transfer.call(this, newLength); });

var KINDS = ['Int8Array', 'Uint8Array', 'Uint8ClampedArray', 'Int16Array', 'Uint16Array', 'Int32Array', 'Uint32Array', 'Float32Array', 'Float64Array', 'BigInt64Array', 'BigUint64Array'];
var SIZES = [1, 1, 1, 2, 2, 4, 4, 4, 8, 8, 8];
/** The element conversion of a kind: ToBigInt for the 64-bit ones. */
function elemOf(kind) { return kind >= 9 ? BigInt : Number; }
var CTORS = [];
function TypedArray() { throw new TypeError('Abstract class TypedArray not directly constructable'); }
var TAP = TypedArray.prototype;
function taOf(o, what) {
  var i = taInfo(o);
  if (i === undefined) throw new TypeError(what + ': this is not a typed array');
  return i;
}
function live(o, what) {
  var i = taOf(o, what);
  if (bufInfo(i[1]) === -2) throw new TypeError('Cannot perform ' + what + ' on a detached ArrayBuffer');
  return i;
}
function construct(kind, nt, a, b, c) {
  var size = SIZES[kind];
  var proto = protoFrom(nt, CTORS[kind].prototype);
  if (!isObject(a)) {
    var n = toIndex(a, 'typed array length');
    return taMake(kind, bufNew(n * size, ArrayBuffer.prototype, false), 0, n, proto);
  }
  var bi = bufInfo(a);
  if (bi !== -1) {
    var off = toIndex(b, 'offset');
    if (off % size) throw new RangeError('start offset of ' + KINDS[kind] + ' should be a multiple of ' + size);
    if (bi === -2) throw new TypeError('Cannot perform Construct on a detached ArrayBuffer');
    var blen = Math.floor(bi), len;
    if (c === undefined) {
      if (blen % size) throw new RangeError('byte length of ' + KINDS[kind] + ' should be a multiple of ' + size);
      if (off > blen) throw new RangeError('Start offset ' + off + ' is outside the bounds of the buffer');
      len = (blen - off) / size;
    } else {
      len = toIndex(c, 'typed array length');
      if (off + len * size > blen) throw new RangeError('Invalid typed array length: ' + len);
    }
    return taMake(kind, a, off, len, proto);
  }
  var src = a;
  if (taInfo(a) === undefined) {
    var iter = a[Symbol.iterator];
    if (iter !== undefined && iter !== null) {
      if (!isCallable(iter)) throw new TypeError('Symbol.iterator is not a function');
      src = [];
      var it = iter.call(a);
      for (;;) { var r = it.next(); if (r.done) break; src.push(r.value); }
    }
  }
  var count = toLength(src.length);
  var t = taMake(kind, bufNew(count * size, ArrayBuffer.prototype, false), 0, count, proto);
  for (var i = 0; i < count; i++) t[i] = src[i];
  return t;
}
function defineTA(kind, C) {
  CTORS[kind] = C;
  Object.setPrototypeOf(C, TypedArray);
  Object.setPrototypeOf(C.prototype, TAP);
  defineProperty(C, 'BYTES_PER_ELEMENT', { value: SIZES[kind], writable: false, enumerable: false, configurable: false });
  defineProperty(C.prototype, 'BYTES_PER_ELEMENT', { value: SIZES[kind], writable: false, enumerable: false, configurable: false });
  hide(globalThis, KINDS[kind], C);
}
defineTA(0, function Int8Array(a, b, c) { if (new.target === undefined) throw new TypeError("Constructor Int8Array requires 'new'"); return construct(0, new.target, a, b, c); });
defineTA(1, function Uint8Array(a, b, c) { if (new.target === undefined) throw new TypeError("Constructor Uint8Array requires 'new'"); return construct(1, new.target, a, b, c); });
defineTA(2, function Uint8ClampedArray(a, b, c) { if (new.target === undefined) throw new TypeError("Constructor Uint8ClampedArray requires 'new'"); return construct(2, new.target, a, b, c); });
defineTA(3, function Int16Array(a, b, c) { if (new.target === undefined) throw new TypeError("Constructor Int16Array requires 'new'"); return construct(3, new.target, a, b, c); });
defineTA(4, function Uint16Array(a, b, c) { if (new.target === undefined) throw new TypeError("Constructor Uint16Array requires 'new'"); return construct(4, new.target, a, b, c); });
defineTA(5, function Int32Array(a, b, c) { if (new.target === undefined) throw new TypeError("Constructor Int32Array requires 'new'"); return construct(5, new.target, a, b, c); });
defineTA(6, function Uint32Array(a, b, c) { if (new.target === undefined) throw new TypeError("Constructor Uint32Array requires 'new'"); return construct(6, new.target, a, b, c); });
defineTA(7, function Float32Array(a, b, c) { if (new.target === undefined) throw new TypeError("Constructor Float32Array requires 'new'"); return construct(7, new.target, a, b, c); });
defineTA(8, function Float64Array(a, b, c) { if (new.target === undefined) throw new TypeError("Constructor Float64Array requires 'new'"); return construct(8, new.target, a, b, c); });
defineTA(9, function BigInt64Array(a, b, c) { if (new.target === undefined) throw new TypeError("Constructor BigInt64Array requires 'new'"); return construct(9, new.target, a, b, c); });
defineTA(10, function BigUint64Array(a, b, c) { if (new.target === undefined) throw new TypeError("Constructor BigUint64Array requires 'new'"); return construct(10, new.target, a, b, c); });
defineProperty(TypedArray, Symbol.species, { get: speciesSelf, enumerable: false, configurable: true });

/** A new typed array from `exemplar`'s species, checked. */
function speciesCreate(exemplar, args) {
  var kind = taOf(exemplar, 'TypedArray species create')[0];
  var C = speciesOf(exemplar, CTORS[kind]);
  var out = args.length === 1 ? new C(args[0]) : new C(args[0], args[1], args[2]);
  var oi = live(out, 'TypedArray species create');
  if (args.length === 1 && typeof args[0] === 'number' && oi[3] < args[0]) throw new TypeError('species constructor returned a too short typed array');
  return out;
}
function typedFrom(C, n) {
  var out = new C(n);
  var oi = live(out, 'TypedArray.from');
  if (oi[3] < n) throw new TypeError('constructor returned a too short typed array');
  return out;
}
hide(TypedArray, 'from', function from(source, mapFn, thisArg) {
  var C = this;
  if (!isCallable(C)) throw new TypeError('TypedArray.from: this is not a constructor');
  if (mapFn !== undefined && !isCallable(mapFn)) throw new TypeError('TypedArray.from: mapFn is not a function');
  var vals = [];
  var iter = source[Symbol.iterator];
  if (iter !== undefined && iter !== null) { for (var v of source) vals.push(v); }
  else { var o = Object(source); var n = toLength(o.length); for (var j = 0; j < n; j++) vals.push(o[j]); }
  var out = typedFrom(C, vals.length);
  for (var i = 0; i < vals.length; i++) out[i] = mapFn ? mapFn.call(thisArg, vals[i], i) : vals[i];
  return out;
});
hide(TypedArray, 'of', function of() {
  var out = typedFrom(this, arguments.length);
  for (var i = 0; i < arguments.length; i++) out[i] = arguments[i];
  return out;
});
getter(TAP, 'buffer', function () { return taOf(this, 'TypedArray.prototype.buffer')[1]; });
getter(TAP, 'byteLength', function () { var i = taOf(this, 'TypedArray.prototype.byteLength'); return i[3] * SIZES[i[0]]; });
getter(TAP, 'byteOffset', function () { var i = taOf(this, 'TypedArray.prototype.byteOffset'); return bufInfo(i[1]) === -2 ? 0 : i[2]; });
getter(TAP, 'length', function () { return taOf(this, 'TypedArray.prototype.length')[3]; });
getter(TAP, Symbol.toStringTag, function () { var i = taInfo(this); return i === undefined ? undefined : KINDS[i[0]]; });
function len(o, what) { return live(o, what)[3]; }
function fn(f, what) { if (!isCallable(f)) throw new TypeError(what + ': ' + String(f) + ' is not a function'); return f; }
hide(TAP, 'at', function at(index) {
  var n = len(this, 'TypedArray.prototype.at'); var k = toIntegerOrInfinity(index); if (k < 0) k += n;
  return k < 0 || k >= n ? undefined : this[k];
});
hide(TAP, 'copyWithin', function copyWithin(target, start, end) {
  var info = live(this, 'TypedArray.prototype.copyWithin'), n = info[3], size = SIZES[info[0]];
  var to = relIndex(target, n, 0), from = relIndex(start, n, 0), fin = relIndex(end, n, n);
  var count = Math.min(fin - from, n - to);
  if (count > 0) bufCopy(info[1], info[2] + from * size, info[1], info[2] + to * size, count * size);
  return this;
});
hide(TAP, 'entries', function entries() { live(this, 'TypedArray.prototype.entries'); return Array.prototype.entries.call(this); });
hide(TAP, 'keys', function keys() { live(this, 'TypedArray.prototype.keys'); return Array.prototype.keys.call(this); });
var taValues = function values() { live(this, 'TypedArray.prototype.values'); return Array.prototype.values.call(this); };
hide(TAP, 'values', taValues);
hide(TAP, Symbol.iterator, taValues);
hide(TAP, 'every', function every(f, t) { var n = len(this, 'TypedArray.prototype.every'); fn(f, 'every'); for (var i = 0; i < n; i++) if (!f.call(t, this[i], i, this)) return false; return true; });
hide(TAP, 'some', function some(f, t) { var n = len(this, 'TypedArray.prototype.some'); fn(f, 'some'); for (var i = 0; i < n; i++) if (f.call(t, this[i], i, this)) return true; return false; });
hide(TAP, 'fill', function fill(v, start, end) {
  var n = len(this, 'TypedArray.prototype.fill'); var x = elemOf(taInfo(this)[0])(v);
  var s = relIndex(start, n, 0), e = relIndex(end, n, n);
  for (var i = s; i < e; i++) this[i] = x;
  return this;
});
hide(TAP, 'filter', function filter(f, t) {
  var n = len(this, 'TypedArray.prototype.filter'); fn(f, 'filter'); var kept = [];
  for (var i = 0; i < n; i++) { var v = this[i]; if (f.call(t, v, i, this)) kept.push(v); }
  var out = speciesCreate(this, [kept.length]);
  for (var j = 0; j < kept.length; j++) out[j] = kept[j];
  return out;
});
hide(TAP, 'find', function find(f, t) { var n = len(this, 'TypedArray.prototype.find'); fn(f, 'find'); for (var i = 0; i < n; i++) { var v = this[i]; if (f.call(t, v, i, this)) return v; } return undefined; });
hide(TAP, 'findIndex', function findIndex(f, t) { var n = len(this, 'TypedArray.prototype.findIndex'); fn(f, 'findIndex'); for (var i = 0; i < n; i++) if (f.call(t, this[i], i, this)) return i; return -1; });
hide(TAP, 'findLast', function findLast(f, t) { var n = len(this, 'TypedArray.prototype.findLast'); fn(f, 'findLast'); for (var i = n - 1; i >= 0; i--) { var v = this[i]; if (f.call(t, v, i, this)) return v; } return undefined; });
hide(TAP, 'findLastIndex', function findLastIndex(f, t) { var n = len(this, 'TypedArray.prototype.findLastIndex'); fn(f, 'findLastIndex'); for (var i = n - 1; i >= 0; i--) if (f.call(t, this[i], i, this)) return i; return -1; });
hide(TAP, 'forEach', function forEach(f, t) { var n = len(this, 'TypedArray.prototype.forEach'); fn(f, 'forEach'); for (var i = 0; i < n; i++) f.call(t, this[i], i, this); });
hide(TAP, 'includes', function includes(x, from) {
  var n = len(this, 'TypedArray.prototype.includes'); if (n === 0) return false;
  for (var i = relIndex(from, n, 0); i < n; i++) { var v = this[i]; if (v === x || (v !== v && x !== x)) return true; }
  return false;
});
hide(TAP, 'indexOf', function indexOf(x, from) {
  var n = len(this, 'TypedArray.prototype.indexOf');
  for (var i = relIndex(from, n, 0); i < n; i++) if (this[i] === x) return i;
  return -1;
});
hide(TAP, 'lastIndexOf', function lastIndexOf(x, from) {
  var n = len(this, 'TypedArray.prototype.lastIndexOf');
  var k = arguments.length > 1 ? toIntegerOrInfinity(from) : n - 1;
  k = k < 0 ? n + k : Math.min(k, n - 1);
  for (var i = k; i >= 0; i--) if (this[i] === x) return i;
  return -1;
});
hide(TAP, 'join', function join(sep) {
  var n = len(this, 'TypedArray.prototype.join'); var s = sep === undefined ? ',' : String(sep); var out = '';
  for (var i = 0; i < n; i++) { if (i) out += s; var v = this[i]; if (v !== undefined) out += String(v); }
  return out;
});
hide(TAP, 'map', function map(f, t) {
  var n = len(this, 'TypedArray.prototype.map'); fn(f, 'map');
  var out = speciesCreate(this, [n]);
  for (var i = 0; i < n; i++) out[i] = f.call(t, this[i], i, this);
  return out;
});
function reducer(right) {
  return function (f, init) {
    var n = len(this, 'TypedArray.prototype.reduce'); fn(f, 'reduce');
    var i = right ? n - 1 : 0, step = right ? -1 : 1, acc;
    if (arguments.length > 1) acc = init;
    else { if (n === 0) throw new TypeError('Reduce of empty array with no initial value'); acc = this[i]; i += step; }
    for (; i >= 0 && i < n; i += step) acc = f(acc, this[i], i, this);
    return acc;
  };
}
hide(TAP, 'reduce', reducer(false));
hide(TAP, 'reduceRight', reducer(true));
hide(TAP, 'reverse', function reverse() {
  var n = len(this, 'TypedArray.prototype.reverse');
  for (var i = 0, j = n - 1; i < j; i++, j--) { var t = this[i]; this[i] = this[j]; this[j] = t; }
  return this;
});
hide(TAP, 'set', function set(source, offset) {
  var info = live(this, 'TypedArray.prototype.set');
  var off = toIntegerOrInfinity(offset);
  if (off < 0) throw new RangeError('offset is out of bounds');
  var src = Object(source); var n = toLength(src.length);
  if (n + off > info[3]) throw new RangeError('offset is out of bounds');
  var vals = [];
  for (var i = 0; i < n; i++) vals.push(src[i]);
  for (var j = 0; j < n; j++) this[off + j] = vals[j];
});
hide(TAP, 'slice', function slice(start, end) {
  var n = len(this, 'TypedArray.prototype.slice');
  var s = relIndex(start, n, 0), e = relIndex(end, n, n); var count = Math.max(e - s, 0);
  var out = speciesCreate(this, [count]);
  for (var i = 0; i < count; i++) out[i] = this[s + i];
  return out;
});
function numericCompare(a, b) {
  if (a !== a) return b !== b ? 0 : 1;
  if (b !== b) return -1;
  if (a < b) return -1;
  if (a > b) return 1;
  if (a === 0 && b === 0) return (1 / a < 0 ? -1 : 0) - (1 / b < 0 ? -1 : 0);
  return 0;
}
function sortedValues(o, cmp, what) {
  if (cmp !== undefined && !isCallable(cmp)) throw new TypeError(what + ': the comparison function must be callable');
  var n = len(o, what); var vals = [];
  for (var i = 0; i < n; i++) vals.push(o[i]);
  vals.sort(cmp === undefined ? numericCompare : function (a, b) { var r = Number(cmp(a, b)); return r !== r ? 0 : r; });
  return vals;
}
hide(TAP, 'sort', function sort(cmp) {
  var vals = sortedValues(this, cmp, 'TypedArray.prototype.sort');
  for (var i = 0; i < vals.length; i++) this[i] = vals[i];
  return this;
});
hide(TAP, 'subarray', function subarray(start, end) {
  var info = taOf(this, 'TypedArray.prototype.subarray'); var n = info[3];
  var s = relIndex(start, n, 0), e = relIndex(end, n, n);
  return speciesCreate(this, [info[1], info[2] + s * SIZES[info[0]], Math.max(e - s, 0)]);
});
hide(TAP, 'toLocaleString', function toLocaleString() {
  var n = len(this, 'TypedArray.prototype.toLocaleString'); var out = '';
  for (var i = 0; i < n; i++) { if (i) out += ','; out += this[i].toLocaleString(); }
  return out;
});
hide(TAP, 'toString', Array.prototype.toString);
function sameKind(o, n) { var i = taOf(o, 'TypedArray'); return new CTORS[i[0]](n); }
hide(TAP, 'toReversed', function toReversed() {
  var n = len(this, 'TypedArray.prototype.toReversed'); var out = sameKind(this, n);
  for (var i = 0; i < n; i++) out[i] = this[n - 1 - i];
  return out;
});
hide(TAP, 'toSorted', function toSorted(cmp) {
  var vals = sortedValues(this, cmp, 'TypedArray.prototype.toSorted'); var out = sameKind(this, vals.length);
  for (var i = 0; i < vals.length; i++) out[i] = vals[i];
  return out;
});
hide(TAP, 'with', function (index, value) {
  var n = len(this, 'TypedArray.prototype.with'); var k = toIntegerOrInfinity(index); if (k < 0) k += n;
  var x = elemOf(taInfo(this)[0])(value);
  if (k < 0 || k >= n) throw new RangeError('Invalid typed array index');
  var out = sameKind(this, n);
  for (var i = 0; i < n; i++) out[i] = i === k ? x : this[i];
  return out;
});

var dvData = new WeakMap();
function dvOf(o) { return isObject(o) ? dvData.get(o) : undefined; }
function DataView(buffer, byteOffset, byteLength) {
  if (new.target === undefined) throw new TypeError("Constructor DataView requires 'new'");
  var bi = bufInfo(buffer);
  if (bi === -1) throw new TypeError('First argument to DataView constructor must be an ArrayBuffer');
  var off = toIndex(byteOffset, 'offset');
  if (bi === -2) throw new TypeError('Cannot perform DataView constructor on a detached ArrayBuffer');
  var blen = Math.floor(bi);
  if (off > blen) throw new RangeError('Start offset ' + off + ' is outside the bounds of the buffer');
  var n = byteLength === undefined ? blen - off : toIndex(byteLength, 'DataView length');
  if (off + n > blen) throw new RangeError('Invalid DataView length ' + n);
  var o = create(protoFrom(new.target, DataView.prototype));
  dvData.set(o, [buffer, off, n]);
  return o;
}
var DVP = DataView.prototype;
function dv(o, what) { var d = dvOf(o); if (d === undefined) throw new TypeError(what + ' called on incompatible receiver'); return d; }
getter(DVP, 'buffer', function () { return dv(this, 'DataView.prototype.buffer')[0]; });
getter(DVP, 'byteLength', function () { var d = dv(this, 'DataView.prototype.byteLength'); if (bufInfo(d[0]) === -2) throw new TypeError('detached'); return d[2]; });
getter(DVP, 'byteOffset', function () { var d = dv(this, 'DataView.prototype.byteOffset'); if (bufInfo(d[0]) === -2) throw new TypeError('detached'); return d[1]; });
tag(DVP, 'DataView');
['Int8', 'Uint8', '', 'Int16', 'Uint16', 'Int32', 'Uint32', 'Float32', 'Float64', 'BigInt64', 'BigUint64'].forEach(function (name, kind) {
  if (!name) return;
  hide(DVP, 'get' + name, function (index, little) {
    var d = dv(this, 'DataView.prototype.get' + name); var i = toIndex(index, 'offset');
    if (bufInfo(d[0]) === -2) throw new TypeError('Cannot perform DataView.prototype.get' + name + ' on a detached ArrayBuffer');
    if (i + SIZES[kind] > d[2]) throw new RangeError('Offset is outside the bounds of the DataView');
    return bufGet(d[0], d[1] + i, kind, !!little);
  });
  hide(DVP, 'set' + name, function (index, value, little) {
    var d = dv(this, 'DataView.prototype.set' + name); var i = toIndex(index, 'offset'); var x = elemOf(kind)(value);
    if (bufInfo(d[0]) === -2) throw new TypeError('Cannot perform DataView.prototype.set' + name + ' on a detached ArrayBuffer');
    if (i + SIZES[kind] > d[2]) throw new RangeError('Offset is outside the bounds of the DataView');
    bufSet(d[0], d[1] + i, kind, x, !!little);
  });
});
hide(globalThis, 'DataView', DataView);

// Atomics: one thread, so plain reads and writes on an integer typed array
var Atomics = {};
function intTA(ta, what) {
  var i = live(ta, 'Atomics.' + what);
  if (i[0] === 2 || (i[0] > 6 && i[0] < 9)) throw new TypeError('Atomics.' + what + ': not an integer typed array');
  return i;
}
function atomicIndex(i, index) {
  var k = toIndex(index, 'atomic access index');
  if (k >= i[3]) throw new RangeError('Invalid atomic access index');
  return k;
}
function rmw(name, op) {
  hide(Atomics, name, function (ta, index, value) {
    var i = intTA(ta, name); var k = atomicIndex(i, index); var v = toIntegerOrInfinity(value);
    var old = ta[k]; ta[k] = op(old, v); return old;
  });
}
rmw('add', function (a, b) { return a + b; });
rmw('sub', function (a, b) { return a - b; });
rmw('and', function (a, b) { return a & b; });
rmw('or', function (a, b) { return a | b; });
rmw('xor', function (a, b) { return a ^ b; });
rmw('exchange', function (a, b) { return b; });
hide(Atomics, 'compareExchange', function compareExchange(ta, index, expected, replacement) {
  var i = intTA(ta, 'compareExchange'); var k = atomicIndex(i, index);
  var e = toIntegerOrInfinity(expected), r = toIntegerOrInfinity(replacement);
  var old = ta[k]; var probe = new CTORS[i[0]](1); probe[0] = e;
  if (old === probe[0]) ta[k] = r;
  return old;
});
hide(Atomics, 'load', function load(ta, index) { var i = intTA(ta, 'load'); return ta[atomicIndex(i, index)]; });
hide(Atomics, 'store', function store(ta, index, value) {
  var i = intTA(ta, 'store'); var k = atomicIndex(i, index); var v = toIntegerOrInfinity(value); ta[k] = v; return v;
});
hide(Atomics, 'isLockFree', function isLockFree(n) { var s = toIntegerOrInfinity(n); return s === 1 || s === 2 || s === 4 || s === 8; });
hide(Atomics, 'wait', function wait(ta, index, value, timeout) {
  var i = intTA(ta, 'wait'); if (i[0] !== 5) throw new TypeError('Atomics.wait: not an Int32Array');
  if (bufInfo(i[1]) % 1 === 0) throw new TypeError('Atomics.wait: not on a SharedArrayBuffer');
  return ta[atomicIndex(i, index)] !== toIntegerOrInfinity(value) ? 'not-equal' : 'timed-out';
});
hide(Atomics, 'notify', function notify(ta, index, count) { var i = intTA(ta, 'notify'); atomicIndex(i, index); return 0; });
tag(Atomics, 'Atomics');
hide(globalThis, 'Atomics', Atomics);

// ---- Proxy: the hooks the VM calls for an operation on a proxy (proxy.rs):
// each runs the handler's trap, or forwards to the target, and checks the
// invariants the specification puts on the trap's answer
var proxySetup = globalThis.__proxySetup, proxyRevoke = globalThis.__proxyRevoke;
delete globalThis.__proxySetup; delete globalThis.__proxyRevoke;
var isProxy = globalThis.__isProxy, isRegExpObj = globalThis.__isRegExp;
delete globalThis.__isProxy; delete globalThis.__isRegExp;
var gopd = Object.getOwnPropertyDescriptor, isExt = Object.isExtensible, objGetProto = Object.getPrototypeOf;
var reflectGet = Reflect.get, reflectHas = Reflect.has, reflectOwnKeys = Reflect.ownKeys;
var reflectDefine = Reflect.defineProperty, reflectDelete = Reflect.deleteProperty;
var reflectApply = Reflect.apply, reflectConstruct = Reflect.construct, nativeReflectSet = Reflect.set;
function trapOf(h, name) {
  var t = h[name];
  if (t === undefined || t === null) return undefined;
  if (!isCallable(t)) throw new TypeError("'" + name + "' on proxy: trap is not a function");
  return t;
}
function keyName(k) { return typeof k === 'symbol' ? k.toString() : String(k); }
function pfail(trap, msg) { throw new TypeError("'" + trap + "' on proxy: " + msg); }
function isDataDesc(d) { return 'value' in d || 'writable' in d; }
function isAccDesc(d) { return 'get' in d || 'set' in d; }
function toDesc(o) {
  if (!isObject(o)) throw new TypeError('Property description must be an object: ' + String(o));
  var d = {};
  if ('enumerable' in o) d.enumerable = !!o.enumerable;
  if ('configurable' in o) d.configurable = !!o.configurable;
  if ('value' in o) d.value = o.value;
  if ('writable' in o) d.writable = !!o.writable;
  if ('get' in o) { var g = o.get; if (g !== undefined && !isCallable(g)) throw new TypeError('Getter must be a function: ' + String(g)); d.get = g; }
  if ('set' in o) { var s = o.set; if (s !== undefined && !isCallable(s)) throw new TypeError('Setter must be a function: ' + String(s)); d.set = s; }
  if (isAccDesc(d) && isDataDesc(d)) throw new TypeError('Invalid property descriptor. Cannot both specify accessors and a value or writable attribute');
  return d;
}
/** IsCompatiblePropertyDescriptor */
// SetIntegrityLevel / TestIntegrityLevel and ToPropertyDescriptor through
// a proxy's traps, in the specification's order
['freeze', 'seal'].forEach(function (name) {
  var nat = Object[name], frozen = name === 'freeze';
  var f = function (o) {
    if (!isProxy(o)) return nat(o);
    if (!Reflect.preventExtensions(o)) throw new TypeError('Cannot ' + name + ' the object');
    var keys = reflectOwnKeys(o);
    for (var i = 0; i < keys.length; i++) {
      if (!frozen) { defineProperty(o, keys[i], { configurable: false }); continue; }
      var d = gopd(o, keys[i]);
      if (d !== undefined) defineProperty(o, keys[i], isAccDesc(d) ? { configurable: false } : { configurable: false, writable: false });
    }
    return o;
  };
  defineProperty(f, 'name', { value: name, writable: false, enumerable: false, configurable: true });
  hide(Object, name, setLength(f, 1));
});
['isFrozen', 'isSealed'].forEach(function (name) {
  var nat = Object[name], frozen = name === 'isFrozen';
  var f = function (o) {
    if (!isProxy(o)) return nat(o);
    if (isExt(o)) return false;
    var keys = reflectOwnKeys(o);
    for (var i = 0; i < keys.length; i++) {
      var d = gopd(o, keys[i]);
      if (d === undefined) continue;
      if (d.configurable) return false;
      if (frozen && isDataDesc(d) && d.writable) return false;
    }
    return true;
  };
  defineProperty(f, 'name', { value: name, writable: false, enumerable: false, configurable: true });
  hide(Object, name, setLength(f, 1));
});
var nativeDefineProperty = defineProperty;
hide(Object, 'defineProperty', setLength(function defineProperty(o, k, d) {
  return nativeDefineProperty(o, k, isProxy(d) ? toDesc(d) : d);
}, 3));

function compatible(ext, d, cur) {
  if (cur === undefined) return ext;
  if (cur.configurable) return true;
  if (d.configurable) return false;
  if ('enumerable' in d && d.enumerable !== cur.enumerable) return false;
  if (isAccDesc(d) && !('get' in cur || 'set' in cur)) return false;
  if (isDataDesc(d) && ('get' in cur || 'set' in cur)) return false;
  if ('value' in cur && !cur.writable) {
    if (d.writable) return false;
    if ('value' in d && !Object.is(d.value, cur.value)) return false;
  }
  if ('get' in cur) {
    if ('get' in d && d.get !== cur.get) return false;
    if ('set' in d && d.set !== cur.set) return false;
  }
  return true;
}
var proxyHooks = [];
proxyHooks[0] = function (t, h, k, r) {
  var trap = trapOf(h, 'get');
  if (!trap) return reflectGet(t, k, r);
  var v = trap.call(h, t, k, r);
  var d = gopd(t, k);
  if (d !== undefined && !d.configurable) {
    if ('value' in d && !d.writable && !Object.is(v, d.value)) pfail('get', "property '" + keyName(k) + "' is a read-only and non-configurable data property on the proxy target but the proxy did not return its actual value");
    if ('get' in d && d.get === undefined && v !== undefined) pfail('get', "property '" + keyName(k) + "' is a non-configurable accessor property on the proxy target and does not have a getter function, but the trap did not return 'undefined'");
  }
  return v;
};
proxyHooks[1] = function (t, h, k, v, r) {
  var trap = trapOf(h, 'set');
  if (!trap) return Reflect.set(t, k, v, r);
  if (!trap.call(h, t, k, v, r)) return false;
  var d = gopd(t, k);
  if (d !== undefined && !d.configurable) {
    if ('value' in d && !d.writable && !Object.is(v, d.value)) pfail('set', "trap returned truish for property '" + keyName(k) + "' which exists in the proxy target as a non-configurable and non-writable data property with a different value");
    if ('set' in d && d.set === undefined) pfail('set', "trap returned truish for property '" + keyName(k) + "' which exists in the proxy target as a non-configurable and non-writable accessor property without a setter");
  }
  return true;
};
proxyHooks[2] = function (t, h, k) {
  var trap = trapOf(h, 'has');
  if (!trap) return reflectHas(t, k);
  var b = !!trap.call(h, t, k);
  if (!b) {
    var d = gopd(t, k);
    if (d !== undefined) {
      if (!d.configurable) pfail('has', "trap returned falsish for property '" + keyName(k) + "' which exists in the proxy target as non-configurable");
      if (!isExt(t)) pfail('has', "trap returned falsish for property '" + keyName(k) + "' but the proxy target is not extensible");
    }
  }
  return b;
};
proxyHooks[3] = function (t, h, k) {
  var trap = trapOf(h, 'deleteProperty');
  if (!trap) return reflectDelete(t, k);
  if (!trap.call(h, t, k)) return false;
  var d = gopd(t, k);
  if (d !== undefined) {
    if (!d.configurable) pfail('deleteProperty', "trap returned truish for property '" + keyName(k) + "' which is non-configurable in the proxy target");
    if (!isExt(t)) pfail('deleteProperty', "trap returned truish for property '" + keyName(k) + "' but the proxy target is non-extensible");
  }
  return true;
};
proxyHooks[4] = function (t, h) {
  var trap = trapOf(h, 'ownKeys');
  if (!trap) return reflectOwnKeys(t);
  var res = trap.call(h, t);
  if (!isObject(res)) pfail('ownKeys', 'trap returned a non-object');
  var n = toLength(res.length), keys = [], seen = new Map();
  for (var i = 0; i < n; i++) {
    var k = res[i];
    if (typeof k !== 'string' && typeof k !== 'symbol') pfail('ownKeys', String(k) + ' is not a valid property name');
    if (seen.has(k)) pfail('ownKeys', "trap returned duplicate entries");
    seen.set(k, true); keys.push(k);
  }
  var ext = isExt(t), tk = reflectOwnKeys(t), fixed = [], loose = [];
  for (var j = 0; j < tk.length; j++) { var d = gopd(t, tk[j]); if (d !== undefined && !d.configurable) fixed.push(tk[j]); else loose.push(tk[j]); }
  if (ext && fixed.length === 0) return keys;
  for (var a = 0; a < fixed.length; a++) {
    if (!seen.has(fixed[a])) pfail('ownKeys', "trap result did not include '" + keyName(fixed[a]) + "'");
    seen.delete(fixed[a]);
  }
  if (ext) return keys;
  for (var b = 0; b < loose.length; b++) {
    if (!seen.has(loose[b])) pfail('ownKeys', "trap result did not include '" + keyName(loose[b]) + "'");
    seen.delete(loose[b]);
  }
  if (seen.size > 0) pfail('ownKeys', 'trap returned extra keys but proxy target is non-extensible');
  return keys;
};
proxyHooks[5] = function (t, h, k) {
  var trap = trapOf(h, 'getOwnPropertyDescriptor');
  if (!trap) return gopd(t, k);
  var r = trap.call(h, t, k);
  if (r !== undefined && !isObject(r)) pfail('getOwnPropertyDescriptor', "trap returned neither object nor undefined for property '" + keyName(k) + "'");
  var td = gopd(t, k);
  if (r === undefined) {
    if (td === undefined) return undefined;
    if (!td.configurable) pfail('getOwnPropertyDescriptor', "trap returned undefined for property '" + keyName(k) + "' which is non-configurable in the proxy target");
    if (!isExt(t)) pfail('getOwnPropertyDescriptor', "trap returned undefined for property '" + keyName(k) + "' which exists in the non-extensible proxy target");
    return undefined;
  }
  var ext = isExt(t), d = toDesc(r);
  if (isAccDesc(d)) { if (!('get' in d)) d.get = undefined; if (!('set' in d)) d.set = undefined; }
  else { if (!('value' in d)) d.value = undefined; if (!('writable' in d)) d.writable = false; }
  if (!('enumerable' in d)) d.enumerable = false;
  if (!('configurable' in d)) d.configurable = false;
  if (!compatible(ext, d, td)) pfail('getOwnPropertyDescriptor', "trap returned descriptor for property '" + keyName(k) + "' that is incompatible with the existing property in the proxy target");
  if (!d.configurable) {
    if (td === undefined || td.configurable) pfail('getOwnPropertyDescriptor', "trap reported non-configurability for property '" + keyName(k) + "' which is either non-existent or configurable in the proxy target");
    if ('writable' in d && !d.writable && td.writable) pfail('getOwnPropertyDescriptor', "trap reported non-configurable and writable for property '" + keyName(k) + "' which is non-configurable, non-writable in the proxy target");
  }
  return d;
};
proxyHooks[6] = function (t, h, k, desc) {
  var d = toDesc(desc);
  var trap = trapOf(h, 'defineProperty');
  if (!trap) return reflectDefine(t, k, d);
  if (!trap.call(h, t, k, d)) return false;
  var td = gopd(t, k), ext = isExt(t);
  var nc = 'configurable' in d && !d.configurable;
  if (td === undefined) {
    if (!ext) pfail('defineProperty', "trap returned truish for adding property '" + keyName(k) + "'  to the non-extensible proxy target");
    if (nc) pfail('defineProperty', "trap returned truish for defining non-configurable property '" + keyName(k) + "' which is either non-existent or configurable in the proxy target");
  } else {
    if (!compatible(ext, d, td)) pfail('defineProperty', "trap returned truish for adding property '" + keyName(k) + "'  that is incompatible with the existing property in the proxy target");
    if (nc && td.configurable) pfail('defineProperty', "trap returned truish for defining non-configurable property '" + keyName(k) + "' which is either non-existent or configurable in the proxy target");
    if ('value' in td && !td.configurable && td.writable && 'writable' in d && !d.writable) pfail('defineProperty', "trap returned truish for defining non-configurable property '" + keyName(k) + "' which cannot be non-writable, unless there exists a corresponding non-configurable, non-writable own property of the target object.");
  }
  return true;
};
proxyHooks[7] = function (t, h) {
  var trap = trapOf(h, 'getPrototypeOf');
  if (!trap) return objGetProto(t);
  var p = trap.call(h, t);
  if (p !== null && !isObject(p)) pfail('getPrototypeOf', 'trap returned neither object nor null');
  if (!isExt(t) && p !== objGetProto(t)) pfail('getPrototypeOf', 'proxy target is non-extensible but the trap did not return its actual prototype');
  return p;
};
proxyHooks[8] = function (t, h, p) {
  var trap = trapOf(h, 'setPrototypeOf');
  if (!trap) { try { Object.setPrototypeOf(t, p); return true; } catch (e) { return false; } }
  if (!trap.call(h, t, p)) return false;
  if (!isExt(t) && p !== objGetProto(t)) pfail('setPrototypeOf', 'trap returned truish for setting a new prototype on the non-extensible proxy target');
  return true;
};
proxyHooks[9] = function (t, h) {
  var trap = trapOf(h, 'isExtensible');
  if (!trap) return isExt(t);
  var b = !!trap.call(h, t);
  if (b !== isExt(t)) pfail('isExtensible', 'trap result does not reflect extensibility of proxy target (which is ' + isExt(t) + ')');
  return b;
};
proxyHooks[10] = function (t, h) {
  var trap = trapOf(h, 'preventExtensions');
  if (!trap) { Object.preventExtensions(t); return true; }
  var b = !!trap.call(h, t);
  if (b && isExt(t)) pfail('preventExtensions', 'trap returned truish but the proxy target is extensible');
  return b;
};
proxyHooks[11] = function (t, h, thisArg, args) {
  var trap = trapOf(h, 'apply');
  if (!trap) return reflectApply(t, thisArg, args);
  return trap.call(h, t, thisArg, args);
};
proxyHooks[12] = function (t, h, args, nt) {
  var trap = trapOf(h, 'construct');
  if (!trap) return reflectConstruct(t, args, nt);
  var r = trap.call(h, t, args, nt);
  if (!isObject(r)) pfail('construct', 'trap returned non-object (' + String(r) + ')');
  return r;
};
proxySetup(proxyHooks);
hide(Proxy, 'revocable', function revocable(target, handler) {
  var p = new Proxy(target, handler);
  return { proxy: p, revoke: function () { proxyRevoke(p); } };
});
// Reflect.set with a receiver other than the target: OrdinarySet
hide(Reflect, 'set', function set(t, k, v, r) {
  if (!isObject(t)) throw new TypeError('Reflect.set called on non-object');
  if (arguments.length < 4 || r === t) return nativeReflectSet(t, k, v);
  var o = t;
  while (o !== null) {
    var d = gopd(o, k);
    if (d !== undefined) {
      if ('get' in d || 'set' in d) { if (d.set === undefined) return false; d.set.call(r, v); return true; }
      if (!d.writable) return false;
      break;
    }
    o = objGetProto(o);
  }
  if (!isObject(r)) return false;
  var rd = gopd(r, k);
  if (rd !== undefined) {
    if ('get' in rd || 'set' in rd || !rd.writable) return false;
    return reflectDefine(r, k, { value: v });
  }
  return reflectDefine(r, k, { value: v, writable: true, enumerable: true, configurable: true });
});
if (!Reflect.getOwnPropertyDescriptor) hide(Reflect, 'getOwnPropertyDescriptor', function getOwnPropertyDescriptor(t, k) {
  if (!isObject(t)) throw new TypeError('Reflect.getOwnPropertyDescriptor called on non-object'); return gopd(t, k);
});
if (!Reflect.isExtensible) hide(Reflect, 'isExtensible', function isExtensible(t) {
  if (!isObject(t)) throw new TypeError('Reflect.isExtensible called on non-object'); return isExt(t);
});
if (!Reflect.preventExtensions) hide(Reflect, 'preventExtensions', function preventExtensions(t) {
  if (!isObject(t)) throw new TypeError('Reflect.preventExtensions called on non-object'); Object.preventExtensions(t); return !isExt(t);
});

// ---- Symbol.species and the methods that honour it; the String methods
// that hand a pattern object to its Symbol.match / replace / search / split
[Array, Map, Set, RegExp].forEach(function (C) {
  defineProperty(C, Symbol.species, { get: speciesSelf, enumerable: false, configurable: true });
});
function setLength(f, n) { defineProperty(f, 'length', { value: n, writable: false, enumerable: false, configurable: true }); return f; }
/** ArraySpeciesCreate: null when a plain Array does. */
function arraySpecies(o, n) {
  if (!Array.isArray(o)) return null;
  var C = o.constructor;
  if (C === Array) return null;
  if (isObject(C)) { C = C[Symbol.species]; if (C === null) C = undefined; }
  if (C === undefined || C === Array) return null;
  if (!isCallable(C)) throw new TypeError('object.constructor[Symbol.species] is not a constructor');
  return new C(n);
}
function copyInto(A, r) {
  for (var i = 0; i < r.length; i++) defineProperty(A, i, { value: r[i], writable: true, enumerable: true, configurable: true });
  A.length = r.length;
  return A;
}
var AP = Array.prototype;
['filter', 'map', 'slice', 'splice', 'flat', 'flatMap'].forEach(function (name) {
  var nat = AP[name];
  var f = {
    filter: function filter(a, b) { var r = nat.apply(this, arguments); var A = arraySpecies(this, 0); return A === null ? r : copyInto(A, r); },
    map: function map(a, b) { var r = nat.apply(this, arguments); var A = arraySpecies(this, r.length); return A === null ? r : copyInto(A, r); },
    slice: function slice(a, b) { var r = nat.apply(this, arguments); var A = arraySpecies(this, r.length); return A === null ? r : copyInto(A, r); },
    splice: function splice(a, b) { var r = nat.apply(this, arguments); var A = arraySpecies(this, r.length); return A === null ? r : copyInto(A, r); },
    flat: function flat() { var r = nat.apply(this, arguments); var A = arraySpecies(this, 0); return A === null ? r : copyInto(A, r); },
    flatMap: function flatMap(a) { var r = nat.apply(this, arguments); var A = arraySpecies(this, 0); return A === null ? r : copyInto(A, r); }
  }[name];
  hide(AP, name, setLength(f, nat.length));
});
var nativeConcat = AP.concat;
function spreadable(o) {
  if (!isObject(o)) return false;
  var s = o[Symbol.isConcatSpreadable];
  return s !== undefined ? !!s : Array.isArray(o);
}
hide(AP, 'concat', setLength(function concat(x) {
  var px = isProxy(this);
  for (var i0 = 0; i0 < arguments.length && !px; i0++) px = isProxy(arguments[i0]);
  if (!px) {
    var plain = this.constructor === Array || !Array.isArray(this);
    var marked = isObject(this) && this[Symbol.isConcatSpreadable] !== undefined;
    for (var i = 0; i < arguments.length && !marked; i++) if (isObject(arguments[i]) && arguments[i][Symbol.isConcatSpreadable] !== undefined) marked = true;
    if (plain && !marked) return nativeConcat.apply(this, arguments);
  }
  var O = Object(this);
  var A = arraySpecies(O, 0);
  if (A === null) A = [];
  var n = 0, items = [O];
  for (var j = 0; j < arguments.length; j++) items.push(arguments[j]);
  for (var k = 0; k < items.length; k++) {
    var E = items[k];
    if (spreadable(E)) {
      var len = toLength(E.length);
      for (var m = 0; m < len; m++, n++) if (m in E) defineProperty(A, n, { value: E[m], writable: true, enumerable: true, configurable: true });
    } else {
      defineProperty(A, n++, { value: E, writable: true, enumerable: true, configurable: true });
    }
  }
  A.length = n;
  return A;
}, 1));
var nativeFrom = Array.from, nativeOf = Array.of;
hide(Array, 'from', setLength(function from(items, mapFn, thisArg) {
  var C = this;
  if (mapFn !== undefined && !isCallable(mapFn)) throw new TypeError(String(mapFn) + ' is not a function');
  var iter = items === undefined || items === null ? undefined : items[Symbol.iterator];
  if (C === Array && Array.isArray(items) && iter === AP.values && mapFn === undefined) return nativeFrom.call(Array, items);
  var ctor = isCallable(C) && C !== Array;
  var A, k = 0;
  if (iter !== undefined && iter !== null) {
    if (!isCallable(iter)) throw new TypeError('Symbol.iterator is not a function');
    A = ctor ? new C() : [];
    var it = iter.call(items);
    for (;;) {
      var r = it.next();
      if (r.done) break;
      var v = r.value;
      if (mapFn) {
        try { v = mapFn.call(thisArg, v, k); }
        catch (e) { if (isCallable(it.return)) { try { it.return(); } catch (e2) {} } throw e; }
      }
      defineProperty(A, k, { value: v, writable: true, enumerable: true, configurable: true });
      k++;
    }
    A.length = k;
    return A;
  }
  var src = Object(items), len = toLength(src.length);
  A = ctor ? new C(len) : new Array(len);
  for (; k < len; k++) {
    var x = mapFn ? mapFn.call(thisArg, src[k], k) : src[k];
    defineProperty(A, k, { value: x, writable: true, enumerable: true, configurable: true });
  }
  A.length = len;
  return A;
}, 1));
hide(Array, 'of', function of() {
  var C = this;
  if (C === Array || !isCallable(C)) return nativeOf.apply(Array, arguments);
  var A = new C(arguments.length);
  for (var i = 0; i < arguments.length; i++) defineProperty(A, i, { value: arguments[i], writable: true, enumerable: true, configurable: true });
  A.length = arguments.length;
  return A;
});

var RP = RegExp.prototype, SP = String.prototype;
var sMatch = SP.match, sReplace = SP.replace, sSearch = SP.search, sSplit = SP.split, sMatchAll = SP.matchAll, sReplaceAll = SP.replaceAll;
function needRegExp(r, what) { if (!isObject(r)) throw new TypeError('RegExp.prototype[' + what + '] called on incompatible receiver'); }

// ---- RegExp.prototype's Symbol methods on objects that are not RegExps
// (a proxy, an object with its own exec), in the specification's steps
var nativeExec = RP.exec;
function regExpExec(R, S) {
  var exec = R.exec;
  if (isCallable(exec)) {
    var r = exec.call(R, S);
    if (r !== null && !isObject(r)) throw new TypeError('exec result must be an object or null');
    return r;
  }
  if (!isRegExpObj(R)) throw new TypeError('RegExp.prototype.exec called on incompatible receiver');
  return nativeExec.call(R, S);
}
function advance(S, i, u) {
  if (!u || i + 1 >= S.length) return i + 1;
  var c = S.charCodeAt(i);
  if (c < 0xD800 || c > 0xDBFF) return i + 1;
  var d = S.charCodeAt(i + 1);
  return d >= 0xDC00 && d <= 0xDFFF ? i + 2 : i + 1;
}
function genMatch(R, S) {
  if (!R.global) return regExpExec(R, S);
  var u = !!R.unicode;
  R.lastIndex = 0;
  var A = [], n = 0;
  for (;;) {
    var r = regExpExec(R, S);
    if (r === null) return n === 0 ? null : A;
    var m = String(r[0]);
    A[n++] = m;
    if (m === '') R.lastIndex = advance(S, toLength(R.lastIndex), u);
  }
}
function genSearch(R, S) {
  var prev = R.lastIndex;
  if (!Object.is(prev, 0)) R.lastIndex = 0;
  var r = regExpExec(R, S);
  var cur = R.lastIndex;
  if (!Object.is(cur, prev)) R.lastIndex = prev;
  return r === null ? -1 : r.index;
}
function substitution(m, S, pos, caps, groups, t) {
  var out = '', tail = pos + m.length, n = caps.length;
  for (var i = 0; i < t.length; i++) {
    var c = t.charAt(i);
    if (c !== '$' || i + 1 >= t.length) { out += c; continue; }
    var d = t.charAt(i + 1);
    if (d === '$') { out += '$'; i++; }
    else if (d === '&') { out += m; i++; }
    else if (d === '`') { out += S.slice(0, pos); i++; }
    else if (d === "'") { out += tail >= S.length ? '' : S.slice(tail); i++; }
    else if (d >= '0' && d <= '9') {
      var k = +d, used = 1, e = t.charAt(i + 2);
      if (e >= '0' && e <= '9' && k * 10 + (+e) >= 1 && k * 10 + (+e) <= n) { k = k * 10 + (+e); used = 2; }
      if (k >= 1 && k <= n) { var cv = caps[k - 1]; out += cv === undefined ? '' : cv; i += used; }
      else out += '$';
    } else if (d === '<' && groups !== undefined) {
      var close = t.indexOf('>', i + 2);
      if (close < 0) out += '$';
      else { var gv = groups[t.slice(i + 2, close)]; out += gv === undefined ? '' : String(gv); i = close; }
    } else out += '$';
  }
  return out;
}
function genReplace(R, S, rep) {
  var fn = isCallable(rep);
  if (!fn) rep = String(rep);
  var global = !!R.global, u = false;
  if (global) { u = !!R.unicode; R.lastIndex = 0; }
  var results = [];
  for (;;) {
    var r = regExpExec(R, S);
    if (r === null) break;
    results.push(r);
    if (!global) break;
    if (String(r[0]) === '') R.lastIndex = advance(S, toLength(R.lastIndex), u);
  }
  var out = '', next = 0;
  for (var i = 0; i < results.length; i++) {
    var res = results[i];
    var nc = Math.max(toLength(res.length) - 1, 0);
    var matched = String(res[0]);
    var pos = Math.max(Math.min(toIntegerOrInfinity(res.index), S.length), 0);
    var caps = [];
    for (var j = 1; j <= nc; j++) { var cj = res[j]; caps.push(cj === undefined ? undefined : String(cj)); }
    var groups = res.groups, s;
    if (fn) {
      var args = [matched];
      for (var a = 0; a < caps.length; a++) args.push(caps[a]);
      args.push(pos, S);
      if (groups !== undefined) args.push(groups);
      s = String(rep.apply(undefined, args));
    } else {
      if (groups !== undefined) groups = Object(groups);
      s = substitution(matched, S, pos, caps, groups, rep);
    }
    if (pos >= next) { out += S.slice(next, pos) + s; next = pos + matched.length; }
  }
  return next >= S.length ? out : out + S.slice(next);
}
function genSplit(R, S, lim) {
  var C = speciesOf(R, RegExp);
  var flags = String(R.flags);
  var u = flags.indexOf('u') >= 0 || flags.indexOf('v') >= 0;
  var sp = new C(R, flags.indexOf('y') >= 0 ? flags : flags + 'y');
  var A = [], max = lim === undefined ? 4294967295 : lim >>> 0;
  if (max === 0) return A;
  var size = S.length;
  if (size === 0) { if (regExpExec(sp, S) === null) A.push(S); return A; }
  var p = 0, q = 0;
  while (q < size) {
    sp.lastIndex = q;
    var z = regExpExec(sp, S);
    if (z === null) { q = advance(S, q, u); continue; }
    var e = Math.min(toLength(sp.lastIndex), size);
    if (e === p) { q = advance(S, q, u); continue; }
    A.push(S.slice(p, q));
    if (A.length === max) return A;
    p = e;
    var nc = Math.max(toLength(z.length) - 1, 0);
    for (var i = 1; i <= nc; i++) { A.push(z[i]); if (A.length === max) return A; }
    q = p;
  }
  A.push(S.slice(p, size));
  return A;
}
hide(RP, Symbol.match, function (s) { needRegExp(this, 'Symbol.match'); if (!isRegExpObj(this)) return genMatch(this, String(s)); return sMatch.call(String(s), this); });
hide(RP, Symbol.matchAll, function (s) { needRegExp(this, 'Symbol.matchAll'); return sMatchAll.call(String(s), this); });
hide(RP, Symbol.replace, function (s, r) { needRegExp(this, 'Symbol.replace'); if (!isRegExpObj(this)) return genReplace(this, String(s), r); return sReplace.call(String(s), this, r); });
hide(RP, Symbol.search, function (s) { needRegExp(this, 'Symbol.search'); if (!isRegExpObj(this)) return genSearch(this, String(s)); return sSearch.call(String(s), this); });
var nativeTest = RP.test;
hide(RP, 'test', setLength(function test(s) {
  needRegExp(this, 'test');
  if (!isRegExpObj(this)) return regExpExec(this, String(s)) !== null;
  return nativeTest.call(this, s);
}, 1));
hide(RP, Symbol.split, function (s, lim) { needRegExp(this, 'Symbol.split'); return sSplit.call(String(s), this, lim); });
[['match', Symbol.match, sMatch, 1], ['matchAll', Symbol.matchAll, sMatchAll, 1], ['replace', Symbol.replace, sReplace, 2],
 ['replaceAll', Symbol.replaceAll, sReplaceAll, 2], ['search', Symbol.search, sSearch, 1], ['split', Symbol.split, sSplit, 2]].forEach(function (e) {
  var name = e[0], sym = e[1], nat = e[2];
  var f = function (x, y) {
    if (this === undefined || this === null) throw new TypeError('String.prototype.' + name + ' called on null or undefined');
    if (x !== undefined && x !== null && typeof x !== 'string' && !(x instanceof RegExp && getPrototypeOf(x) === RP)) {
      var m = x[name === 'replaceAll' ? Symbol.replace : sym];
      if (m !== undefined && m !== null) {
        if (name === 'replaceAll' && isObject(x) && x instanceof RegExp && String(x.flags).indexOf('g') < 0) throw new TypeError('replaceAll must be called with a global RegExp');
        return m.call(x, this, y);
      }
    }
    return nat.call(this, x, y);
  };
  defineProperty(f, 'name', { value: name, writable: false, enumerable: false, configurable: true });
  hide(SP, name, setLength(f, e[3]));
});
function isRegExpLike(x) {
  if (!isObject(x)) return false;
  var m = x[Symbol.match];
  if (m !== undefined) return !!m;
  return x instanceof RegExp;
}
['startsWith', 'endsWith', 'includes'].forEach(function (name) {
  var nat = SP[name];
  var f = function (s, pos) {
    if (isRegExpLike(s)) throw new TypeError('First argument to String.prototype.' + name + ' must not be a regular expression');
    return nat.call(this, isObject(s) ? String(s) : s, pos);
  };
  defineProperty(f, 'name', { value: name, writable: false, enumerable: false, configurable: true });
  hide(SP, name, setLength(f, 1));
});

// ---- smaller pieces: unscopables, Date's @@toPrimitive, Number.parseFloat,
// the Annex B String HTML methods, RegExp flags and RegExp.escape
var unscop = create(null);
['at', 'copyWithin', 'entries', 'fill', 'find', 'findIndex', 'findLast', 'findLastIndex', 'flat', 'flatMap', 'includes',
 'keys', 'toReversed', 'toSorted', 'toSpliced', 'values'].forEach(function (k) { unscop[k] = true; });
defineProperty(AP, Symbol.unscopables, { value: unscop, writable: false, enumerable: false, configurable: true });
defineProperty(Date.prototype, Symbol.toPrimitive, { value: function (hint) {
  if (!isObject(this)) throw new TypeError('Date.prototype[Symbol.toPrimitive] called on non-object');
  var order = hint === 'number' ? ['valueOf', 'toString'] : hint === 'string' || hint === 'default' ? ['toString', 'valueOf'] : null;
  if (order === null) throw new TypeError('Invalid hint: ' + String(hint));
  for (var i = 0; i < 2; i++) {
    var f = this[order[i]];
    if (isCallable(f)) { var r = f.call(this); if (!isObject(r)) return r; }
  }
  throw new TypeError('Cannot convert object to primitive value');
}, writable: false, enumerable: false, configurable: true });
hide(Number, 'parseFloat', parseFloat);
hide(Number, 'parseInt', parseInt);
function html(tag, attr) {
  return function (v) {
    if (this === undefined || this === null) throw new TypeError('String.prototype method called on null or undefined');
    var s = String(this), open = '<' + tag;
    if (attr) open += ' ' + attr + '="' + String(v).replace(/"/g, '&quot;') + '"';
    return open + '>' + s + '</' + tag + '>';
  };
}
[['anchor', 'a', 'name'], ['big', 'big'], ['blink', 'blink'], ['bold', 'b'], ['fixed', 'tt'], ['fontcolor', 'font', 'color'],
 ['fontsize', 'font', 'size'], ['italics', 'i'], ['link', 'a', 'href'], ['small', 'small'], ['strike', 'strike'], ['sub', 'sub'], ['sup', 'sup']].forEach(function (e) {
  var f = html(e[1], e[2]);
  defineProperty(f, 'name', { value: e[0], writable: false, enumerable: false, configurable: true });
  hide(SP, e[0], setLength(f, e[2] ? 1 : 0));
});
hide(Object, 'getOwnPropertyDescriptors', function getOwnPropertyDescriptors(o) {
  var O = Object(o), out = {}, keys = reflectOwnKeys(O);
  for (var i = 0; i < keys.length; i++) {
    var d = gopd(O, keys[i]);
    if (d !== undefined) defineProperty(out, keys[i], { value: d, writable: true, enumerable: true, configurable: true });
  }
  return out;
});
// includes on array-likes reads only what it looks at
var nativeIncludes = AP.includes;
hide(AP, 'includes', setLength(function includes(x, from) {
  if (Array.isArray(this)) return nativeIncludes.call(this, x, from);
  var O = Object(this), n = toLength(O.length);
  if (n === 0) return false;
  for (var i = relIndex(from, n, 0); i < n; i++) { var v = O[i]; if (v === x || (v !== v && x !== x)) return true; }
  return false;
}, 1));
var nativeFlags = gopd(RP, 'flags').get;
var flagNames = [['hasIndices', 'd'], ['global', 'g'], ['ignoreCase', 'i'], ['multiline', 'm'], ['dotAll', 's'], ['unicode', 'u'], ['unicodeSets', 'v'], ['sticky', 'y']];
flagNames.forEach(function (e) {
  if (gopd(RP, e[0]) !== undefined) return;
  getter(RP, e[0], function () {
    if (this === RP) return undefined;
    if (!(this instanceof RegExp)) throw new TypeError('RegExp.prototype.' + e[0] + ' getter called on non-RegExp');
    return String(nativeFlags.call(this)).indexOf(e[1]) >= 0;
  });
});
getter(RP, 'flags', function () {
  if (!isObject(this)) throw new TypeError('RegExp.prototype.flags getter called on non-object');
  var r = '';
  for (var i = 0; i < flagNames.length; i++) if (this[flagNames[i][0]]) r += flagNames[i][1];
  return r;
});
hide(RegExp, 'escape', function escape(s) {
  if (typeof s !== 'string') throw new TypeError('RegExp.escape requires a string');
  var out = '';
  for (var i = 0; i < s.length; i++) {
    var c = s.charAt(i), code = s.charCodeAt(i);
    if (i === 0 && /[0-9A-Za-z]/.test(c)) { out += '\\x' + code.toString(16); continue; }
    if ('^$\\.*+?()[]{}|/'.indexOf(c) >= 0) { out += '\\' + c; continue; }
    if (',-=<>#&!%:;@~\'`"'.indexOf(c) >= 0 || /[\t\n\v\f\r \u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff]/.test(c)) {
      var h = code.toString(16);
      out += code <= 0xff ? '\\x' + (h.length < 2 ? '0' + h : h) : '\\u' + ('0000' + h).slice(-4);
      continue;
    }
    out += c;
  }
  return out;
});
var reSplit = RP[Symbol.split];
hide(RP, Symbol.split, function (s, lim) {
  needRegExp(this, 'Symbol.split');
  if (!isRegExpObj(this)) return genSplit(this, String(s), lim);
  var C = speciesOf(this, RegExp);
  var rx = this;
  if (C !== RegExp) rx = new C(this, String(this.flags));
  return sSplit.call(String(s), rx instanceof RegExp ? rx : this, lim);
});
['__defineGetter__', '__defineSetter__', '__lookupGetter__', '__lookupSetter__'].forEach(function (name) {
  var nat = OP[name];
  hide(OP, name, setLength(function (a, b) {
    'use strict';
    if (this === undefined || this === null) throw new TypeError('Object.prototype.' + name + ' called on null or undefined');
    return nat.call(this, a, b);
  }, nat.length));
});

// ---- CreateAsyncFromSyncIterator, for `for await` over a sync iterable
var setAsyncFromSync = globalThis.__setAsyncFromSync; delete globalThis.__setAsyncFromSync;
var AsyncIteratorProto = getPrototypeOf(getPrototypeOf(getPrototypeOf((async function* () {})())));
setAsyncFromSync(function (sync) {
  var it = create(AsyncIteratorProto);
  function step(r) {
    if (!isObject(r)) throw new TypeError('Iterator result is not an object');
    return Promise.resolve(r.value).then(function (v) { return { value: v, done: !!r.done }; });
  }
  hide(it, 'next', function (v) { try { return step(sync.next(v)); } catch (e) { return Promise.reject(e); } });
  hide(it, 'return', function (v) {
    try { var m = sync.return; if (m === undefined || m === null) return Promise.resolve({ value: v, done: true }); return step(m.call(sync, v)); }
    catch (e) { return Promise.reject(e); }
  });
  hide(it, 'throw', function (v) {
    try { var m = sync.throw; if (m === undefined || m === null) throw v; return step(m.call(sync, v)); }
    catch (e) { return Promise.reject(e); }
  });
  return it;
});

// ---- the mutating Array methods on array-likes, as the specification
// writes them (arrays keep the native ones)
function genericOnly(name, len, f) {
  var nat = AP[name];
  var w = function () {
    'use strict';
    if (this === undefined || this === null) throw new TypeError('Array.prototype.' + name + ' called on null or undefined');
    return Array.isArray(this) && !isProxy(this) ? nat.apply(this, arguments) : f.apply(Object(this), arguments);
  };
  defineProperty(w, 'name', { value: name, writable: false, enumerable: false, configurable: true });
  hide(AP, name, setLength(w, len));
}
function moveTo(O, from, to) { if (from in O) O[to] = O[from]; else delete O[to]; }
genericOnly('reverse', 0, function () {
  var n = toLength(this.length);
  for (var lo = 0, hi = n - 1; lo < hi; lo++, hi--) {
    var hasLo = lo in this, a, b;
    if (hasLo) a = this[lo];
    var hasHi = hi in this;
    if (hasHi) b = this[hi];
    if (hasLo && hasHi) { this[lo] = b; this[hi] = a; }
    else if (hasHi) { this[lo] = b; delete this[hi]; }
    else if (hasLo) { delete this[lo]; this[hi] = a; }
  }
  return this;
});
genericOnly('copyWithin', 2, function (target, start, end) {
  var n = toLength(this.length);
  var to = relIndex(target, n, 0), from = relIndex(start, n, 0), fin = relIndex(end, n, n);
  var count = Math.min(fin - from, n - to), dir = 1;
  if (from < to && to < from + count) { dir = -1; from += count - 1; to += count - 1; }
  for (; count > 0; count--, from += dir, to += dir) moveTo(this, from, to);
  return this;
});
genericOnly('lastIndexOf', 1, function (x, fromIndex) {
  var n = toLength(this.length);
  if (n === 0) return -1;
  var k = arguments.length > 1 ? toIntegerOrInfinity(fromIndex) : n - 1;
  k = k >= 0 ? Math.min(k, n - 1) : n + k;
  for (; k >= 0; k--) if (k in this && this[k] === x) return k;
  return -1;
});
genericOnly('reduceRight', 1, function (f, init) {
  var n = toLength(this.length);
  need(f);
  var k = n - 1, acc;
  if (arguments.length > 1) acc = init;
  else {
    while (k >= 0 && !(k in this)) k--;
    if (k < 0) throw new TypeError('Reduce of empty array with no initial value');
    acc = this[k--];
  }
  for (; k >= 0; k--) if (k in this) acc = f(acc, this[k], k, this);
  return acc;
});
genericOnly('shift', 0, function () {
  var n = toLength(this.length);
  if (n === 0) { this.length = 0; return undefined; }
  var first = this[0];
  for (var k = 1; k < n; k++) moveTo(this, k, k - 1);
  delete this[n - 1];
  this.length = n - 1;
  return first;
});
genericOnly('unshift', 1, function () {
  var n = toLength(this.length), c = arguments.length;
  if (c > 0) {
    for (var k = n; k > 0; k--) moveTo(this, k - 1, k + c - 1);
    for (var j = 0; j < c; j++) this[j] = arguments[j];
  }
  this.length = n + c;
  return n + c;
});
genericOnly('splice', 2, function (start, count) {
  var n = toLength(this.length);
  var s = relIndex(start, n, 0);
  var del = arguments.length === 0 ? 0 : arguments.length === 1 ? n - s : Math.min(Math.max(toIntegerOrInfinity(count), 0), n - s);
  var removed = arraySpecies(this, del);
  if (removed === null) removed = [];
  for (var i = 0; i < del; i++) if ((s + i) in this) removed[i] = this[s + i];
  removed.length = del;
  var items = [];
  for (var a = 2; a < arguments.length; a++) items.push(arguments[a]);
  var c = items.length;
  if (c < del) {
    for (var k = s; k < n - del; k++) moveTo(this, k + del, k + c);
    for (var d = n; d > n - del + c; d--) delete this[d - 1];
  } else if (c > del) {
    for (var k2 = n - del; k2 > s; k2--) moveTo(this, k2 + del - 1, k2 + c - 1);
  }
  for (var j = 0; j < c; j++) this[s + j] = items[j];
  this.length = n - del + c;
  return removed;
});

// ---- Date string forms the built-ins lack (the time zone is UTC)
var DP = Date.prototype;
function toPrimitiveNumber(O) {
  var ex = O[Symbol.toPrimitive];
  if (ex !== undefined && ex !== null) {
    var r = ex.call(O, 'number');
    if (isObject(r)) throw new TypeError('Cannot convert object to primitive value');
    return r;
  }
  var names = ['valueOf', 'toString'];
  for (var i = 0; i < 2; i++) {
    var m = O[names[i]];
    if (isCallable(m)) { var v = m.call(O); if (!isObject(v)) return v; }
  }
  throw new TypeError('Cannot convert object to primitive value');
}
hide(DP, 'toJSON', setLength(function toJSON(key) {
  'use strict';
  if (this === undefined || this === null) throw new TypeError('Date.prototype.toJSON called on null or undefined');
  var O = Object(this);
  var tv = toPrimitiveNumber(O);
  if (typeof tv === 'number' && !isFinite(tv)) return null;
  var f = O.toISOString;
  if (!isCallable(f)) throw new TypeError('toISOString is not a function');
  return f.call(O);
}, 1));
var nativeBind = Function.prototype.bind;
hide(Function.prototype, 'bind', setLength(function bind(t) {
  if (!isProxy(this)) return nativeBind.apply(this, arguments);
  var L = 0;
  if (OP.hasOwnProperty.call(this, 'length')) {
    var tl = this.length;
    if (typeof tl === 'number') L = tl === Infinity ? Infinity : Math.max(0, toIntegerOrInfinity(tl) - Math.max(arguments.length - 1, 0));
  }
  var nm = this.name;
  if (typeof nm !== 'string') nm = '';
  var b = nativeBind.apply(this, arguments);
  defineProperty(b, 'length', { value: L, writable: false, enumerable: false, configurable: true });
  defineProperty(b, 'name', { value: 'bound ' + nm, writable: false, enumerable: false, configurable: true });
  return b;
}, 1));
var MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
var DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
function pad2(n) { return n < 10 ? '0' + n : String(n); }
function dateOk(d, what) {
  var t = DP.getTime.call(d);
  return t === t;
}
if (!DP.toDateString || DP.toDateString.call(new Date(0)).length > 15) hide(DP, 'toDateString', function toDateString() {
  if (!dateOk(this)) return 'Invalid Date';
  var y = this.getFullYear();
  return DAYS[this.getDay()] + ' ' + MONTHS[this.getMonth()] + ' ' + pad2(this.getDate()) + ' ' + (y < 0 ? '-' + ('00000' + -y).slice(-6) : ('000' + y).slice(-4));
});
if (!DP.toTimeString) hide(DP, 'toTimeString', function toTimeString() {
  if (!dateOk(this)) return 'Invalid Date';
  return pad2(this.getHours()) + ':' + pad2(this.getMinutes()) + ':' + pad2(this.getSeconds()) + ' GMT+0000 (Coordinated Universal Time)';
});
if (!DP.toLocaleDateString) hide(DP, 'toLocaleDateString', function toLocaleDateString() {
  if (!dateOk(this)) return 'Invalid Date';
  return (this.getMonth() + 1) + '/' + this.getDate() + '/' + this.getFullYear();
});
if (!DP.toLocaleTimeString) hide(DP, 'toLocaleTimeString', function toLocaleTimeString() {
  if (!dateOk(this)) return 'Invalid Date';
  var h = this.getHours();
  return ((h + 11) % 12 + 1) + ':' + pad2(this.getMinutes()) + ':' + pad2(this.getSeconds()) + (h < 12 ? ' AM' : ' PM');
});
if (!DP.toLocaleString) hide(DP, 'toLocaleString', function toLocaleString() {
  if (!dateOk(this)) return 'Invalid Date';
  return this.toLocaleDateString() + ', ' + this.toLocaleTimeString();
});

// ---- Intl (ECMA-402), without ICU: the locales in LOCALES (their number
// separators, plural rules, date order and names), English for any other.
// Collator compares as the root collation does at its first three levels
// (letters, then accents, then case, lower first); numeric digit runs.
var LOCALES = {
  en: { dec: '.', grp: ',', cur: 'before', date: 'mdy', dsep: '/', tsep: ':', h12: true, rel: 'en', list: ['and', 'or'], one: function (n, i, v) { return i === 1 && v === 0; } },
  fi: { dec: ',', grp: ' ', cur: 'after', date: 'dmy', dsep: '.', tsep: '.', h12: false, list: ['ja', 'tai'], one: function (n, i, v) { return i === 1 && v === 0; },
    months: ['tammikuuta', 'helmikuuta', 'maaliskuuta', 'huhtikuuta', 'toukokuuta', 'kesäkuuta', 'heinäkuuta', 'elokuuta', 'syyskuuta', 'lokakuuta', 'marraskuuta', 'joulukuuta'],
    monthsAlone: ['tammikuu', 'helmikuu', 'maaliskuu', 'huhtikuu', 'toukokuu', 'kesäkuu', 'heinäkuu', 'elokuu', 'syyskuu', 'lokakuu', 'marraskuu', 'joulukuu'],
    days: ['sunnuntai', 'maanantai', 'tiistai', 'keskiviikko', 'torstai', 'perjantai', 'lauantai'], dayDot: true },
  sv: { dec: ',', grp: ' ', cur: 'after', date: 'ymd', dsep: '-', tsep: ':', h12: false, list: ['och', 'eller'], one: function (n, i, v) { return i === 1 && v === 0; },
    months: ['januari', 'februari', 'mars', 'april', 'maj', 'juni', 'juli', 'augusti', 'september', 'oktober', 'november', 'december'],
    days: ['söndag', 'måndag', 'tisdag', 'onsdag', 'torsdag', 'fredag', 'lördag'] },
  de: { dec: ',', grp: '.', cur: 'after', date: 'dmy', dsep: '.', tsep: ':', h12: false, list: ['und', 'oder'], one: function (n, i, v) { return i === 1 && v === 0; },
    months: ['Januar', 'Februar', 'März', 'April', 'Mai', 'Juni', 'Juli', 'August', 'September', 'Oktober', 'November', 'Dezember'],
    days: ['Sonntag', 'Montag', 'Dienstag', 'Mittwoch', 'Donnerstag', 'Freitag', 'Samstag'], dayDot: true },
  fr: { dec: ',', grp: ' ', cur: 'after', date: 'dmy', dsep: '/', tsep: ':', h12: false, list: ['et', 'ou'], one: function (n, i, v) { return i === 0 || i === 1; },
    months: ['janvier', 'février', 'mars', 'avril', 'mai', 'juin', 'juillet', 'août', 'septembre', 'octobre', 'novembre', 'décembre'],
    days: ['dimanche', 'lundi', 'mardi', 'mercredi', 'jeudi', 'vendredi', 'samedi'] },
  nl: { dec: ',', grp: '.', cur: 'before', date: 'dmy', dsep: '-', tsep: ':', h12: false, list: ['en', 'of'], one: function (n, i, v) { return i === 1 && v === 0; },
    months: ['januari', 'februari', 'maart', 'april', 'mei', 'juni', 'juli', 'augustus', 'september', 'oktober', 'november', 'december'],
    days: ['zondag', 'maandag', 'dinsdag', 'woensdag', 'donderdag', 'vrijdag', 'zaterdag'] },
  es: { dec: ',', grp: '.', cur: 'after', date: 'dmy', dsep: '/', tsep: ':', h12: false, list: ['y', 'o'], one: function (n, i, v) { return n === 1; },
    months: ['enero', 'febrero', 'marzo', 'abril', 'mayo', 'junio', 'julio', 'agosto', 'septiembre', 'octubre', 'noviembre', 'diciembre'],
    days: ['domingo', 'lunes', 'martes', 'miércoles', 'jueves', 'viernes', 'sábado'] }
};
var EN_MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
var EN_DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
var CURRENCY = { USD: '$', EUR: '€', GBP: '£', JPY: '¥', SEK: 'kr', NOK: 'kr', DKK: 'kr', CHF: 'CHF', CNY: 'CN¥', INR: '₹' };
function canonLocale(tag) {
  if (typeof tag !== 'string' && !(isObject(tag) && typeof tag.toString === 'function')) throw new TypeError('Incorrect locale information provided');
  var s = String(tag);
  if (!/^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$/.test(s.replace(/_/g, '-'))) throw new RangeError('Incorrect locale information provided');
  var parts = s.replace(/_/g, '-').split('-');
  parts[0] = parts[0].toLowerCase();
  for (var i = 1; i < parts.length; i++) {
    if (parts[i].length === 2) parts[i] = parts[i].toUpperCase();
    else if (parts[i].length === 4) parts[i] = parts[i].charAt(0).toUpperCase() + parts[i].slice(1).toLowerCase();
    else parts[i] = parts[i].toLowerCase();
  }
  return parts.join('-');
}
function localeList(locales) {
  if (locales === undefined) return [];
  if (typeof locales === 'string') return [canonLocale(locales)];
  var out = [], o = Object(locales), n = toIntegerOrInfinity(o.length);
  for (var i = 0; i < n; i++) if (i in o) { var c = canonLocale(o[i]); if (out.indexOf(c) < 0) out.push(c); }
  return out;
}
function pickLocale(locales) {
  var list = localeList(locales);
  for (var i = 0; i < list.length; i++) if (LOCALES[list[i].split('-')[0]]) return list[i];
  return 'en-US';
}
function localeData(tag) { return LOCALES[tag.split('-')[0]] || LOCALES.en; }
function option(o, name, allowed, dflt) {
  var v = o[name];
  if (v === undefined) return dflt;
  v = typeof dflt === 'boolean' ? Boolean(v) : String(v);
  if (allowed && allowed.indexOf(v) < 0) throw new RangeError('Value ' + v + ' out of range for Intl options property ' + name);
  return v;
}
function numOption(o, name, min, max, dflt) {
  var v = o[name];
  if (v === undefined) return dflt;
  v = Number(v);
  if (v !== v || v < min || v > max) throw new RangeError(name + ' value is out of range.');
  return Math.floor(v);
}
function optionsObject(o) { return o === undefined ? create(null) : Object(o); }
function supportedLocalesOf(locales) { return localeList(locales).filter(function (l) { return !!LOCALES[l.split('-')[0]]; }); }
function intlBrand(o, brand, what) { if (!isObject(o) || o.__intl !== brand) throw new TypeError('Method ' + what + ' called on incompatible receiver'); }

// numbers: digits rounded to the fraction range, grouped by the locale
function groupDigits(int, sep, minGroup) {
  if (int.length < 4 + (minGroup || 0)) return int;
  var out = '';
  for (var i = int.length; i > 0; i -= 3) out = int.slice(Math.max(0, i - 3), i) + (out ? sep + out : '');
  return out;
}
function fixedDigits(x, minF, maxF) {
  var s = x.toFixed(maxF);
  if (s.indexOf('e') >= 0) s = x.toLocaleString === undefined ? String(x) : String(x);
  var dot = s.indexOf('.');
  if (dot >= 0) {
    var end = s.length;
    while (end > dot + 1 + minF && s.charAt(end - 1) === '0') end--;
    if (end === dot + 1) end = dot;
    s = s.slice(0, end);
  }
  return s;
}
function NumberFormat(locales, options) {
  if (!(this instanceof NumberFormat) || this.__intl) return new NumberFormat(locales, options);
  var o = optionsObject(options);
  var loc = pickLocale(locales);
  var style = option(o, 'style', ['decimal', 'percent', 'currency', 'unit'], 'decimal');
  var currency = o.currency === undefined ? undefined : String(o.currency).toUpperCase();
  if (style === 'currency' && currency === undefined) throw new TypeError('Currency code is required with currency style.');
  var cd = currency === 'JPY' ? 0 : 2;
  var minF = numOption(o, 'minimumFractionDigits', 0, 100, undefined);
  var maxF = numOption(o, 'maximumFractionDigits', 0, 100, undefined);
  var dMin = style === 'currency' ? cd : 0, dMax = style === 'currency' ? cd : style === 'percent' ? 0 : 3;
  if (minF === undefined && maxF === undefined) { minF = dMin; maxF = dMax; }
  else if (minF === undefined) minF = Math.min(dMin, maxF);
  else if (maxF === undefined) maxF = Math.max(dMax, minF);
  else if (minF > maxF) throw new RangeError('maximumFractionDigits value is out of range.');
  var g = o.useGrouping;
  hide(this, '__intl', 'NumberFormat');
  hide(this, '__o', {
    locale: loc, numberingSystem: 'latn', style: style, currency: currency,
    currencyDisplay: option(o, 'currencyDisplay', ['symbol', 'narrowSymbol', 'code', 'name'], 'symbol'),
    unit: o.unit === undefined ? undefined : String(o.unit), unitDisplay: option(o, 'unitDisplay', ['short', 'narrow', 'long'], 'short'),
    minimumIntegerDigits: numOption(o, 'minimumIntegerDigits', 1, 21, 1),
    minimumFractionDigits: minF, maximumFractionDigits: maxF,
    maximumSignificantDigits: numOption(o, 'maximumSignificantDigits', 1, 21, undefined),
    useGrouping: g === undefined ? 'auto' : g === false || g === 'false' ? false : g === 'min2' ? 'min2' : 'auto',
    notation: option(o, 'notation', ['standard', 'scientific', 'engineering', 'compact'], 'standard'),
    signDisplay: option(o, 'signDisplay', ['auto', 'never', 'always', 'exceptZero', 'negative'], 'auto')
  });
}
function numberParts(nf, x) {
  var o = nf.__o, d = localeData(o.locale), parts = [];
  if (typeof x === 'bigint') x = Number(x);
  x = Number(x);
  var neg = x < 0 || (x === 0 && 1 / x < 0);
  var sign = '';
  if (o.signDisplay === 'always' || (o.signDisplay === 'exceptZero' && x !== 0)) sign = neg ? '-' : '+';
  else if (o.signDisplay !== 'never' && neg && !(o.signDisplay === 'negative' && x === 0) && !(o.signDisplay === 'auto' && x === 0 && false)) sign = '-';
  var a = Math.abs(x);
  if (o.style === 'percent') a *= 100;
  var body;
  if (a !== a) body = [{ type: 'nan', value: 'NaN' }];
  else if (a === Infinity) body = [{ type: 'infinity', value: '∞' }];
  else {
    var s;
    if (o.maximumSignificantDigits) s = String(Number(a.toPrecision(o.maximumSignificantDigits)));
    else s = fixedDigits(a, o.minimumFractionDigits, o.maximumFractionDigits);
    var dot = s.indexOf('.');
    var int = dot < 0 ? s : s.slice(0, dot), frac = dot < 0 ? '' : s.slice(dot + 1);
    while (int.length < o.minimumIntegerDigits) int = '0' + int;
    body = [];
    if (o.useGrouping === false) body.push({ type: 'integer', value: int });
    else {
      var grouped = groupDigits(int, '\u0001', o.useGrouping === 'min2' || d.grp === ' ' && false ? 1 : 0).split('\u0001');
      for (var i = 0; i < grouped.length; i++) {
        if (i) body.push({ type: 'group', value: d.grp });
        body.push({ type: 'integer', value: grouped[i] });
      }
    }
    if (frac) { body.push({ type: 'decimal', value: d.dec }); body.push({ type: 'fraction', value: frac }); }
  }
  if (sign) parts.push({ type: sign === '-' ? 'minusSign' : 'plusSign', value: sign === '-' && d.dec === ',' && o.locale.split('-')[0] !== 'de' && o.locale.split('-')[0] !== 'nl' && o.locale.split('-')[0] !== 'es' ? '−' : sign });
  if (o.style === 'currency') {
    var sym = o.currencyDisplay === 'code' ? o.currency : o.currencyDisplay === 'name' ? o.currency : CURRENCY[o.currency] || o.currency;
    if (d.cur === 'before') { parts.push({ type: 'currency', value: sym }); if (sym.length > 1 && /^[A-Z]+$/.test(sym)) parts.push({ type: 'literal', value: ' ' }); parts = parts.concat(body); }
    else { parts = parts.concat(body); parts.push({ type: 'literal', value: ' ' }); parts.push({ type: 'currency', value: sym }); }
  } else parts = parts.concat(body);
  if (o.style === 'percent') { if (d.dec === ',') parts.push({ type: 'literal', value: ' ' }); parts.push({ type: 'percentSign', value: '%' }); }
  if (o.style === 'unit' && o.unit) parts.push({ type: 'literal', value: ' ' }, { type: 'unit', value: o.unit });
  return parts;
}
hide(NumberFormat.prototype, 'formatToParts', function formatToParts(x) { intlBrand(this, 'NumberFormat', 'Intl.NumberFormat.prototype.formatToParts'); return numberParts(this, x); });
defineProperty(NumberFormat.prototype, 'format', { configurable: true, enumerable: false, get: function () {
  intlBrand(this, 'NumberFormat', 'get Intl.NumberFormat.prototype.format');
  var self = this;
  if (!this.__f) hide(this, '__f', function (x) { return numberParts(self, x).map(function (p) { return p.value; }).join(''); });
  return this.__f;
} });
hide(NumberFormat.prototype, 'resolvedOptions', function resolvedOptions() {
  intlBrand(this, 'NumberFormat', 'Intl.NumberFormat.prototype.resolvedOptions');
  var r = {}, o = this.__o;
  for (var k in o) if (o[k] !== undefined) r[k] = o[k];
  return r;
});

// plural rules: the locale's `one`, and English ordinals
function PluralRules(locales, options) {
  if (!(this instanceof PluralRules)) throw new TypeError("Constructor Intl.PluralRules requires 'new'");
  var o = optionsObject(options);
  hide(this, '__intl', 'PluralRules');
  hide(this, '__o', { locale: pickLocale(locales), type: option(o, 'type', ['cardinal', 'ordinal'], 'cardinal'), minimumIntegerDigits: 1, minimumFractionDigits: 0, maximumFractionDigits: 3, pluralCategories: [] });
  this.__o.pluralCategories = this.__o.type === 'ordinal' && this.__o.locale.split('-')[0] === 'en' ? ['few', 'one', 'two', 'other'] : this.__o.type === 'ordinal' ? ['other'] : ['one', 'other'];
}
hide(PluralRules.prototype, 'select', function select(n) {
  intlBrand(this, 'PluralRules', 'Intl.PluralRules.prototype.select');
  n = Number(n);
  var o = this.__o, lang = o.locale.split('-')[0];
  if (n !== n || !isFinite(n)) return 'other';
  var a = Math.abs(n), i = Math.floor(a), v = a === i ? 0 : String(a).split('.')[1].length;
  if (o.type === 'ordinal') {
    if (lang !== 'en') return 'other';
    var m10 = i % 10, m100 = i % 100;
    return m10 === 1 && m100 !== 11 ? 'one' : m10 === 2 && m100 !== 12 ? 'two' : m10 === 3 && m100 !== 13 ? 'few' : 'other';
  }
  return localeData(o.locale).one(a, i, v) ? 'one' : 'other';
});
hide(PluralRules.prototype, 'resolvedOptions', function resolvedOptions() { intlBrand(this, 'PluralRules', 'Intl.PluralRules.prototype.resolvedOptions'); var r = {}; for (var k in this.__o) r[k] = this.__o[k]; r.pluralCategories = this.__o.pluralCategories.slice(); return r; });

// dates: the fields asked for, in the locale's order, local time or UTC
var DT_FIELDS = ['weekday', 'era', 'year', 'month', 'day', 'hour', 'minute', 'second'];
function DateTimeFormat(locales, options) {
  if (!(this instanceof DateTimeFormat) || this.__intl) return new DateTimeFormat(locales, options);
  var o = optionsObject(options);
  var loc = pickLocale(locales);
  var tz = o.timeZone === undefined ? undefined : String(o.timeZone);
  if (tz !== undefined) {
    var up = tz.toUpperCase();
    if (up === 'UTC' || up === 'GMT' || up === 'ETC/UTC' || up === 'ETC/GMT') tz = 'UTC';
    else if (!/^[A-Za-z_]+(\/[A-Za-z_+\-0-9]+)*$/.test(tz)) throw new RangeError('Invalid time zone specified: ' + tz);
  }
  var r = { locale: loc, calendar: 'gregory', numberingSystem: 'latn', timeZone: tz || 'UTC' };
  var any = false;
  var vals = { weekday: ['narrow', 'short', 'long'], era: ['narrow', 'short', 'long'], year: ['2-digit', 'numeric'], month: ['2-digit', 'numeric', 'narrow', 'short', 'long'], day: ['2-digit', 'numeric'], hour: ['2-digit', 'numeric'], minute: ['2-digit', 'numeric'], second: ['2-digit', 'numeric'] };
  for (var i = 0; i < DT_FIELDS.length; i++) { var f = DT_FIELDS[i]; var v = option(o, f, vals[f], undefined); if (v !== undefined) { r[f] = v; any = true; } }
  var ds = option(o, 'dateStyle', ['full', 'long', 'medium', 'short'], undefined), ts = option(o, 'timeStyle', ['full', 'long', 'medium', 'short'], undefined);
  if ((ds || ts) && any) throw new TypeError("Can't set option " + 'dateStyle' + ' when other date/time options are set');
  if (ds) { r.dateStyle = ds; r.year = 'numeric'; r.month = ds === 'short' ? 'numeric' : ds === 'medium' ? 'short' : 'long'; r.day = 'numeric'; if (ds === 'full') r.weekday = 'long'; }
  if (ts) { r.timeStyle = ts; r.hour = 'numeric'; r.minute = '2-digit'; if (ts !== 'short') r.second = '2-digit'; }
  var dflt = o.__dflt || 'date';
  if (!any && !ds && !ts) {
    if (dflt === 'date' || dflt === 'all') { r.year = 'numeric'; r.month = 'numeric'; r.day = 'numeric'; }
    if (dflt === 'time' || dflt === 'all') { r.hour = 'numeric'; r.minute = '2-digit'; r.second = '2-digit'; }
  }
  var h12 = o.hour12 !== undefined ? Boolean(o.hour12) : o.hourCycle !== undefined ? (o.hourCycle === 'h12' || o.hourCycle === 'h11') : localeData(loc).h12 && loc !== 'en-GB';
  if (r.hour) { r.hour12 = h12; r.hourCycle = h12 ? 'h12' : 'h23'; }
  hide(this, '__intl', 'DateTimeFormat');
  hide(this, '__o', r);
  hide(this, '__utc', tz === 'UTC' || tz === undefined && false);
  hide(this, '__tz', tz);
}
function two(n) { return n < 10 ? '0' + n : String(n); }
function dateParts(dtf, date) {
  var t = date === undefined ? Date.now() : typeof date === 'number' ? date : DP.getTime.call(date instanceof Date ? date : new Date(date));
  if (t !== t) throw new RangeError('Invalid time value');
  var d = new Date(t), o = dtf.__o, utc = dtf.__tz === 'UTC';
  var Y = utc ? d.getUTCFullYear() : d.getFullYear(), M = utc ? d.getUTCMonth() : d.getMonth(), D = utc ? d.getUTCDate() : d.getDate();
  var W = utc ? d.getUTCDay() : d.getDay(), h = utc ? d.getUTCHours() : d.getHours(), mi = utc ? d.getUTCMinutes() : d.getMinutes(), s = utc ? d.getUTCSeconds() : d.getSeconds();
  var loc = o.locale, lang = loc.split('-')[0], ld = localeData(loc);
  var gb = loc === 'en-GB' || loc === 'en-AU' || loc === 'en-NZ' || loc === 'en-IE';
  var order = lang === 'en' && gb ? 'dmy' : ld.date;
  var textMonth = o.month === 'long' || o.month === 'short' || o.month === 'narrow';
  var months = lang === 'en' || !ld.months ? EN_MONTHS : (o.day ? ld.months : ld.monthsAlone || ld.months);
  var days = lang === 'en' || !ld.days ? EN_DAYS : ld.days;
  var P = [];
  function lit(v) { if (P.length && P[P.length - 1].type === 'literal') P[P.length - 1].value += v; else P.push({ type: 'literal', value: v }); }
  function field(type, v) { P.push({ type: type, value: v }); }
  var year = o.year === '2-digit' ? two(Y % 100) : String(Y);
  var day = o.day === '2-digit' ? two(D) : String(D);
  var month = textMonth ? (o.month === 'long' ? months[M] : o.month === 'narrow' ? months[M].charAt(0).toUpperCase() : (lang === 'en' ? months[M].slice(0, 3) : months[M].slice(0, 3) + (lang === 'fi' ? 'k.' : lang === 'de' ? '.' : '.'))) : o.month === '2-digit' ? two(M + 1) : String(M + 1);
  var hasDate = o.year || o.month || o.day;
  if (o.weekday) {
    var w = o.weekday === 'long' ? days[W] : o.weekday === 'short' ? days[W].slice(0, lang === 'fi' ? 2 : 3) : days[W].charAt(0).toUpperCase();
    field('weekday', w);
    if (hasDate) lit(', ');
  }
  if (hasDate) {
    var seq = [];
    if (textMonth) {
      if (lang === 'en' && !gb) seq = [['month', month], [' '], ['day', day], [', '], ['year', year]];
      else seq = [['day', day], [ld.dayDot ? '. ' : ' '], ['month', month], [' '], ['year', year]];
    } else {
      var map = { d: ['day', day], m: ['month', month], y: ['year', year] };
      var sep = lang === 'en' && gb ? '/' : ld.dsep;
      for (var k = 0; k < 3; k++) { if (k) seq.push([sep]); seq.push(map[order.charAt(k)]); }
      if (lang === 'sv' && !o.day && o.year && o.month) seq = [['year', year], ['-'], ['month', two(M + 1)]];
    }
    // the fields asked for, and the separators between those kept
    var kept = [];
    for (var q = 0; q < seq.length; q++) if (seq[q].length === 1 || o[seq[q][0]]) kept.push(seq[q]);
    var clean = [];
    for (var z = 0; z < kept.length; z++) {
      if (kept[z].length === 1 && (!clean.length || clean[clean.length - 1].length === 1)) continue;
      clean.push(kept[z]);
    }
    while (clean.length && clean[clean.length - 1].length === 1) clean.pop();
    for (var c = 0; c < clean.length; c++) { if (clean[c].length === 1) lit(clean[c][0]); else field(clean[c][0], clean[c][1]); }
  }
  if (o.hour || o.minute || o.second) {
    if (hasDate || o.weekday) lit(lang === 'fi' ? ' klo ' : lang === 'en' ? ', ' : ' ');
    var hh = o.hour12 ? (h + 11) % 12 + 1 : h;
    var tsep = ld.tsep;
    if (o.hour) field('hour', o.hour === '2-digit' || (!o.hour12 && o.minute && lang !== 'fi') ? two(hh) : String(hh));
    if (o.minute) { if (o.hour) lit(tsep); field('minute', o.hour || o.minute === '2-digit' ? two(mi) : String(mi)); }
    if (o.second) { if (o.hour || o.minute) lit(tsep); field('second', o.hour || o.minute || o.second === '2-digit' ? two(s) : String(s)); }
    if (o.hour && o.hour12) { lit(' '); field('dayPeriod', h < 12 ? 'AM' : 'PM'); }
  }
  return P;
}
hide(DateTimeFormat.prototype, 'formatToParts', function formatToParts(d) { intlBrand(this, 'DateTimeFormat', 'Intl.DateTimeFormat.prototype.formatToParts'); return dateParts(this, d); });
defineProperty(DateTimeFormat.prototype, 'format', { configurable: true, enumerable: false, get: function () {
  intlBrand(this, 'DateTimeFormat', 'get Intl.DateTimeFormat.prototype.format');
  var self = this;
  if (!this.__f) hide(this, '__f', function (d) { return dateParts(self, d).map(function (p) { return p.value; }).join(''); });
  return this.__f;
} });
hide(DateTimeFormat.prototype, 'formatRange', function formatRange(a, b) { return this.format(a) + ' – ' + this.format(b); });
hide(DateTimeFormat.prototype, 'resolvedOptions', function resolvedOptions() { intlBrand(this, 'DateTimeFormat', 'Intl.DateTimeFormat.prototype.resolvedOptions'); var r = {}; for (var k in this.__o) r[k] = this.__o[k]; return r; });

// collation: letters (accents and case folded away), then accents, then case
function collationKeys(s, numeric, ignorePunct) {
  var d = String(s).normalize('NFD');
  var base = '', marks = '', cases = '';
  for (var i = 0; i < d.length; i++) {
    var ch = d.charAt(i);
    if (/[̀-ͯ]/.test(ch)) { marks = marks.slice(0, -1) + ch; continue; }
    if (ignorePunct && /[\s!-\/:-@\[-`{-~]/.test(ch)) continue;
    var low = ch.toLowerCase();
    base += low;
    marks += '\u0000';
    cases += low === ch ? '0' : '1';
  }
  return [base, marks, cases];
}
function compareBase(a, b, numeric) {
  if (!numeric) return a < b ? -1 : a > b ? 1 : 0;
  var re = /(\d+)|(\D+)/g, x = a.match(re) || [], y = b.match(re) || [];
  for (var i = 0; i < x.length && i < y.length; i++) {
    var p = x[i], q = y[i];
    var pn = /^\d/.test(p), qn = /^\d/.test(q);
    if (pn && qn) { var dn = Number(p) - Number(q); if (dn) return dn < 0 ? -1 : 1; continue; }
    if (p !== q) return p < q ? -1 : 1;
  }
  return x.length - y.length < 0 ? -1 : x.length > y.length ? 1 : 0;
}
function Collator(locales, options) {
  if (!(this instanceof Collator) || this.__intl) return new Collator(locales, options);
  var o = optionsObject(options);
  hide(this, '__intl', 'Collator');
  hide(this, '__o', {
    locale: pickLocale(locales), usage: option(o, 'usage', ['sort', 'search'], 'sort'),
    sensitivity: option(o, 'sensitivity', ['base', 'accent', 'case', 'variant'], 'variant'),
    ignorePunctuation: option(o, 'ignorePunctuation', null, false), collation: 'default',
    numeric: option(o, 'numeric', null, false), caseFirst: option(o, 'caseFirst', ['upper', 'lower', 'false'], 'false')
  });
}
function collate(c, a, b) {
  var o = c.__o;
  var x = collationKeys(a, o.numeric, o.ignorePunctuation), y = collationKeys(b, o.numeric, o.ignorePunctuation);
  var r = compareBase(x[0], y[0], o.numeric);
  if (r || o.sensitivity === 'base') return r;
  if (o.sensitivity !== 'case') { r = x[1] < y[1] ? -1 : x[1] > y[1] ? 1 : 0; if (r) return r; }
  if (o.sensitivity === 'accent') return 0;
  r = x[2] < y[2] ? -1 : x[2] > y[2] ? 1 : 0;
  if (o.caseFirst === 'upper') r = -r;
  return r;
}
defineProperty(Collator.prototype, 'compare', { configurable: true, enumerable: false, get: function () {
  intlBrand(this, 'Collator', 'get Intl.Collator.prototype.compare');
  var self = this;
  if (!this.__f) hide(this, '__f', function (a, b) { return collate(self, a, b); });
  return this.__f;
} });
hide(Collator.prototype, 'resolvedOptions', function resolvedOptions() { intlBrand(this, 'Collator', 'Intl.Collator.prototype.resolvedOptions'); var r = {}; for (var k in this.__o) r[k] = this.__o[k]; return r; });

// relative times and lists (English, and the lists' conjunctions)
var REL_UNITS = ['year', 'quarter', 'month', 'week', 'day', 'hour', 'minute', 'second'];
function RelativeTimeFormat(locales, options) {
  if (!(this instanceof RelativeTimeFormat)) throw new TypeError("Constructor Intl.RelativeTimeFormat requires 'new'");
  var o = optionsObject(options);
  hide(this, '__intl', 'RelativeTimeFormat');
  hide(this, '__o', { locale: pickLocale(locales), style: option(o, 'style', ['long', 'short', 'narrow'], 'long'), numeric: option(o, 'numeric', ['always', 'auto'], 'always'), numberingSystem: 'latn' });
}
hide(RelativeTimeFormat.prototype, 'format', function format(v, unit) {
  intlBrand(this, 'RelativeTimeFormat', 'Intl.RelativeTimeFormat.prototype.format');
  v = Number(v);
  var u = String(unit).replace(/s$/, '');
  if (REL_UNITS.indexOf(u) < 0) throw new RangeError('Invalid unit argument for format() \'' + unit + '\'');
  if (this.__o.numeric === 'auto') {
    if (u === 'day' && v === 0) return 'today';
    if (u === 'day' && v === 1) return 'tomorrow';
    if (u === 'day' && v === -1) return 'yesterday';
    if (v === 0) return 'this ' + u;
    if (v === 1 && u !== 'hour' && u !== 'minute' && u !== 'second') return 'next ' + u;
    if (v === -1 && u !== 'hour' && u !== 'minute' && u !== 'second') return 'last ' + u;
  }
  var a = Math.abs(v), n = new NumberFormat(this.__o.locale).format(a) + ' ' + u + (a === 1 ? '' : 's');
  return v < 0 || (v === 0 && 1 / v < 0) ? n + ' ago' : 'in ' + n;
});
hide(RelativeTimeFormat.prototype, 'resolvedOptions', function resolvedOptions() { var r = {}; for (var k in this.__o) r[k] = this.__o[k]; return r; });
function ListFormat(locales, options) {
  if (!(this instanceof ListFormat)) throw new TypeError("Constructor Intl.ListFormat requires 'new'");
  var o = optionsObject(options);
  hide(this, '__intl', 'ListFormat');
  hide(this, '__o', { locale: pickLocale(locales), type: option(o, 'type', ['conjunction', 'disjunction', 'unit'], 'conjunction'), style: option(o, 'style', ['long', 'short', 'narrow'], 'long') });
}
hide(ListFormat.prototype, 'format', function format(list) {
  intlBrand(this, 'ListFormat', 'Intl.ListFormat.prototype.format');
  var items = [];
  for (var x of list) { if (typeof x !== 'string') throw new TypeError('Iterable yielded ' + String(x) + ' which is not a string'); items.push(x); }
  var o = this.__o, ld = localeData(o.locale), lang = o.locale.split('-')[0];
  if (items.length < 2) return items.join('');
  if (o.type === 'unit') return items.join(o.style === 'narrow' ? ' ' : ', ');
  var word = o.type === 'disjunction' ? ld.list[1] : ld.list[0];
  if (o.type === 'conjunction' && o.style !== 'long' && lang === 'en') word = o.style === 'short' ? '&' : '';
  var head = items.slice(0, -1).join(', ');
  var last = items[items.length - 1];
  if (lang === 'en' && word === '') return head + ', ' + last;
  return head + (lang === 'en' && items.length > 2 ? ', ' : ' ') + word + ' ' + last;
});
hide(ListFormat.prototype, 'resolvedOptions', function resolvedOptions() { var r = {}; for (var k in this.__o) r[k] = this.__o[k]; return r; });

var Intl = {};
[['NumberFormat', NumberFormat], ['PluralRules', PluralRules], ['DateTimeFormat', DateTimeFormat], ['Collator', Collator], ['RelativeTimeFormat', RelativeTimeFormat], ['ListFormat', ListFormat]].forEach(function (e) {
  hide(e[1], 'supportedLocalesOf', function supportedLocalesOf_(l) { return supportedLocalesOf(l); });
  tag(e[1].prototype, 'Intl.' + e[0]);
  hide(Intl, e[0], e[1]);
});
hide(Intl, 'getCanonicalLocales', function getCanonicalLocales(l) { return localeList(l); });
hide(Intl, 'supportedValuesOf', function supportedValuesOf(k) { if (k === 'currency') return Object.keys(CURRENCY).sort(); if (k === 'timeZone') return ['UTC']; return []; });
tag(Intl, 'Intl');
hide(globalThis, 'Intl', Intl);
// the toLocale… methods with locales or options go through Intl
var nativeNumLocale = Number.prototype.toLocaleString;
hide(Number.prototype, 'toLocaleString', function toLocaleString(locales, options) {
  var x = typeof this === 'number' ? this : Number.prototype.valueOf.call(this);
  return new NumberFormat(locales, options).format(x);
});
hide(String.prototype, 'localeCompare', function localeCompare(that, locales, options) {
  if (this === null || this === undefined) throw new TypeError('String.prototype.localeCompare called on null or undefined');
  return collate(new Collator(locales, options), String(this), String(that));
});
function dateLocale(d, locales, options, dflt) {
  if (!dateOk(d)) return 'Invalid Date';
  var o = {};
  if (options !== undefined) { var src = Object(options); for (var k in src) o[k] = src[k]; }
  o.__dflt = dflt;
  var has = false;
  for (var i = 0; i < DT_FIELDS.length; i++) if (o[DT_FIELDS[i]] !== undefined) has = true;
  if (o.dateStyle !== undefined || o.timeStyle !== undefined) has = true;
  if (!has && dflt === 'all') { o.year = 'numeric'; o.month = 'numeric'; o.day = 'numeric'; o.hour = 'numeric'; o.minute = '2-digit'; o.second = '2-digit'; }
  else if (!has && dflt === 'time') { o.hour = 'numeric'; o.minute = '2-digit'; o.second = '2-digit'; }
  return new DateTimeFormat(locales, o).format(d);
}
hide(DP, 'toLocaleString', function toLocaleString(locales, options) { return dateLocale(this, locales, options, 'all'); });
hide(DP, 'toLocaleDateString', function toLocaleDateString(locales, options) { return dateLocale(this, locales, options, 'date'); });
hide(DP, 'toLocaleTimeString', function toLocaleTimeString(locales, options) { return dateLocale(this, locales, options, 'time'); });

// ---- WeakRef / FinalizationRegistry (ES2021): the collector never runs
// a callback, so a held object simply stays
function WeakRef(target) {
  if (new.target === undefined) throw new TypeError("Constructor WeakRef requires 'new'");
  if (!isObject(target)) throw new TypeError('WeakRef: invalid target');
  var t = target;
  hide(this, 'deref', function deref() { return t; });
}
tag(WeakRef.prototype, 'WeakRef');
hide(globalThis, 'WeakRef', WeakRef);
function FinalizationRegistry(cleanup) {
  if (new.target === undefined) throw new TypeError("Constructor FinalizationRegistry requires 'new'");
  if (!isCallable(cleanup)) throw new TypeError('FinalizationRegistry: cleanup must be callable');
}
hide(FinalizationRegistry.prototype, 'register', function register(target, held, token) {
  if (!isObject(target)) throw new TypeError('FinalizationRegistry.prototype.register: invalid target');
});
hide(FinalizationRegistry.prototype, 'unregister', function unregister(token) { return false; });
tag(FinalizationRegistry.prototype, 'FinalizationRegistry');
hide(globalThis, 'FinalizationRegistry', FinalizationRegistry);
})();"#;
