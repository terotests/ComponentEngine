// SPDX-License-Identifier: AGPL-3.0-or-later
//! Built-ins written in JavaScript: the ones that are only other built-ins
//! put together (Set methods, iterator helpers, `groupBy`, the Annex B
//! accessor methods, `Promise.allSettled` / `any` / `withResolvers`, …).
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
function isPromise(x) { return isObject(x) && x instanceof Promise; }
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
  var plain = this.constructor === Array || !Array.isArray(this);
  var marked = isObject(this) && this[Symbol.isConcatSpreadable] !== undefined;
  for (var i = 0; i < arguments.length && !marked; i++) if (isObject(arguments[i]) && arguments[i][Symbol.isConcatSpreadable] !== undefined) marked = true;
  if (plain && !marked) return nativeConcat.apply(this, arguments);
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
hide(RP, Symbol.match, function (s) { needRegExp(this, 'Symbol.match'); return sMatch.call(String(s), this); });
hide(RP, Symbol.matchAll, function (s) { needRegExp(this, 'Symbol.matchAll'); return sMatchAll.call(String(s), this); });
hide(RP, Symbol.replace, function (s, r) { needRegExp(this, 'Symbol.replace'); return sReplace.call(String(s), this, r); });
hide(RP, Symbol.search, function (s) { needRegExp(this, 'Symbol.search'); return sSearch.call(String(s), this); });
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
  var C = speciesOf(this, RegExp);
  var rx = this;
  if (C !== RegExp) rx = new C(this, String(this.flags));
  return sSplit.call(String(s), rx instanceof RegExp ? rx : this, lim);
});
['__defineGetter__', '__defineSetter__', '__lookupGetter__', '__lookupSetter__'].forEach(function (name) {
  var nat = OP[name];
  hide(OP, name, setLength(function (a, b) {
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
