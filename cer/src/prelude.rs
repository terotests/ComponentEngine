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
