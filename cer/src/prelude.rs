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
var asMethod = globalThis.__asMethod;
delete globalThis.__asMethod;
var isConstructor = globalThis.__isConstructor;
delete globalThis.__isConstructor;
function hide(o, name, v) {
  if (typeof v === 'function' && o !== globalThis && !(typeof name === 'string' && name.charCodeAt(0) >= 65 && name.charCodeAt(0) <= 90)) asMethod(v);
  defineProperty(o, name, { value: v, writable: true, enumerable: false, configurable: true });
}
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

// ---- Set methods (ES2025), as the specification writes them: the
// argument is any set-like (size, has, keys); the receiver's own data is
// read through the original native methods, never its overridable ones
var SetProto = Set.prototype;
var setSizeGet = Object.getOwnPropertyDescriptor(SetProto, 'size').get;
var setHasN = SetProto.has, setAddN = SetProto.add, setDelN = SetProto['delete'], setEachN = SetProto.forEach;
function setRecord(o) {
  if (!isObject(o)) throw new TypeError('The argument must be a set-like object');
  var raw = o.size;
  var num = +raw;
  if (num !== num) throw new TypeError("The 'size' property must be a number");
  var size = toIntegerOrInfinity(num);
  if (size < 0) throw new RangeError("The 'size' property must not be negative");
  var has = o.has;
  if (!isCallable(has)) throw new TypeError("The 'has' property must be a function");
  var keys = o.keys;
  if (!isCallable(keys)) throw new TypeError("The 'keys' property must be a function");
  return { obj: o, size: size, has: has, keys: keys };
}
function setKeys(rec) {
  var it = rec.keys.call(rec.obj);
  if (!isObject(it)) throw new TypeError('keys() result is not an object');
  return { iter: it, next: it.next };
}
function setStep(kr) {
  var r = kr.next.call(kr.iter);
  if (!isObject(r)) throw new TypeError('Iterator result ' + String(r) + ' is not an object');
  return r;
}
function setClose(kr) {
  var m = kr.iter['return'];
  if (m === undefined || m === null) return;
  var r = m.call(kr.iter);
  if (!isObject(r)) throw new TypeError('Iterator result ' + String(r) + ' is not an object');
}
function setCopy(o) {
  var out = new Set();
  setEachN.call(o, function (v) { setAddN.call(out, v); });
  return out;
}
// each element of the receiver, as a live index walk; f returning true stops
function setEach(o, f) {
  var stop = false;
  setEachN.call(o, function (v) { if (!stop && f(v) === true) stop = true; });
  return stop;
}
function canon(v) { return v === 0 ? 0 : v; }
hide(SetProto, 'union', function union(other) {
  setSizeGet.call(this);
  var rec = setRecord(other), kr = setKeys(rec), out = setCopy(this);
  for (;;) { var r = setStep(kr); if (r.done) break; setAddN.call(out, canon(r.value)); }
  return out;
});
hide(SetProto, 'intersection', function intersection(other) {
  var self = this, size = setSizeGet.call(this);
  var rec = setRecord(other), out = new Set();
  if (size <= rec.size) {
    setEach(this, function (v) { if (rec.has.call(rec.obj, v)) setAddN.call(out, v); });
  } else {
    var kr = setKeys(rec);
    for (;;) { var r = setStep(kr); if (r.done) break; var v = canon(r.value); if (setHasN.call(self, v)) setAddN.call(out, v); }
  }
  return out;
});
hide(SetProto, 'difference', function difference(other) {
  var size = setSizeGet.call(this);
  var rec = setRecord(other), out = setCopy(this);
  if (size <= rec.size) {
    setEach(this, function (v) { if (rec.has.call(rec.obj, v)) setDelN.call(out, v); });
  } else {
    var kr = setKeys(rec);
    for (;;) { var r = setStep(kr); if (r.done) break; setDelN.call(out, canon(r.value)); }
  }
  return out;
});
hide(SetProto, 'symmetricDifference', function symmetricDifference(other) {
  setSizeGet.call(this);
  var rec = setRecord(other), kr = setKeys(rec), out = setCopy(this);
  for (;;) {
    var r = setStep(kr); if (r.done) break;
    var v = canon(r.value), inThis = setHasN.call(this, v), inOut = setHasN.call(out, v);
    if (inThis) { if (inOut) setDelN.call(out, v); }
    else if (!inOut) setAddN.call(out, v);
  }
  return out;
});
hide(SetProto, 'isSubsetOf', function isSubsetOf(other) {
  var size = setSizeGet.call(this);
  var rec = setRecord(other);
  if (size > rec.size) return false;
  return !setEach(this, function (v) { return !rec.has.call(rec.obj, v); });
});
hide(SetProto, 'isSupersetOf', function isSupersetOf(other) {
  var size = setSizeGet.call(this);
  var rec = setRecord(other);
  if (size < rec.size) return false;
  var kr = setKeys(rec);
  for (;;) {
    var r = setStep(kr); if (r.done) return true;
    if (!setHasN.call(this, r.value)) { setClose(kr); return false; }
  }
});
hide(SetProto, 'isDisjointFrom', function isDisjointFrom(other) {
  var size = setSizeGet.call(this);
  var rec = setRecord(other);
  if (size <= rec.size) {
    return !setEach(this, function (v) { return !!rec.has.call(rec.obj, v); });
  }
  var kr = setKeys(rec);
  for (;;) {
    var r = setStep(kr); if (r.done) return true;
    if (setHasN.call(this, r.value)) { setClose(kr); return false; }
  }
});

// ---- Iterator (ES2025): the prototype every built-in iterator shares, and
// its helpers
var ArrayIteratorProto = getPrototypeOf([][Symbol.iterator]());
var IteratorProto = getPrototypeOf(ArrayIteratorProto);
function Iterator() {
  if (new.target === undefined || new.target === Iterator) throw new TypeError('Abstract class Iterator not directly constructable');
}
defineProperty(Iterator, 'prototype', { value: IteratorProto, writable: false, enumerable: false, configurable: false });
hide(globalThis, 'Iterator', Iterator);

(function () {
'use strict';
// the helpers as the specification writes them: an Iterator Helper object
// runs a generator over the underlying iterator record; a return closes
// what is open
var HelperProto = create(IteratorProto);
tag(HelperProto, 'Iterator Helper');
var helperSlots = new WeakMap();
function iterDirect(o) {
  if (!isObject(o)) throw new TypeError(String(o) + ' is not an object');
  return { iter: o, next: o.next };
}
function stepRec(rec) {
  var r = rec.next.call(rec.iter);
  if (!isObject(r)) throw new TypeError('Iterator result ' + String(r) + ' is not an object');
  return r;
}
function closeNormal(o) {
  var m = o.return;
  if (m === undefined || m === null) return;
  if (!isCallable(m)) throw new TypeError('iterator.return is not a function');
  var r = m.call(o);
  if (!isObject(r)) throw new TypeError('Iterator result ' + String(r) + ' is not an object');
}
function closeThrow(o, e) {
  try { var m = o.return; if (m !== undefined && m !== null) m.call(o); } catch (x) {}
  throw e;
}
function makeHelper(under, gen) {
  var h = create(HelperProto);
  helperSlots.set(h, { gen: gen, started: false, under: under });
  return h;
}
function helperSlot(h, name) {
  var s = isObject(h) ? helperSlots.get(h) : undefined;
  if (s === undefined) throw new TypeError('Iterator Helper.prototype.' + name + ' called on incompatible receiver');
  return s;
}
hide(HelperProto, 'next', function next() {
  var s = helperSlot(this, 'next'); s.started = true; return s.gen.next();
});
hide(HelperProto, 'return', function () {
  var s = helperSlot(this, 'return');
  if (!s.started) {
    s.started = true; s.gen.return(undefined);
    for (var i = 0; i < s.under.length; i++) closeNormal(s.under[i]);
    return { value: undefined, done: true };
  }
  return s.gen.return(undefined);
});
function thisIter(o, name) {
  if (!isObject(o)) throw new TypeError('Iterator.prototype.' + name + ' called on non-object');
  return o;
}
function needOrClose(f, o) {
  if (!isCallable(f)) closeThrow(o, new TypeError(String(f) + ' is not a function'));
}
// yields v; a return at the yield closes the open iterators (last first)
function* yieldOrClose(v, a, b) {
  var resumed = false;
  try { yield v; resumed = true; }
  finally { if (!resumed) { if (b) { try { closeNormal(b); } catch (e) { closeThrow(a, e); } } closeNormal(a); } }
}
hide(IteratorProto, 'map', function map(f) {
  var o = thisIter(this, 'map'); needOrClose(f, o); var rec = iterDirect(o);
  return makeHelper([o], (function* () {
    for (var c = 0; ; c++) {
      var r = stepRec(rec); if (r.done) return; var v = r.value, m;
      try { m = f(v, c); } catch (e) { closeThrow(o, e); }
      yield* yieldOrClose(m, o);
    }
  })());
});
hide(IteratorProto, 'filter', function filter(f) {
  var o = thisIter(this, 'filter'); needOrClose(f, o); var rec = iterDirect(o);
  return makeHelper([o], (function* () {
    for (var c = 0; ; c++) {
      var r = stepRec(rec); if (r.done) return; var v = r.value, keep;
      try { keep = f(v, c); } catch (e) { closeThrow(o, e); }
      if (keep) yield* yieldOrClose(v, o);
    }
  })());
});
function limitOf(n, o) {
  var v;
  try { v = Number(n); } catch (e) { closeThrow(o, e); }
  if (v !== v) closeThrow(o, new RangeError(String(n) + ' must be positive'));
  v = toIntegerOrInfinity(v);
  if (v < 0 || (v !== Infinity && v > 9007199254740991)) closeThrow(o, new RangeError(String(n) + ' must be positive'));
  return v;
}
hide(IteratorProto, 'take', function take(n) {
  var o = thisIter(this, 'take'); var left = limitOf(n, o); var rec = iterDirect(o);
  return makeHelper([o], (function* () {
    for (;;) {
      if (left === 0) { closeNormal(o); return; }
      if (left !== Infinity) left--;
      var r = stepRec(rec); if (r.done) return;
      yield* yieldOrClose(r.value, o);
    }
  })());
});
hide(IteratorProto, 'drop', function drop(n) {
  var o = thisIter(this, 'drop'); var left = limitOf(n, o); var rec = iterDirect(o);
  return makeHelper([o], (function* () {
    while (left > 0) { if (left !== Infinity) left--; var s = stepRec(rec); if (s.done) return; }
    for (;;) { var r = stepRec(rec); if (r.done) return; yield* yieldOrClose(r.value, o); }
  })());
});
function flattenable(m, strings) {
  if (!isObject(m) && !(strings && typeof m === 'string')) throw new TypeError(String(m) + ' is not an object');
  var f = m[Symbol.iterator], it;
  if (f === undefined || f === null) it = m;
  else { it = f.call(m); if (!isObject(it)) throw new TypeError(String(it) + ' is not an object'); }
  return iterDirect(it);
}
hide(IteratorProto, 'flatMap', function flatMap(f) {
  var o = thisIter(this, 'flatMap'); needOrClose(f, o); var rec = iterDirect(o);
  return makeHelper([o], (function* () {
    for (var c = 0; ; c++) {
      var r = stepRec(rec); if (r.done) return; var v = r.value, inner;
      try { inner = flattenable(f(v, c), false); } catch (e) { closeThrow(o, e); }
      for (;;) {
        var ir;
        try { ir = stepRec(inner); } catch (e) { closeThrow(o, e); }
        if (ir.done) break;
        yield* yieldOrClose(ir.value, o, inner.iter);
      }
    }
  })());
});
function consume(o, name, f, each) {
  thisIter(o, name); needOrClose(f, o); var rec = iterDirect(o);
  for (var c = 0; ; c++) {
    var r = stepRec(rec); if (r.done) return undefined;
    var v = r.value, out;
    try { out = each(v, c); } catch (e) { closeThrow(o, e); }
    if (out !== undefined) { closeNormal(o); return out.v; }
  }
}
hide(IteratorProto, 'reduce', function reduce(f) {
  var o = thisIter(this, 'reduce'); needOrClose(f, o); var rec = iterDirect(o), acc, c = 0, r;
  if (arguments.length < 2) {
    r = stepRec(rec); if (r.done) throw new TypeError('Reduce of empty iterator with no initial value');
    acc = r.value; c = 1;
  } else acc = arguments[1];
  for (;; c++) {
    r = stepRec(rec); if (r.done) return acc; var v = r.value;
    try { acc = f(acc, v, c); } catch (e) { closeThrow(o, e); }
  }
});
hide(IteratorProto, 'toArray', function toArray() {
  var o = thisIter(this, 'toArray'), rec = iterDirect(o), out = [];
  for (;;) { var r = stepRec(rec); if (r.done) return out; out.push(r.value); }
});
hide(IteratorProto, 'forEach', function forEach(f) {
  consume(this, 'forEach', f, function (v, c) { f(v, c); });
});
hide(IteratorProto, 'some', function some(f) {
  var r = consume(this, 'some', f, function (v, c) { return f(v, c) ? { v: true } : undefined; });
  return r === true;
});
hide(IteratorProto, 'every', function every(f) {
  var r = consume(this, 'every', f, function (v, c) { return f(v, c) ? undefined : { v: false }; });
  return r !== false;
});
hide(IteratorProto, 'find', function find(f) {
  return consume(this, 'find', f, function (v, c) { return f(v, c) ? { v: v } : undefined; });
});
// Iterator.prototype's constructor and Symbol.toStringTag: accessors whose
// setters leave Iterator.prototype alone (SetterThatIgnoresPrototypeProperties)
function ignoringSetter(p) {
  return function (v) {
    if (!isObject(this)) throw new TypeError('Cannot set property on a non-object');
    if (this === IteratorProto) throw new TypeError('Cannot assign to read only property of Iterator.prototype');
    if (Object.getOwnPropertyDescriptor(this, p) === undefined) createDataPropertyOrThrow(this, p, v);
    else this[p] = v;
  };
}
defineProperty(IteratorProto, Symbol.toStringTag, { get: asMethod(function () { return 'Iterator'; }), set: asMethod(ignoringSetter(Symbol.toStringTag)), enumerable: false, configurable: true });
defineProperty(IteratorProto, 'constructor', { get: asMethod(function () { return Iterator; }), set: asMethod(ignoringSetter('constructor')), enumerable: false, configurable: true });
if (typeof Symbol.dispose === 'symbol') hide(IteratorProto, Symbol.dispose, defineProperty(function () {
  var m = this.return; if (m !== undefined && m !== null) m.call(this);
  return undefined;
}, 'name', { value: '[Symbol.dispose]', configurable: true }));
var WrapProto = create(IteratorProto);
var wrapSlots = new WeakMap();
function wrapSlot(w) {
  var s = isObject(w) ? wrapSlots.get(w) : undefined;
  if (s === undefined) throw new TypeError('incompatible receiver');
  return s;
}
hide(WrapProto, 'next', function next() { var s = wrapSlot(this); return s.next.call(s.iter); });
hide(WrapProto, 'return', function () {
  var s = wrapSlot(this), m = s.iter.return;
  if (m === undefined || m === null) return { value: undefined, done: true };
  if (!isCallable(m)) throw new TypeError('return is not a function');
  return m.call(s.iter);
});
// ---- Iterator.prototype.includes / chunks / windows / join (proposals)
function intArg(o, n, name, lo, hi) {
  if (typeof n !== 'number' || n !== n) closeThrow(o, new TypeError(name + ' must be a number'));
  if (n === Infinity || n === -Infinity || Math.floor(n) !== n) closeThrow(o, new TypeError(name + ' must be an integer'));
  if (n < lo || n > hi) closeThrow(o, new RangeError(name + ' is out of range'));
  return n;
}
hide(IteratorProto, 'includes', function includes(searchElement) {
  var o = thisIter(this, 'includes');
  var skip = arguments.length > 1 ? arguments[1] : undefined, toSkip = 0;
  if (skip !== undefined) {
    if (skip === Infinity) toSkip = Infinity; else if (skip === -Infinity) closeThrow(o, new RangeError('skippedElements is out of range'));
    else toSkip = intArg(o, skip, 'skippedElements', 0, 9007199254740991);
  }
  var rec = iterDirect(o), skipped = 0;
  for (;;) {
    var r = stepRec(rec); if (r.done) return false;
    var v = r.value;
    if (skipped < toSkip) { skipped++; continue; }
    if (v === searchElement || (v !== v && searchElement !== searchElement)) { closeNormal(o); return true; }
  }
});
hide(IteratorProto, 'chunks', function chunks(chunkSize) {
  var o = thisIter(this, 'chunks');
  var size = intArg(o, chunkSize, 'chunkSize', 1, 4294967295);
  var rec = iterDirect(o);
  return makeHelper([o], (function* () {
    var buf = [];
    for (;;) {
      var r = stepRec(rec);
      if (r.done) { if (buf.length > 0) yield buf; return; }
      buf.push(r.value);
      if (buf.length === size) { var out = buf; buf = []; yield* yieldOrClose(out, o); }
    }
  })());
});
hide(IteratorProto, 'windows', function windows(windowSize) {
  var o = thisIter(this, 'windows');
  var size = intArg(o, windowSize, 'windowSize', 1, 4294967295);
  var undersized = arguments.length > 1 ? arguments[1] : undefined;
  if (undersized === undefined) undersized = 'only-full';
  if (undersized !== 'only-full' && undersized !== 'allow-partial') closeThrow(o, new TypeError('undersized must be "only-full" or "allow-partial"'));
  var rec = iterDirect(o);
  return makeHelper([o], (function* () {
    var buf = [];
    for (;;) {
      var r = stepRec(rec);
      if (r.done) { if (undersized === 'allow-partial' && buf.length > 0 && buf.length < size) yield buf.slice(); return; }
      buf.push(r.value);
      if (buf.length > size) buf.shift();
      if (buf.length === size) yield* yieldOrClose(buf.slice(), o);
    }
  })());
});
hide(IteratorProto, 'join', function join(separator) {
  var o = thisIter(this, 'join');
  var sep = ',';
  if (separator !== undefined) { try { sep = String(separator); if (typeof separator === 'symbol') throw new TypeError('Cannot convert a Symbol value to a string'); } catch (e) { closeThrow(o, e); } }
  var rec = iterDirect(o), out = '', first = true;
  for (;;) {
    var r = stepRec(rec); if (r.done) return out;
    var v = r.value, t;
    if (v === undefined || v === null) t = '';
    else { try { if (typeof v === 'symbol') throw new TypeError('Cannot convert a Symbol value to a string'); t = String(v); } catch (e) { closeThrow(o, e); } }
    out = first ? t : out + sep + t;
    first = false;
  }
});
// ---- Iterator.zip / zipKeyed (joint iteration)
// IteratorCloseAll: the records' iterators, last first; a throw stands
function closeAll(recs, thrown, err) {
  for (var i = recs.length - 1; i >= 0; i--) {
    var o = recs[i].iter;
    if (thrown) { try { var m = o.return; if (m !== undefined && m !== null) m.call(o); } catch (x) {} }
    else { try { closeNormal(o); } catch (x) { thrown = true; err = x; } }
  }
  if (thrown) throw err;
}
function dropRec(list, rec) { var i = list.indexOf(rec); if (i >= 0) list.splice(i, 1); }
function getIterRec(o) {
  var f = o[Symbol.iterator];
  if (!isCallable(f)) throw new TypeError(String(f) + ' is not a function');
  var it = f.call(o);
  if (!isObject(it)) throw new TypeError(String(it) + ' is not an object');
  return iterDirect(it);
}
function zipMode(options) {
  if (options === undefined) return { mode: 'shortest', opts: undefined };
  if (!isObject(options)) throw new TypeError('options must be an object');
  var mode = options.mode;
  if (mode === undefined) mode = 'shortest';
  if (mode !== 'shortest' && mode !== 'longest' && mode !== 'strict') throw new TypeError('mode must be "shortest", "longest" or "strict"');
  var pad;
  if (mode === 'longest') {
    pad = options.padding;
    if (pad !== undefined && !isObject(pad)) throw new TypeError('padding must be an object');
  }
  return { mode: mode, pad: pad };
}
function zipCore(iters, mode, padding, finish) {
  var open = iters.slice();
  var n = iters.length;
  var gen = (function* () {
    if (n === 0) return;
    var cur = iters.slice();
    for (;;) {
      var results = [];
      for (var i = 0; i < n; i++) {
        var rec = cur[i], v, done = false;
        if (rec === null) v = padding[i];
        else {
          try { var r = stepRec(rec); done = !!r.done; if (!done) v = r.value; }
          catch (e) { dropRec(open, rec); closeAll(open, true, e); }
          if (done) {
            dropRec(open, rec);
            if (mode === 'shortest') { closeAll(open, false); return; }
            if (mode === 'strict') {
              if (i !== 0) closeAll(open, true, new TypeError('Iterator.zip: the iterators have different lengths'));
              for (var k = 1; k < n; k++) {
                var o2 = cur[k], d2 = false;
                try { d2 = !!stepRec(o2).done; } catch (e) { dropRec(open, o2); closeAll(open, true, e); }
                if (!d2) closeAll(open, true, new TypeError('Iterator.zip: the iterators have different lengths'));
                dropRec(open, o2);
              }
              return;
            }
            if (open.length === 0) return;
            cur[i] = null; v = padding[i];
          }
        }
        results.push(v);
      }
      var out = finish(results);
      var resumed = false;
      try { yield out; resumed = true; } finally { if (!resumed) closeAll(open, false); }
    }
  })();
  return makeHelper(iters.slice().reverse().map(function (r) { return r.iter; }), gen);
}
hide(Iterator, 'zip', function zip(iterables, options) {
  if (!isObject(iterables)) throw new TypeError('Iterator.zip requires an object');
  var m = zipMode(options);
  var iters = [];
  var input = getIterRec(iterables);
  for (;;) {
    var r, v;
    try { r = stepRec(input); if (r.done) break; v = r.value; } catch (e) { closeAll(iters, true, e); }
    var it;
    try { it = flattenable(v, false); } catch (e) { closeAll([input].concat(iters), true, e); }
    iters.push(it);
  }
  var n = iters.length, padding = [];
  if (m.mode === 'longest') {
    if (m.pad === undefined) { for (var i = 0; i < n; i++) padding.push(undefined); }
    else {
      var pit;
      try { pit = getIterRec(m.pad); } catch (e) { closeAll(iters, true, e); }
      var using = true;
      for (var j = 0; j < n; j++) {
        if (using) {
          var pr;
          try { pr = stepRec(pit); var pd = !!pr.done; var pv = pd ? undefined : pr.value; } catch (e) { closeAll(iters, true, e); }
          if (pd) using = false; else padding.push(pv);
        }
        if (!using) padding.push(undefined);
      }
      if (using) { try { closeNormal(pit.iter); } catch (e) { closeAll(iters, true, e); } }
    }
  }
  return zipCore(iters, m.mode, padding, function (results) { return results; });
});
defineProperty(Iterator.zip, 'length', { value: 1, configurable: true });
hide(Iterator, 'zipKeyed', function zipKeyed(iterables, options) {
  if (!isObject(iterables)) throw new TypeError('Iterator.zipKeyed requires an object');
  var m = zipMode(options);
  var iters = [], keys = [];
  var all = Reflect.ownKeys(iterables);
  for (var i = 0; i < all.length; i++) {
    var key = all[i], value;
    try {
      var desc = Reflect.getOwnPropertyDescriptor(iterables, key);
      if (desc === undefined || !desc.enumerable) continue;
      value = iterables[key];
      if (value === undefined) continue;
      var it = flattenable(value, false);
    } catch (e) { closeAll(iters, true, e); }
    keys.push(key); iters.push(it);
  }
  var padding = [];
  if (m.mode === 'longest') {
    for (var j = 0; j < keys.length; j++) {
      var pv;
      if (m.pad !== undefined) { try { pv = m.pad[keys[j]]; } catch (e) { closeAll(iters, true, e); } }
      padding.push(pv);
    }
  }
  return zipCore(iters, m.mode, padding, function (results) {
    var o = create(null);
    for (var k = 0; k < keys.length; k++) defineProperty(o, keys[k], { value: results[k], writable: true, enumerable: true, configurable: true });
    return o;
  });
});
defineProperty(Iterator.zipKeyed, 'length', { value: 1, configurable: true });
hide(Iterator, 'from', function from(o) {
  var rec = flattenable(o, true);
  if (rec.iter instanceof Iterator) return rec.iter;
  var w = create(WrapProto); wrapSlots.set(w, rec); return w;
});
hide(Iterator, 'concat', function concat() {
  var items = [];
  for (var i = 0; i < arguments.length; i++) {
    var it = arguments[i];
    if (!isObject(it)) throw new TypeError(String(it) + ' is not an object');
    var m = it[Symbol.iterator];
    if (!isCallable(m)) throw new TypeError(String(m) + ' is not a function');
    items.push({ open: m, iterable: it });
  }
  return makeHelper([], (function* () {
    for (var k = 0; k < items.length; k++) {
      var x = items[k].open.call(items[k].iterable);
      if (!isObject(x)) throw new TypeError(String(x) + ' is not an object');
      var rec = iterDirect(x);
      for (;;) {
        var r = stepRec(rec); if (r.done) break;
        yield* yieldOrClose(r.value, x);
      }
    }
  })());
});
})();

// ---- groupBy (ES2024)
function need(f) { if (!isCallable(f)) throw new TypeError(String(f) + ' is not a function'); }
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
getter(Promise, Symbol.species, function () { return this; });
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
// Promise.allKeyed / allSettledKeyed (Await Dictionary): an object's own
// enumerable values awaited, the result a null-prototype object of the keys
function awaitKeyed(C, promises, settled) {
  var cap = capability(C);
  try {
    var resolveFn = C.resolve; if (!isCallable(resolveFn)) throw new TypeError('Promise.resolve is not a function');
    if (!isObject(promises)) throw new TypeError('Promise.' + (settled ? 'allSettledKeyed' : 'allKeyed') + ' requires an object');
    var allKeys = Reflect.ownKeys(promises);
    var keys = [], values = [], left = 1;
    var finish = function () {
      var out = create(null);
      for (var j = 0; j < keys.length; j++) defineProperty(out, keys[j], { value: values[j], writable: true, enumerable: true, configurable: true });
      cap.resolve(out);
    };
    for (var i = 0; i < allKeys.length; i++) {
      var key = allKeys[i];
      var desc = Reflect.getOwnPropertyDescriptor(promises, key);
      if (desc === undefined || !desc.enumerable) continue;
      var value = promises[key];
      var k = keys.length;
      keys.push(key); values.push(undefined);
      left++;
      var next = resolveFn.call(C, value);
      (function (k) {
        var called = false;
        var onOk = settled
          ? (x) => { if (called) return; called = true; var r = create(Object.prototype); r.status = 'fulfilled'; r.value = x; values[k] = r; if (--left === 0) finish(); }
          : (x) => { if (called) return; called = true; values[k] = x; if (--left === 0) finish(); };
        var onErr = settled
          ? (e) => { if (called) return; called = true; var r = create(Object.prototype); r.status = 'rejected'; r.reason = e; values[k] = r; if (--left === 0) finish(); }
          : cap.reject;
        next.then(onOk, onErr);
      })(k);
    }
    if (--left === 0) finish();
  } catch (e) { cap.reject(e); }
  return cap.promise;
}
hide(Promise, 'allKeyed', function allKeyed(promises) { return awaitKeyed(this, promises, false); });
hide(Promise, 'allSettledKeyed', function allSettledKeyed(promises) { return awaitKeyed(this, promises, true); });
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

// ---- Array.prototype, generic (the spec's steps): the native methods
// call these for a receiver that is not a plain array (an array-like, a
// proxy, an array with element properties or a species constructor)
var setArrayGeneric = globalThis.__setArrayGeneric; delete globalThis.__setArrayGeneric;
(function () {
  'use strict';
  var MAX = 9007199254740991;
  var isArray = Array.isArray;
  function toObj(v) { if (v === null || v === undefined) throw new TypeError('Array.prototype method called on null or undefined'); return Object(v); }
  function toLength(v) { var n = toIntegerOrInfinity(v); return n <= 0 ? 0 : n > MAX ? MAX : n; }
  function lenOf(o) { return toLength(o.length); }
  function relIndex(v, len, dflt) {
    if (v === undefined) return dflt;
    var n = toIntegerOrInfinity(v);
    return n < 0 ? (n === -Infinity ? 0 : Math.max(len + n, 0)) : Math.min(n, len);
  }
  function fnArg(f) { if (typeof f !== 'function') throw new TypeError(typeof f === 'object' && f !== null ? 'object is not a function' : String(f) + ' is not a function'); }
  function arrayCreate(n) { if (n > 4294967295) throw new RangeError('Invalid array length'); return new Array(n); }
  function speciesCreate(o, n) {
    if (!isArray(o)) return arrayCreate(n);
    var C = o.constructor;
    if (isObject(C)) { C = C[Symbol.species]; if (C === null) C = undefined; }
    if (C === undefined) return arrayCreate(n);
    if (!isConstructor(C)) throw new TypeError('object.constructor[Symbol.species] is not a constructor');
    return new C(n);
  }
  function put(o, k, v) { o[k] = v; }
  function move(o, from, to) { if (from in o) o[to] = o[from]; else delete o[to]; }
  function sameValueZero(x, y) { return x === y || (x !== x && y !== y); }
  function str(v) { return `${v}`; }
  var fns = [];
  fns[0] = function push() {
    var o = toObj(this), len = lenOf(o), n = arguments.length;
    if (len + n > MAX) throw new TypeError('Pushing ' + n + ' elements on an array-like of length ' + len + ' is disallowed');
    for (var i = 0; i < n; i++) o[len++] = arguments[i];
    o.length = len; return len;
  };
  fns[1] = function pop() {
    var o = toObj(this), len = lenOf(o);
    if (len === 0) { o.length = 0; return undefined; }
    var i = len - 1, v = o[i]; delete o[i]; o.length = i; return v;
  };
  fns[2] = function shift() {
    var o = toObj(this), len = lenOf(o);
    if (len === 0) { o.length = 0; return undefined; }
    var first = o[0];
    for (var k = 1; k < len; k++) move(o, k, k - 1);
    delete o[len - 1]; o.length = len - 1; return first;
  };
  fns[3] = function unshift() {
    var o = toObj(this), len = lenOf(o), n = arguments.length;
    if (n > 0) {
      if (len + n > MAX) throw new TypeError('Unshift would exceed the maximum array-like length');
      for (var k = len; k > 0; k--) move(o, k - 1, k + n - 1);
      for (var j = 0; j < n; j++) o[j] = arguments[j];
    }
    o.length = len + n; return len + n;
  };
  fns[4] = function slice(start, end) {
    var o = toObj(this), len = lenOf(o);
    var k = relIndex(start, len, 0), fin = relIndex(end, len, len);
    var a = speciesCreate(o, Math.max(fin - k, 0)), n = 0;
    for (; k < fin; k++, n++) if (k in o) createDataPropertyOrThrow(a, n, o[k]);
    a.length = n; return a;
  };
  fns[5] = function splice(start, deleteCount) {
    var o = toObj(this), len = lenOf(o), s = relIndex(start, len, 0), argc = arguments.length;
    var dc = argc === 0 ? 0 : argc === 1 ? len - s : Math.min(Math.max(toIntegerOrInfinity(deleteCount), 0), len - s);
    var items = argc > 2 ? argc - 2 : 0;
    if (len + items - dc > MAX) throw new TypeError('Invalid array length');
    var a = speciesCreate(o, dc), k;
    for (k = 0; k < dc; k++) if (s + k in o) createDataPropertyOrThrow(a, k, o[s + k]);
    a.length = dc;
    if (items < dc) {
      for (k = s; k < len - dc; k++) move(o, k + dc, k + items);
      for (k = len; k > len - dc + items; k--) delete o[k - 1];
    } else if (items > dc) {
      for (k = len - dc; k > s; k--) move(o, k + dc - 1, k + items - 1);
    }
    for (var j = 0; j < items; j++) o[s + j] = arguments[j + 2];
    o.length = len - dc + items; return a;
  };
  function spreadable(e) {
    if (!isObject(e)) return false;
    var sp = e[Symbol.isConcatSpreadable];
    return sp !== undefined ? !!sp : isArray(e);
  }
  fns[6] = function concat() {
    var o = toObj(this), a = speciesCreate(o, 0), n = 0;
    for (var i = -1; i < arguments.length; i++) {
      var e = i < 0 ? o : arguments[i];
      if (spreadable(e)) {
        var len = lenOf(e);
        if (n + len > MAX) throw new TypeError('Invalid array length');
        for (var k = 0; k < len; k++, n++) if (k in e) createDataPropertyOrThrow(a, n, e[k]);
      } else {
        if (n >= MAX) throw new TypeError('Invalid array length');
        createDataPropertyOrThrow(a, n++, e);
      }
    }
    a.length = n; return a;
  };
  fns[7] = function join(sep) {
    var o = toObj(this), len = lenOf(o), s = sep === undefined ? ',' : str(sep), r = '';
    for (var k = 0; k < len; k++) { if (k > 0) r += s; var e = o[k]; if (e !== undefined && e !== null) r += str(e); }
    return r;
  };
  fns[8] = function reverse() {
    var o = toObj(this), len = lenOf(o), mid = Math.floor(len / 2);
    for (var lo = 0; lo !== mid; lo++) {
      var up = len - lo - 1, le = lo in o, lv, uv;
      if (le) lv = o[lo];
      var ue = up in o;
      if (ue) uv = o[up];
      if (le && ue) { o[lo] = uv; o[up] = lv; }
      else if (ue) { o[lo] = uv; delete o[up]; }
      else if (le) { delete o[lo]; o[up] = lv; }
    }
    return o;
  };
  fns[9] = function indexOf(x, from) {
    var o = toObj(this), len = lenOf(o); if (len === 0) return -1;
    var n = toIntegerOrInfinity(from); if (n === Infinity) return -1;
    for (var k = n >= 0 ? n : Math.max(len + n, 0); k < len; k++) if (k in o && o[k] === x) return k;
    return -1;
  };
  fns[10] = function lastIndexOf(x, from) {
    var o = toObj(this), len = lenOf(o); if (len === 0) return -1;
    var n = arguments.length > 1 ? toIntegerOrInfinity(from) : len - 1; if (n === -Infinity) return -1;
    for (var k = n >= 0 ? Math.min(n, len - 1) : len + n; k >= 0; k--) if (k in o && o[k] === x) return k;
    return -1;
  };
  fns[11] = function includes(x, from) {
    var o = toObj(this), len = lenOf(o); if (len === 0) return false;
    var n = toIntegerOrInfinity(from); if (n === Infinity) return false;
    for (var k = n >= 0 ? n : Math.max(len + n, 0); k < len; k++) if (sameValueZero(o[k], x)) return true;
    return false;
  };
  fns[12] = function forEach(f, t) {
    var o = toObj(this), len = lenOf(o); fnArg(f);
    for (var k = 0; k < len; k++) if (k in o) f.call(t, o[k], k, o);
  };
  fns[13] = function map(f, t) {
    var o = toObj(this), len = lenOf(o); fnArg(f);
    var a = speciesCreate(o, len);
    for (var k = 0; k < len; k++) if (k in o) createDataPropertyOrThrow(a, k, f.call(t, o[k], k, o));
    return a;
  };
  fns[14] = function filter(f, t) {
    var o = toObj(this), len = lenOf(o); fnArg(f);
    var a = speciesCreate(o, 0), n = 0;
    for (var k = 0; k < len; k++) if (k in o) { var v = o[k]; if (f.call(t, v, k, o)) createDataPropertyOrThrow(a, n++, v); }
    return a;
  };
  function reduceWith(o, f, args, right) {
    var len = lenOf(o); fnArg(f);
    var k = right ? len - 1 : 0, step = right ? -1 : 1, acc;
    if (args.length >= 2) acc = args[1];
    else {
      for (;;) {
        if (right ? k < 0 : k >= len) throw new TypeError('Reduce of empty array with no initial value');
        var has = k in o; if (has) acc = o[k]; k += step; if (has) break;
      }
    }
    for (; right ? k >= 0 : k < len; k += step) if (k in o) acc = f(acc, o[k], k, o);
    return acc;
  }
  fns[15] = function reduce(f) { return reduceWith(toObj(this), f, arguments, false); };
  fns[16] = function reduceRight(f) { return reduceWith(toObj(this), f, arguments, true); };
  fns[17] = function some(f, t) {
    var o = toObj(this), len = lenOf(o); fnArg(f);
    for (var k = 0; k < len; k++) if (k in o && f.call(t, o[k], k, o)) return true;
    return false;
  };
  fns[18] = function every(f, t) {
    var o = toObj(this), len = lenOf(o); fnArg(f);
    for (var k = 0; k < len; k++) if (k in o && !f.call(t, o[k], k, o)) return false;
    return true;
  };
  function findWith(o, f, t, back, wantIndex) {
    var len = lenOf(o); fnArg(f);
    for (var i = 0; i < len; i++) {
      var k = back ? len - 1 - i : i, v = o[k];
      if (f.call(t, v, k, o)) return wantIndex ? k : v;
    }
    return wantIndex ? -1 : undefined;
  }
  fns[19] = function find(f, t) { return findWith(toObj(this), f, t, false, false); };
  fns[20] = function findIndex(f, t) { return findWith(toObj(this), f, t, false, true); };
  fns[21] = function findLast(f, t) { return findWith(toObj(this), f, t, true, false); };
  fns[22] = function findLastIndex(f, t) { return findWith(toObj(this), f, t, true, true); };
  var plainSort = Array.prototype.sort;
  function sortItems(items, cmp) {
    return plainSort.call(items, cmp === undefined ? undefined : function (x, y) {
      var v = Number(cmp(x, y)); return v !== v ? 0 : v;
    });
  }
  fns[23] = function sort(cmp) {
    if (cmp !== undefined && typeof cmp !== 'function') throw new TypeError('The comparison function must be either a function or undefined');
    var o = toObj(this), len = lenOf(o), items = [];
    for (var k = 0; k < len; k++) if (k in o) items[items.length] = o[k];
    sortItems(items, cmp);
    var j = 0;
    for (; j < items.length; j++) o[j] = items[j];
    for (; j < len; j++) delete o[j];
    return o;
  };
  fns[24] = function fill(v, start, end) {
    var o = toObj(this), len = lenOf(o);
    for (var k = relIndex(start, len, 0), fin = relIndex(end, len, len); k < fin; k++) o[k] = v;
    return o;
  };
  function flattenInto(target, src, len, start, depth, f, t) {
    var ti = start;
    for (var si = 0; si < len; si++) {
      if (!(si in src)) continue;
      var e = src[si];
      if (f) e = f.call(t, e, si, src);
      if (depth > 0 && isArray(e)) ti = flattenInto(target, e, lenOf(e), ti, depth === Infinity ? Infinity : depth - 1);
      else { if (ti >= MAX) throw new TypeError('Invalid array length'); createDataPropertyOrThrow(target, ti++, e); }
    }
    return ti;
  }
  fns[28] = function flat(depth) {
    var o = toObj(this), len = lenOf(o), d = depth === undefined ? 1 : toIntegerOrInfinity(depth);
    if (d < 0) d = 0;
    var a = speciesCreate(o, 0); flattenInto(a, o, len, 0, d); return a;
  };
  fns[29] = function flatMap(f, t) {
    var o = toObj(this), len = lenOf(o); fnArg(f);
    var a = speciesCreate(o, 0); flattenInto(a, o, len, 0, 1, f, t); return a;
  };
  fns[30] = function toString() {
    var o = toObj(this), j = o.join;
    return typeof j === 'function' ? j.call(o) : Object.prototype.toString.call(o);
  };
  fns[31] = function at(i) {
    var o = toObj(this), len = lenOf(o), r = toIntegerOrInfinity(i), k = r >= 0 ? r : len + r;
    return k < 0 || k >= len ? undefined : o[k];
  };
  fns[32] = function copyWithin(target, start, end) {
    var o = toObj(this), len = lenOf(o);
    var to = relIndex(target, len, 0), from = relIndex(start, len, 0), fin = relIndex(end, len, len);
    var count = Math.min(fin - from, len - to), dir = 1;
    if (from < to && to < from + count) { dir = -1; from += count - 1; to += count - 1; }
    for (; count > 0; count--, from += dir, to += dir) move(o, from, to);
    return o;
  };
  fns[33] = function toReversed() {
    var o = toObj(this), len = lenOf(o), a = arrayCreate(len);
    for (var k = 0; k < len; k++) a[k] = o[len - k - 1];
    return a;
  };
  fns[34] = function toSorted(cmp) {
    if (cmp !== undefined && typeof cmp !== 'function') throw new TypeError('The comparison function must be either a function or undefined');
    var o = toObj(this), len = lenOf(o), a = arrayCreate(len), items = [];
    for (var k = 0; k < len; k++) items[k] = o[k];
    sortItems(items, cmp);
    for (var j = 0; j < len; j++) a[j] = items[j];
    return a;
  };
  fns[35] = function with_(index, value) {
    var o = toObj(this), len = lenOf(o), r = toIntegerOrInfinity(index), at = r >= 0 ? r : len + r;
    if (at >= len || at < 0) throw new RangeError('Invalid index : ' + index);
    var a = arrayCreate(len);
    for (var k = 0; k < len; k++) a[k] = k === at ? value : o[k];
    return a;
  };
  fns[36] = Array;
  setArrayGeneric(fns);
})();

// ---- ArrayBuffer, SharedArrayBuffer, the typed arrays, DataView, Atomics:
// the bytes and the element access are native (typed.rs), the rest is here
var bufNew = globalThis.__bufNew, bufInfo = globalThis.__bufInfo, bufGet = globalThis.__bufGet, bufSet = globalThis.__bufSet;
var bufCopy = globalThis.__bufCopy, bufDetach = globalThis.__bufDetach, taMake = globalThis.__taMake, taInfo = globalThis.__taInfo, taLength = globalThis.__taLength, taSetFast = globalThis.__taSetFast, dvMake = globalThis.__dvMake, dvGetFast = globalThis.__dvGet, dvSetFast = globalThis.__dvSet, taKind = globalThis.__taKind, taSub = globalThis.__taSub, taNew = globalThis.__taNew, taSort = globalThis.__taSort;
var bufMax = globalThis.__bufMax, bufResize = globalThis.__bufResize;
['__bufNew', '__bufInfo', '__bufGet', '__bufSet', '__bufCopy', '__bufDetach', '__taMake', '__taInfo', '__taLength', '__taSetFast', '__dvMake', '__dvGet', '__dvSet', '__taKind', '__taSub', '__taNew', '__taSort', '__bufMax', '__bufResize'].forEach(function (k) { delete globalThis[k]; });
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
// a function's name for property key `k` (a symbol's in brackets)
function fnKeyName(k) { if (typeof k !== 'symbol') return String(k); var d = k.description; return d === undefined ? '' : '[' + d + ']'; }
function getter(o, name, f) {
  defineProperty(f, 'name', { value: 'get ' + fnKeyName(name), writable: false, enumerable: false, configurable: true });
  asMethod(f);
  defineProperty(o, name, { get: f, enumerable: false, configurable: true });
}
function speciesSelf() { return this; }

function bufLength(b, shared, what) {
  var i = bufInfo(b);
  if (i === -1 || (i % 1 !== 0) !== shared) throw new TypeError(what + ' called on incompatible receiver');
  return i === -2 ? -1 : Math.floor(i);
}
// options.maxByteLength (GetArrayBufferMaxByteLengthOption): undefined when absent
function maxLengthOption(options) {
  if (!isObject(options)) return undefined;
  var m = options.maxByteLength;
  return m === undefined ? undefined : toIndex(m, 'array buffer max length');
}
function allocBuffer(nt, dflt, n, max, shared) {
  if (max !== undefined && n > max) throw new RangeError('Invalid array buffer length: ' + n + ' is larger than maxByteLength ' + max);
  return bufNew(n, protoFrom(nt, dflt), shared, max);
}
function ArrayBuffer(length) {
  if (new.target === undefined) throw new TypeError("Constructor ArrayBuffer requires 'new'");
  var n = toIndex(length, 'array buffer length');
  return allocBuffer(new.target, ArrayBuffer.prototype, n, maxLengthOption(arguments[1]), false);
}
function SharedArrayBuffer(length) {
  if (new.target === undefined) throw new TypeError("Constructor SharedArrayBuffer requires 'new'");
  var n = toIndex(length, 'array buffer length');
  return allocBuffer(new.target, SharedArrayBuffer.prototype, n, maxLengthOption(arguments[1]), true);
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
    var cur = bufLength(this, shared, name + '.prototype.slice');
    if (cur < 0) throw new TypeError('Cannot perform ' + name + '.prototype.slice on a detached ArrayBuffer');
    if (first < cur) bufCopy(this, first, out, 0, Math.min(n, cur - first));
    return out;
  };
}
[[ArrayBuffer, false, 'ArrayBuffer'], [SharedArrayBuffer, true, 'SharedArrayBuffer']].forEach(function (e) {
  var C = e[0], shared = e[1], name = e[2], P = C.prototype;
  getter(P, 'byteLength', function () { var n = bufLength(this, shared, name + '.prototype.byteLength'); return n < 0 ? 0 : n; });
  hide(P, 'slice', bufferSlice(C, shared, name));
  getter(C, Symbol.species, speciesSelf);
  tag(P, name);
  hide(globalThis, name, C);
});
hide(ArrayBuffer, 'isView', function isView(x) { return taInfo(x) !== undefined || dvOf(x) !== undefined; });
getter(ArrayBuffer.prototype, 'detached', function () { bufLength(this, false, 'ArrayBuffer.prototype.detached'); return bufInfo(this) === -2; });
[[ArrayBuffer, false, 'ArrayBuffer', 'resizable', 'resize'], [SharedArrayBuffer, true, 'SharedArrayBuffer', 'growable', 'grow']].forEach(function (e) {
  var shared = e[1], name = e[2], P = e[0].prototype;
  getter(P, e[3], function () { bufLength(this, shared, name + '.prototype.' + e[3]); return bufMax(this) >= 0; });
  getter(P, 'maxByteLength', function () {
    var n = bufLength(this, shared, name + '.prototype.maxByteLength');
    if (n < 0) return 0;
    var m = bufMax(this); return m >= 0 ? m : n;
  });
  hide(P, e[4], ({ f: function (newLength) {
    var what = name + '.prototype.' + e[4];
    bufLength(this, shared, what);
    var m = bufMax(this);
    if (m < 0) throw new TypeError('Method ' + what + ' called on incompatible receiver');
    var n = toIndex(newLength, 'length');
    var cur = bufLength(this, shared, what);
    if (cur < 0) throw new TypeError('Cannot perform ' + what + ' on a detached ArrayBuffer');
    if (n > m) throw new RangeError(what + ': Invalid length parameter');
    if (shared && n < cur) throw new RangeError(what + ': Invalid length parameter');
    bufResize(this, n);
    return undefined;
  } }).f);
  defineProperty(P[e[4]], 'name', { value: e[4], writable: false, enumerable: false, configurable: true });
});
function copyAndDetach(o, newLength, keep, what) {
  bufLength(o, false, what);
  var n0 = bufLength(o, false, what);
  var n = newLength === undefined ? (n0 < 0 ? 0 : n0) : toIndex(newLength, 'array buffer length');
  var len = bufLength(o, false, what);
  if (len < 0) throw new TypeError('Cannot perform ' + what + ' on a detached ArrayBuffer');
  var m = keep ? bufMax(o) : -1;
  var out = allocBuffer(undefined, ArrayBuffer.prototype, n, m >= 0 ? m : undefined, false);
  bufCopy(o, 0, out, 0, Math.min(n, len));
  bufDetach(o);
  return out;
}
hide(ArrayBuffer.prototype, 'transfer', function transfer(newLength) { return copyAndDetach(this, newLength, true, 'ArrayBuffer.prototype.transfer'); });
hide(ArrayBuffer.prototype, 'transferToFixedLength', function transferToFixedLength(newLength) { return copyAndDetach(this, newLength, false, 'ArrayBuffer.prototype.transferToFixedLength'); });

var KINDS = ['Int8Array', 'Uint8Array', 'Uint8ClampedArray', 'Int16Array', 'Uint16Array', 'Int32Array', 'Uint32Array', 'Float32Array', 'Float64Array', 'BigInt64Array', 'BigUint64Array', 'Float16Array'];
var SIZES = [1, 1, 1, 2, 2, 4, 4, 4, 8, 8, 8, 2];
/** The element conversion of a kind: ToBigInt for the 64-bit ones. */
function elemOf(kind) { return kind === 9 || kind === 10 ? BigInt : Number; }
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
  if (i[4]) throw new TypeError('Cannot perform ' + what + ' on an out of bounds TypedArray');
  return i;
}
function construct(kind, nt, a, b, c) {
  var size = SIZES[kind];
  var proto = protoFrom(nt, CTORS[kind].prototype);
  if (!isObject(a)) {
    if (typeof a === 'number') { var t0 = taNew(kind, a, proto, ArrayBuffer.prototype); if (t0 !== undefined) return t0; }
    var n = toIndex(a, 'typed array length');
    return taMake(kind, bufNew(n * size, ArrayBuffer.prototype, false, undefined, kind), 0, n, proto);
  }
  var bi = bufInfo(a);
  if (bi !== -1) {
    var off = toIndex(b, 'offset');
    if (off % size) throw new RangeError('start offset of ' + KINDS[kind] + ' should be a multiple of ' + size);
    var len = c === undefined ? undefined : toIndex(c, 'typed array length');
    if (bi === -2) throw new TypeError('Cannot perform Construct on a detached ArrayBuffer');
    var blen = Math.floor(bi);
    if (c === undefined && bufMax(a) >= 0) {
      // over a resizable buffer: the view tracks its length
      if (off > blen) throw new RangeError('Start offset ' + off + ' is outside the bounds of the buffer');
      return taMake(kind, a, off, undefined, proto);
    }
    if (c === undefined) {
      if (blen % size) throw new RangeError('byte length of ' + KINDS[kind] + ' should be a multiple of ' + size);
      if (off > blen) throw new RangeError('Start offset ' + off + ' is outside the bounds of the buffer');
      len = (blen - off) / size;
    } else {
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
  var t = taMake(kind, bufNew(count * size, ArrayBuffer.prototype, false, undefined, kind), 0, count, proto);
  for (var i = 0; i < count; i++) t[i] = src[i];
  return t;
}
function defineTA(kind, C) {
  CTORS[kind] = C;
  Object.setPrototypeOf(C, TypedArray);
  Object.setPrototypeOf(C.prototype, TAP);
  defineProperty(C, 'prototype', { writable: false, enumerable: false, configurable: false });
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
defineTA(11, function Float16Array(a, b, c) { if (new.target === undefined) throw new TypeError("Constructor Float16Array requires 'new'"); return construct(11, new.target, a, b, c); });
var f16cell = new Float16Array(1);
hide(Math, 'f16round', function f16round(x) { f16cell[0] = x; return f16cell[0]; });
getter(TypedArray, Symbol.species, speciesSelf);

/** A new typed array from `exemplar`'s species, checked. */
function speciesCreate(exemplar, args) {
  var kind = taOf(exemplar, 'TypedArray species create')[0];
  var C = speciesOf(exemplar, CTORS[kind]);
  var out = args.length === 1 ? new C(args[0]) : args.length === 2 ? new C(args[0], args[1]) : new C(args[0], args[1], args[2]);
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
getter(TAP, 'byteOffset', function () { var i = taOf(this, 'TypedArray.prototype.byteOffset'); return i[4] ? 0 : i[2]; });
getter(TAP, 'length', taLength);
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
  if (count > 0) {
    // the coercions can have shrunk or detached the buffer
    var cur = live(this, 'TypedArray.prototype.copyWithin');
    var limit = cur[3] * size + cur[2], fb = cur[2] + from * size, tb = cur[2] + to * size;
    var bytes = Math.min(count * size, limit - fb, limit - tb);
    if (bytes > 0) bufCopy(info[1], fb, info[1], tb, bytes);
  }
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
  e = Math.min(e, len(this, 'TypedArray.prototype.fill'));
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
  if (taSetFast(this, source, offset)) return;
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
  if (cmp === undefined && taSort(this)) return this;
  var vals = sortedValues(this, cmp, 'TypedArray.prototype.sort');
  for (var i = 0; i < vals.length; i++) this[i] = vals[i];
  return this;
});
hide(TAP, 'subarray', function subarray(start, end) {
  // the default species and number bounds: native (the order of the
  // steps is not observable then)
  var k = taKind(this);
  if (k >= 0 && (start === undefined || typeof start === 'number') && (end === undefined || typeof end === 'number')) {
    var D = CTORS[k];
    if (speciesOf(this, D) === D) { var r = taSub(this, start, end, D.prototype); if (r !== undefined) return r; }
  }
  var info = taOf(this, 'TypedArray.prototype.subarray'); var n = info[3];
  var s = relIndex(start, n, 0);
  if (info[5] && end === undefined) return speciesCreate(this, [info[1], info[2] + s * SIZES[info[0]]]);
  var e = relIndex(end, n, n);
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

// the specification's lengths for the methods written with more parameters
[['every', 1], ['fill', 1], ['filter', 1], ['find', 1], ['findIndex', 1], ['findLast', 1], ['findLastIndex', 1], ['forEach', 1], ['includes', 1], ['indexOf', 1], ['lastIndexOf', 1], ['map', 1], ['reduce', 1], ['reduceRight', 1], ['set', 1], ['some', 1], ['join', 1], ['copyWithin', 2], ['slice', 2], ['subarray', 2], ['sort', 1], ['with', 2], ['at', 1], ['toSorted', 1]].forEach(function (e) {
  var f = TAP[e[0]];
  if (typeof f === 'function' && f.length !== e[1]) setLength(f, e[1]);
});
if (TypedArray.from.length !== 1) setLength(TypedArray.from, 1);
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
  var tracking = byteLength === undefined && bufMax(buffer) >= 0;
  var n = byteLength === undefined ? blen - off : toIndex(byteLength, 'DataView length');
  if (off + n > blen) throw new RangeError('Invalid DataView length ' + n);
  var o = create(protoFrom(new.target, DataView.prototype));
  // the buffer can be detached or resized by then
  bi = bufInfo(buffer);
  if (bi === -2) throw new TypeError('Cannot perform DataView constructor on a detached ArrayBuffer');
  blen = Math.floor(bi);
  if (off > blen) throw new RangeError('Start offset ' + off + ' is outside the bounds of the buffer');
  if (byteLength !== undefined && off + n > blen) throw new RangeError('Invalid DataView length ' + n);
  dvData.set(o, [buffer, off, tracking ? -1 : n]);
  dvMake(o, buffer, off, tracking ? -1 : n);
  return o;
}
// the view's byte length; -1 when out of bounds or detached
function dvLength(d) {
  var bi = bufInfo(d[0]);
  if (bi === -2) return -1;
  var blen = Math.floor(bi);
  if (d[1] > blen) return -1;
  if (d[2] < 0) return blen - d[1];
  return d[1] + d[2] > blen ? -1 : d[2];
}
var DVP = DataView.prototype;
function dv(o, what) { var d = dvOf(o); if (d === undefined) throw new TypeError(what + ' called on incompatible receiver'); return d; }
getter(DVP, 'buffer', function () { return dv(this, 'DataView.prototype.buffer')[0]; });
getter(DVP, 'byteLength', function () { var d = dv(this, 'DataView.prototype.byteLength'), n = dvLength(d); if (n < 0) throw new TypeError(bufInfo(d[0]) === -2 ? 'Cannot perform DataView.prototype.byteLength on a detached ArrayBuffer' : 'DataView is out of bounds'); return n; });
getter(DVP, 'byteOffset', function () { var d = dv(this, 'DataView.prototype.byteOffset'); if (dvLength(d) < 0) throw new TypeError(bufInfo(d[0]) === -2 ? 'Cannot perform DataView.prototype.byteOffset on a detached ArrayBuffer' : 'DataView is out of bounds'); return d[1]; });
tag(DVP, 'DataView');
['Int8', 'Uint8', '', 'Int16', 'Uint16', 'Int32', 'Uint32', 'Float32', 'Float64', 'BigInt64', 'BigUint64', 'Float16'].forEach(function (name, kind) {
  if (!name) return;
  hide(DVP, 'get' + name, function (index, little) {
    var r = dvGetFast(this, index, kind, little);
    if (r !== undefined) return r;
    var d = dv(this, 'DataView.prototype.get' + name); var i = toIndex(index, 'offset');
    var n = dvLength(d);
    if (n < 0) throw new TypeError(bufInfo(d[0]) === -2 ? 'Cannot perform DataView.prototype.get' + name + ' on a detached ArrayBuffer' : 'DataView is out of bounds');
    if (i + SIZES[kind] > n) throw new RangeError('Offset is outside the bounds of the DataView');
    return bufGet(d[0], d[1] + i, kind, !!little);
  });
  hide(DVP, 'set' + name, function (index, value, little) {
    if (dvSetFast(this, index, kind, value, little)) return;
    var d = dv(this, 'DataView.prototype.set' + name); var i = toIndex(index, 'offset'); var x = elemOf(kind)(value);
    var n = dvLength(d);
    if (n < 0) throw new TypeError(bufInfo(d[0]) === -2 ? 'Cannot perform DataView.prototype.set' + name + ' on a detached ArrayBuffer' : 'DataView is out of bounds');
    if (i + SIZES[kind] > n) throw new RangeError('Offset is outside the bounds of the DataView');
    bufSet(d[0], d[1] + i, kind, x, !!little);
  });
  setLength(DVP['get' + name], 1);
  setLength(DVP['set' + name], 2);
});
hide(globalThis, 'DataView', DataView);

// ---- ES2026: Error.isError, Math.sumPrecise, Uint8Array base64 / hex
var isErrorObj = globalThis.__isError; delete globalThis.__isError;
(function () {
  'use strict';
  hide(Error, 'isError', function isError(v) { return isErrorObj(v); });

  // the exact sum of doubles: each one an integer count of 2^-1074
  var sumCell = new DataView(new ArrayBuffer(8));
  function exactOf(x) {
    sumCell.setFloat64(0, x);
    var bits = sumCell.getBigUint64(0);
    var exp = Number((bits >> 52n) & 0x7ffn), mant = bits & 0xfffffffffffffn;
    var v = exp === 0 ? mant : (mant | 0x10000000000000n) << BigInt(exp - 1);
    return (bits >> 63n) ? -v : v;
  }
  function pow2(e) {
    // 2^e as a double, for -1074 <= e <= 1023
    var r = 1;
    while (e > 0) { var k = e > 1000 ? 1000 : e; r *= Math.pow(2, k); e -= k; }
    while (e < 0) { var j = e < -1000 ? -1000 : e; r *= Math.pow(2, j); e -= j; }
    return r;
  }
  function roundExact(s) {
    if (s === 0n) return 0;
    var neg = s < 0n, a = neg ? -s : s;
    var L = a.toString(2).length, r;
    if (L <= 53) r = Number(a) * pow2(-1074);
    else {
      var shift = L - 53, q = a >> BigInt(shift), rem = a - (q << BigInt(shift)), half = 1n << BigInt(shift - 1);
      if (rem > half || (rem === half && (q & 1n) === 1n)) q += 1n;
      var e = shift - 1074;
      r = e > 1023 ? Infinity : Number(q) * pow2(e);
    }
    return neg ? -r : r;
  }
  hide(Math, 'sumPrecise', function sumPrecise(items) {
    if (items === undefined || items === null) throw new TypeError('Math.sumPrecise called on null or undefined');
    var m = items[Symbol.iterator];
    if (!isCallable(m)) throw new TypeError('items is not iterable');
    var it = m.call(items);
    if (!isObject(it)) throw new TypeError('Result of the Symbol.iterator method is not an object');
    var next = it.next, state = 0, sum = 0n; // 0 minus-zero, 1 finite, 2 +inf, 3 -inf, 4 NaN
    for (;;) {
      var r = next.call(it);
      if (!isObject(r)) throw new TypeError('Iterator result is not an object');
      if (r.done) break;
      var n = r.value;
      if (typeof n !== 'number') {
        try { var ret = it.return; if (ret !== undefined && ret !== null) ret.call(it); } catch (e) {}
        throw new TypeError('Math.sumPrecise: ' + String(typeof n) + ' is not a number');
      }
      if (state === 4) continue;
      if (n !== n) state = 4;
      else if (n === Infinity) state = state === 3 ? 4 : 2;
      else if (n === -Infinity) state = state === 2 ? 4 : 3;
      else if (!(n === 0 && 1 / n < 0) && (state === 0 || state === 1)) { state = 1; sum += exactOf(n); }
    }
    if (state === 4) return NaN;
    if (state === 2) return Infinity;
    if (state === 3) return -Infinity;
    if (state === 0) return -0;
    return roundExact(sum);
  });

  var B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  var B64URL = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
  var U8 = Uint8Array;
  function u8Of(o, what) {
    var i = taInfo(o);
    if (i === undefined || i[0] !== 1) throw new TypeError(what + ': this is not a Uint8Array');
    return i;
  }
  function u8Bytes(o, what) {
    var i = taInfo(o);
    if (bufInfo(i[1]) === -2 || i[4]) throw new TypeError('Cannot perform ' + what + ' on a detached or out of bounds Uint8Array');
    var out = [];
    for (var k = 0; k < i[3]; k++) out.push(o[k]);
    return out;
  }
  function optionsOf(o) {
    if (o === undefined) return create(null);
    if (!isObject(o)) throw new TypeError('options must be an object');
    return o;
  }
  function alphabetOf(opts) {
    var a = opts.alphabet;
    if (a === undefined) return 'base64';
    if (a !== 'base64' && a !== 'base64url') throw new TypeError('expected alphabet to be either "base64" or "base64url"');
    return a;
  }
  function lastChunkOf(opts) {
    var l = opts.lastChunkHandling;
    if (l === undefined) return 'loose';
    if (l !== 'loose' && l !== 'strict' && l !== 'stop-before-partial') throw new TypeError('expected lastChunkHandling to be "loose", "strict" or "stop-before-partial"');
    return l;
  }
  function isWs(c) { return c === 9 || c === 10 || c === 12 || c === 13 || c === 32; }
  function skipWs(s, i) { while (i < s.length && isWs(s.charCodeAt(i))) i++; return i; }
  function decodeChunk(chunk, strict, out) {
    var n = chunk.length;
    if (n === 2) chunk += 'AA'; else if (n === 3) chunk += 'A';
    var v = 0;
    for (var k = 0; k < 4; k++) v = v * 64 + B64.indexOf(chunk[k]);
    var b0 = (v >> 16) & 255, b1 = (v >> 8) & 255, b2 = v & 255;
    if (n === 2) { if (strict && b1 !== 0) return false; out.push(b0); return true; }
    if (n === 3) { if (strict && b2 !== 0) return false; out.push(b0, b1); return true; }
    out.push(b0, b1, b2); return true;
  }
  function fromBase64(s, alphabet, last, max) {
    var bytes = [], read = 0, chunk = '', i = 0, len = s.length;
    function fail() { return { read: read, bytes: bytes, error: true }; }
    if (max === 0) return { read: 0, bytes: bytes, error: false };
    for (;;) {
      i = skipWs(s, i);
      if (i === len) {
        if (chunk.length > 0) {
          if (last === 'stop-before-partial') return { read: read, bytes: bytes, error: false };
          if (last === 'loose') { if (chunk.length === 1) return fail(); decodeChunk(chunk, false, bytes); }
          else return fail();
        }
        return { read: len, bytes: bytes, error: false };
      }
      var c = s[i]; i++;
      if (c === '=') {
        if (chunk.length < 2) return fail();
        i = skipWs(s, i);
        if (chunk.length === 2) {
          if (i === len) { if (last === 'stop-before-partial') return { read: read, bytes: bytes, error: false }; return fail(); }
          if (s[i] === '=') i = skipWs(s, i + 1);
        }
        if (i < len) return fail();
        if (!decodeChunk(chunk, last === 'strict', bytes)) return fail();
        return { read: len, bytes: bytes, error: false };
      }
      if (alphabet === 'base64url') {
        if (c === '+' || c === '/') return fail();
        if (c === '-') c = '+'; else if (c === '_') c = '/';
      }
      if (B64.indexOf(c) < 0 || c.length !== 1) return fail();
      var remaining = max - bytes.length;
      if ((remaining === 1 && chunk.length === 2) || (remaining === 2 && chunk.length === 3)) return { read: read, bytes: bytes, error: false };
      chunk += c;
      if (chunk.length === 4) {
        decodeChunk(chunk, false, bytes); chunk = ''; read = i;
        if (bytes.length === max) return { read: read, bytes: bytes, error: false };
      }
    }
  }
  function hexVal(c) {
    if (c >= 48 && c <= 57) return c - 48;
    if (c >= 65 && c <= 70) return c - 55;
    if (c >= 97 && c <= 102) return c - 87;
    return -1;
  }
  function fromHex(s, max) {
    var bytes = [], read = 0, len = s.length;
    if (len % 2 !== 0) return { read: 0, bytes: bytes, error: true };
    while (read < len && bytes.length < max) {
      var h = hexVal(s.charCodeAt(read)), l = hexVal(s.charCodeAt(read + 1));
      if (h < 0 || l < 0) return { read: read, bytes: bytes, error: true };
      read += 2; bytes.push(h * 16 + l);
    }
    return { read: read, bytes: bytes, error: false };
  }
  function newU8(bytes) { var a = new U8(bytes.length); for (var k = 0; k < bytes.length; k++) a[k] = bytes[k]; return a; }
  hide(U8, 'fromBase64', function fromBase64_(string) {
    if (typeof string !== 'string') throw new TypeError('expected input to be a string');
    var opts = optionsOf(arguments[1]), alphabet = alphabetOf(opts), last = lastChunkOf(opts);
    var r = fromBase64(string, alphabet, last, 9007199254740991);
    if (r.error) throw new SyntaxError('Invalid base64 string');
    return newU8(r.bytes);
  });
  defineProperty(U8.fromBase64, 'name', { value: 'fromBase64', writable: false, enumerable: false, configurable: true });
  hide(U8, 'fromHex', function fromHex_(string) {
    if (typeof string !== 'string') throw new TypeError('expected input to be a string');
    var r = fromHex(string, 9007199254740991);
    if (r.error) throw new SyntaxError('Invalid hex string');
    return newU8(r.bytes);
  });
  defineProperty(U8.fromHex, 'name', { value: 'fromHex', writable: false, enumerable: false, configurable: true });
  var U8P = U8.prototype;
  hide(U8P, 'toBase64', function toBase64() {
    u8Of(this, 'Uint8Array.prototype.toBase64');
    var opts = optionsOf(arguments[0]), alphabet = alphabetOf(opts), omit = !!opts.omitPadding;
    var b = u8Bytes(this, 'Uint8Array.prototype.toBase64'), A = alphabet === 'base64url' ? B64URL : B64, out = '';
    for (var k = 0; k < b.length; k += 3) {
      var n = b.length - k, v = (b[k] << 16) | ((n > 1 ? b[k + 1] : 0) << 8) | (n > 2 ? b[k + 2] : 0);
      out += A[(v >> 18) & 63] + A[(v >> 12) & 63];
      out += n > 1 ? A[(v >> 6) & 63] : (omit ? '' : '=');
      out += n > 2 ? A[v & 63] : (omit ? '' : '=');
    }
    return out;
  });
  hide(U8P, 'toHex', function toHex() {
    u8Of(this, 'Uint8Array.prototype.toHex');
    var b = u8Bytes(this, 'Uint8Array.prototype.toHex'), out = '';
    for (var k = 0; k < b.length; k++) out += (b[k] < 16 ? '0' : '') + b[k].toString(16);
    return out;
  });
  function setInto(o, r, what) {
    for (var k = 0; k < r.bytes.length; k++) o[k] = r.bytes[k];
    if (r.error) throw new SyntaxError(what);
    var res = {}; res.read = r.read; res.written = r.bytes.length; return res;
  }
  hide(U8P, 'setFromBase64', function setFromBase64(string) {
    u8Of(this, 'Uint8Array.prototype.setFromBase64');
    if (typeof string !== 'string') throw new TypeError('expected input to be a string');
    var opts = optionsOf(arguments[1]), alphabet = alphabetOf(opts), last = lastChunkOf(opts);
    var i = taInfo(this);
    if (bufInfo(i[1]) === -2 || i[4]) throw new TypeError('Cannot perform Uint8Array.prototype.setFromBase64 on a detached or out of bounds Uint8Array');
    return setInto(this, fromBase64(string, alphabet, last, i[3]), 'Invalid base64 string');
  });
  hide(U8P, 'setFromHex', function setFromHex(string) {
    u8Of(this, 'Uint8Array.prototype.setFromHex');
    if (typeof string !== 'string') throw new TypeError('expected input to be a string');
    var i = taInfo(this);
    if (bufInfo(i[1]) === -2 || i[4]) throw new TypeError('Cannot perform Uint8Array.prototype.setFromHex on a detached or out of bounds Uint8Array');
    return setInto(this, fromHex(string, i[3]), 'Invalid hex string');
  });
})();

// Atomics: one thread, so plain reads and writes on an integer typed array
var Atomics = {};
function intTA(ta, what) {
  var i = live(ta, 'Atomics.' + what);
  if (i[0] === 2 || (i[0] > 6 && i[0] < 9) || i[0] === 11) throw new TypeError('Atomics.' + what + ': not an integer typed array');
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
  getter(C, Symbol.species, speciesSelf);
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
// forEach and map in JavaScript: the callback runs as one of the
// interpreter's frames, so a time slice (Engine::set_slice) can stop in it
var fcall = Function.prototype.call;
function arrayThis(t) {
  if (t === null || t === undefined) throw new TypeError('Array.prototype method called on null or undefined');
  return Object(t);
}
hide(AP, 'forEach', setLength(function forEach(f, t) {
  var o = arrayThis(this), n = toLength(o.length);
  if (typeof f !== 'function') throw new TypeError('callback is not a function');
  for (var i = 0; i < n; i++) if (i in o) fcall.call(f, t, o[i], i, o);
}, 1));
hide(AP, 'map', setLength(function map(f, t) {
  var o = arrayThis(this), n = toLength(o.length);
  if (typeof f !== 'function') throw new TypeError('callback is not a function');
  var A = arraySpecies(o, n), plain = A === null;
  if (plain) A = new Array(n);
  for (var i = 0; i < n; i++) {
    if (i in o) {
      var v = fcall.call(f, t, o[i], i, o);
      if (plain) A[i] = v; else defineProperty(A, i, { value: v, writable: true, enumerable: true, configurable: true });
    }
  }
  return A;
}, 1));
var nativeFrom = Array.from, nativeOf = Array.of;
var fromSetValues = Set.prototype.values, fromMapEntries = Map.prototype.entries, fromStrIter = String.prototype[Symbol.iterator];
hide(Array, 'from', setLength(function from(items, mapFn, thisArg) {
  var C = this;
  if (mapFn !== undefined && !isCallable(mapFn)) throw new TypeError(String(mapFn) + ' is not a function');
  var iter = items === undefined || items === null ? undefined : items[Symbol.iterator];
  if (C === Array && Array.isArray(items) && iter === AP.values && mapFn === undefined) return nativeFrom.call(Array, items);
  var ctor = isCallable(C) && C !== Array;
  // a Set, a Map or a string with its own iterator, into a plain array:
  // the native spread (no user code runs while it iterates)
  if (!ctor && mapFn === undefined && iter !== undefined && ((iter === fromSetValues && items instanceof Set) || (iter === fromMapEntries && items instanceof Map) || (typeof items === 'string' && iter === fromStrIter))) return nativeFrom.call(Array, items);
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
      if (ctor) defineProperty(A, k, { value: v, writable: true, enumerable: true, configurable: true }); else A[k] = v;
      k++;
    }
    A.length = k;
    return A;
  }
  var src = Object(items), len = toLength(src.length);
  A = ctor ? new C(len) : new Array(len);
  for (; k < len; k++) {
    var x = mapFn ? mapFn.call(thisArg, src[k], k) : src[k];
    if (ctor) defineProperty(A, k, { value: x, writable: true, enumerable: true, configurable: true }); else A[k] = x;
  }
  A.length = len;
  return A;
}, 1));
function createDataPropertyOrThrow(o, k, v) {
  defineProperty(o, k, { value: v, writable: true, enumerable: true, configurable: true });
}
// Array.fromAsync: an async iterable, a sync one (each value awaited) or an
// array-like
hide(Array, 'fromAsync', setLength(function fromAsync(items, mapFn, thisArg) {
  var C = this;
  return (async function () {
    var mapping = mapFn !== undefined;
    if (mapping && !isCallable(mapFn)) throw new TypeError(String(mapFn) + ' is not a function');
    var A, k = 0;
    var am = items === undefined || items === null ? undefined : items[Symbol.asyncIterator];
    var it, next;
    if (am !== undefined && am !== null) {
      if (!isCallable(am)) throw new TypeError('Symbol.asyncIterator is not a function');
      it = am.call(items);
      if (!isObject(it)) throw new TypeError('Result of the Symbol.asyncIterator method is not an object');
      next = it.next;
    } else {
      var sm = items === undefined || items === null ? undefined : items[Symbol.iterator];
      if (sm !== undefined && sm !== null) {
        if (!isCallable(sm)) throw new TypeError('Symbol.iterator is not a function');
        var sit = sm.call(items);
        if (!isObject(sit)) throw new TypeError('Result of the Symbol.iterator method is not an object');
        it = asyncFromSyncOf(sit);
        next = it.next;
      }
    }
    if (it !== undefined) {
      A = isConstructor(C) ? new C() : [];
      for (;;) {
        var r = await next.call(it);
        if (!isObject(r)) throw new TypeError('Iterator result is not an object');
        if (r.done) { A.length = k; return A; }
        var v = r.value;
        try {
          if (mapping) v = await mapFn.call(thisArg, v, k);
          createDataPropertyOrThrow(A, k, v);
        } catch (e) {
          var ret = it.return;
          if (ret !== undefined && ret !== null) { try { await ret.call(it); } catch (e2) {} }
          throw e;
        }
        k++;
      }
    }
    var src = Object(items), len = toLength(src.length);
    A = isConstructor(C) ? new C(len) : new Array(len);
    for (; k < len; k++) {
      var x = await src[k];
      if (mapping) x = await mapFn.call(thisArg, x, k);
      createDataPropertyOrThrow(A, k, x);
    }
    A.length = len;
    return A;
  })();
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
function named0(f, n) { return defineProperty(f, 'name', { value: n, configurable: true }); }
var sMatch = SP.match, sReplace = SP.replace, sSearch = SP.search, sSplit = SP.split, sMatchAll = SP.matchAll, sReplaceAll = SP.replaceAll;
function needRegExp(r, what) { if (!isObject(r)) throw new TypeError('RegExp.prototype[' + what + '] called on incompatible receiver'); }

// ---- RegExp.prototype's Symbol methods on objects that are not RegExps
// (a proxy, an object with its own exec), in the specification's steps
var nativeExec = RP.exec, sCodeAt = SP.charCodeAt, sCharAt = SP.charAt, sSlice = SP.slice, sIndexOf = SP.indexOf, fApply = Function.prototype.apply;
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
  var c = sCodeAt.call(S, i);
  if (c < 0xD800 || c > 0xDBFF) return i + 1;
  var d = sCodeAt.call(S, i + 1);
  return d >= 0xDC00 && d <= 0xDFFF ? i + 2 : i + 1;
}
function genMatch(R, S) {
  'use strict';
  var flags = String(R.flags);
  if (sIndexOf.call(flags, 'g') < 0) return regExpExec(R, S);
  var u = sIndexOf.call(flags, 'u') >= 0 || sIndexOf.call(flags, 'v') >= 0;
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
  'use strict';
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
    var c = sCharAt.call(t, i);
    if (c !== '$' || i + 1 >= t.length) { out += c; continue; }
    var d = sCharAt.call(t, i + 1);
    if (d === '$') { out += '$'; i++; }
    else if (d === '&') { out += m; i++; }
    else if (d === '`') { out += sSlice.call(S, 0, pos); i++; }
    else if (d === "'") { out += tail >= S.length ? '' : sSlice.call(S, tail); i++; }
    else if (d >= '0' && d <= '9') {
      var k = +d, used = 1, e = sCharAt.call(t, i + 2);
      if (e >= '0' && e <= '9' && k * 10 + (+e) >= 1 && k * 10 + (+e) <= n) { k = k * 10 + (+e); used = 2; }
      if (k >= 1 && k <= n) { var cv = caps[k - 1]; out += cv === undefined ? '' : cv; i += used; }
      else out += '$';
    } else if (d === '<' && groups !== undefined) {
      var close = sIndexOf.call(t, '>', i + 2);
      if (close < 0) out += '$';
      else { var gv = groups[sSlice.call(t, i + 2, close)]; out += gv === undefined ? '' : String(gv); i = close; }
    } else out += '$';
  }
  return out;
}
function genReplace(R, S, rep) {
  'use strict';
  var fn = isCallable(rep);
  if (!fn) rep = String(rep);
  var flags = String(R.flags);
  var global = sIndexOf.call(flags, 'g') >= 0, u = false;
  if (global) { u = sIndexOf.call(flags, 'u') >= 0 || sIndexOf.call(flags, 'v') >= 0; R.lastIndex = 0; }
  var results = create(null), rn = 0;
  for (;;) {
    var r = regExpExec(R, S);
    if (r === null) break;
    results[rn++] = r;
    if (!global) break;
    if (String(r[0]) === '') R.lastIndex = advance(S, toLength(R.lastIndex), u);
  }
  var out = '', next = 0;
  for (var i = 0; i < rn; i++) {
    var res = results[i];
    var nc = Math.max(toLength(res.length) - 1, 0);
    var matched = String(res[0]);
    var pos = Math.max(Math.min(toIntegerOrInfinity(res.index), S.length), 0);
    var caps = create(null);
    caps.length = nc;
    for (var j = 1; j <= nc; j++) { var cj = res[j]; caps[j - 1] = cj === undefined ? undefined : String(cj); }
    var groups = res.groups, s;
    if (fn) {
      var args = create(null), an = 0;
      args[an++] = matched;
      for (var a = 0; a < nc; a++) args[an++] = caps[a];
      args[an++] = pos;
      args[an++] = S;
      if (groups !== undefined) args[an++] = groups;
      args.length = an;
      s = String(fApply.call(rep, undefined, args));
    } else {
      if (groups !== undefined) groups = Object(groups);
      s = substitution(matched, S, pos, caps, groups, rep);
    }
    if (pos >= next) { out += sSlice.call(S, next, pos) + s; next = pos + matched.length; }
  }
  return next >= S.length ? out : out + sSlice.call(S, next);
}
function genSplit(R, S, lim) {
  'use strict';
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
function symMethod(sym, name, f) {
  defineProperty(f, 'name', { value: '[Symbol.' + name + ']', writable: false, enumerable: false, configurable: true });
  hide(RP, sym, f);
}
// as the specification writes them, on any object (String.prototype's
// methods take the native path for a plain RegExp themselves)
symMethod(Symbol.match, 'match', function (s) { 'use strict'; needRegExp(this, 'Symbol.match'); return genMatch(this, String(s)); });
symMethod(Symbol.replace, 'replace', function (s, r) { 'use strict'; needRegExp(this, 'Symbol.replace'); return genReplace(this, String(s), r); });
symMethod(Symbol.search, 'search', function (s) { 'use strict'; needRegExp(this, 'Symbol.search'); return genSearch(this, String(s)); });
symMethod(Symbol.split, 'split', function (s, lim) { 'use strict'; needRegExp(this, 'Symbol.split'); return genSplit(this, String(s), lim); });
var RSIP = create(getPrototypeOf(getPrototypeOf([][Symbol.iterator]()))), rsiState = new WeakMap();
defineProperty(RSIP, Symbol.toStringTag, { value: 'RegExp String Iterator', writable: false, enumerable: false, configurable: true });
hide(RSIP, 'next', function next() {
  'use strict';
  var st = isObject(this) ? rsiState.get(this) : undefined;
  if (st === undefined) throw new TypeError('%RegExpStringIteratorPrototype%.next called on incompatible receiver');
  if (st.done) return { value: undefined, done: true };
  var r = regExpExec(st.R, st.S);
  if (r === null) { st.done = true; return { value: undefined, done: true }; }
  if (!st.g) { st.done = true; return { value: r, done: false }; }
  if (String(r[0]) === '') st.R.lastIndex = advance(st.S, toLength(st.R.lastIndex), st.u);
  return { value: r, done: false };
});
symMethod(Symbol.matchAll, 'matchAll', function (s) {
  'use strict';
  needRegExp(this, 'Symbol.matchAll');
  var S = String(s), C = speciesOf(this, RegExp), flags = String(this.flags);
  var matcher = new C(this, flags);
  matcher.lastIndex = toLength(this.lastIndex);
  var it = create(RSIP);
  rsiState.set(it, { R: matcher, S: S, g: flags.indexOf('g') >= 0, u: flags.indexOf('u') >= 0 || flags.indexOf('v') >= 0, done: false });
  return it;
});
hide(SP, 'matchAll', setLength(named0(function matchAll(x) {
  'use strict';
  if (this === undefined || this === null) throw new TypeError('String.prototype.matchAll called on null or undefined');
  if (isObject(x)) {
    if (isRegExpLike(x)) {
      var fl = x.flags;
      if (fl === undefined || fl === null) throw new TypeError('String.prototype.matchAll called with a RegExp without flags');
      if (String(fl).indexOf('g') < 0) throw new TypeError('String.prototype.matchAll called with a non-global RegExp argument');
    }
    var m = x[Symbol.matchAll];
    if (m !== undefined && m !== null) return m.call(x, this);
  }
  var S = String(this), rx = new RegExp(x, 'g');
  return rx[Symbol.matchAll](S);
}, 'matchAll'), 1));
[['match', Symbol.match, sMatch, 1], ['replace', Symbol.replace, sReplace, 2],
 ['replaceAll', Symbol.replaceAll, sReplaceAll, 2], ['search', Symbol.search, sSearch, 1], ['split', Symbol.split, sSplit, 2]].forEach(function (e) {
  var name = e[0], sym = e[1], nat = e[2];
  var f = function (x, y) {
    'use strict';
    if (this === undefined || this === null) throw new TypeError('String.prototype.' + name + ' called on null or undefined');
    if (isObject(x) && !(x instanceof RegExp && getPrototypeOf(x) === RP)) {
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
// startsWith, endsWith and includes are the natives: they do IsRegExp

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
    'use strict';
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
  'use strict';
  if (Array.isArray(this)) return nativeIncludes.call(this, x, from);
  if (this === undefined || this === null) throw new TypeError('Array.prototype.includes called on null or undefined');
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
    'use strict';
    if (this === RP) return undefined;
    if (!isRegExpObj(this)) throw new TypeError('RegExp.prototype.' + e[0] + ' getter called on non-RegExp');
    return sIndexOf.call(String(nativeFlags.call(this)), e[1]) >= 0;
  });
});
getter(RP, 'flags', function () {
  'use strict';
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
var afsResolve = Promise.resolve, afsReject = Promise.reject, afsThen = Promise.prototype.then, AfsPromise = Promise;
var asyncFromSyncOf = function (sync) {
  // CreateAsyncFromSyncIterator: next is read once
  var it = create(AsyncIteratorProto), syncNext = sync.next;
  // IteratorClose: after an error (`thrown`) that error stands
  function closeSync(thrown) {
    if (thrown) { try { var m0 = sync.return; if (m0 !== undefined && m0 !== null) m0.call(sync); } catch (e) {} return; }
    var m = sync.return;
    if (m !== undefined && m !== null && !isObject(m.call(sync))) throw new TypeError('Iterator result is not an object');
  }
  // AsyncFromSyncIteratorContinuation
  function cont(r, closeOnRejection) {
    if (!isObject(r)) throw new TypeError('Iterator result is not an object');
    var done = !!r.done, value = r.value, w;
    try { w = afsResolve.call(AfsPromise, value); }
    catch (e) { if (!done && closeOnRejection) closeSync(true); throw e; }
    return afsThen.call(w, function (v) { return { value: v, done: done }; },
      done || !closeOnRejection ? undefined : function (e) { closeSync(true); throw e; });
  }
  hide(it, 'next', function (v) {
    try { return cont(arguments.length > 0 ? syncNext.call(sync, v) : syncNext.call(sync), true); }
    catch (e) { return afsReject.call(AfsPromise, e); }
  });
  hide(it, 'return', function (v) {
    try {
      var m = sync.return;
      if (m === undefined || m === null) return afsResolve.call(AfsPromise, { value: v, done: true });
      var r = arguments.length > 0 ? m.call(sync, v) : m.call(sync);
      if (!isObject(r)) throw new TypeError('Iterator result is not an object');
      return cont(r, false);
    } catch (e) { return afsReject.call(AfsPromise, e); }
  });
  hide(it, 'throw', function (v) {
    try {
      var m = sync.throw;
      if (m === undefined || m === null) {
        closeSync(false);
        throw new TypeError('The iterator does not provide a throw method');
      }
      var r = m.call(sync, v);
      if (!isObject(r)) throw new TypeError('Iterator result is not an object');
      return cont(r, true);
    } catch (e) { return afsReject.call(AfsPromise, e); }
  });
  return it;
};
setAsyncFromSync(asyncFromSyncOf);

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

// ---- Intl (ECMA-402) without ICU: ComponentEngine's CLDR tables
// (intl_data.rs, from engine/LocaleData.rgr and LocalePlural.rgr) for the
// locales in `tags` -- number symbols, grouping and affixes, currency
// symbols, date patterns and names, plural categories, list separators --
// read through __cerIntlTable on first use. A locale outside them falls back
// to en-US, and resolvedOptions().locale says so. Collation is native
// (intl.rs): three levels with the per-locale tailorings.
// The implementation is intl.rs's INTL, compiled the first time a script
// reaches for Intl or a toLocale… method, so that a script that does not
// pays nothing for it.
var intlTable = globalThis.__cerIntlTable, intlCollate = globalThis.__cerIntlCollate, intlSource = globalThis.__cerIntlSource;
delete globalThis.__cerIntlTable;
delete globalThis.__cerIntlCollate;
delete globalThis.__cerIntlSource;
var INTL = null;
function intl() {
  if (!INTL) INTL = intlSource()(hide, tag, isObject, create, defineProperty, toIntegerOrInfinity, DP, dateOk, intlTable, intlCollate);
  return INTL;
}
defineProperty(globalThis, 'Intl', { enumerable: false, configurable: true,
  get: function () { return intl().Intl; },
  set: function (v) { defineProperty(globalThis, 'Intl', { value: v, writable: true, enumerable: false, configurable: true }); } });
hide(Number.prototype, 'toLocaleString', function toLocaleString(locales, options) {
  var x = typeof this === 'number' ? this : Number.prototype.valueOf.call(this);
  return intl().numberLocale(x, locales, options);
});
if (typeof BigInt === 'function') hide(BigInt.prototype, 'toLocaleString', function toLocaleString(locales, options) {
  var x = typeof this === 'bigint' ? this : BigInt.prototype.valueOf.call(this);
  return intl().numberLocale(x, locales, options);
});
hide(String.prototype, 'localeCompare', function localeCompare(that, locales, options) {
  'use strict';
  if (locales === undefined && options === undefined) {
    if (typeof this === 'string' && typeof that === 'string') return intlCollate(this, that, '', 3);
    if (this === null || this === undefined) throw new TypeError('String.prototype.localeCompare called on null or undefined');
    return intlCollate(String(this), String(that), '', 3);
  }
  if (this === null || this === undefined) throw new TypeError('String.prototype.localeCompare called on null or undefined');
  return intl().collate(String(this), String(that), locales, options);
});
hide(DP, 'toLocaleString', function toLocaleString(locales, options) { return intl().dateLocale(this, locales, options, 'all'); });
hide(DP, 'toLocaleDateString', function toLocaleDateString(locales, options) { return intl().dateLocale(this, locales, options, 'date'); });
hide(DP, 'toLocaleTimeString', function toLocaleTimeString(locales, options) { return intl().dateLocale(this, locales, options, 'time'); });

// ---- WeakRef / FinalizationRegistry (ES2021): the collector never runs
// a callback, so a held object simply stays
(function () {
function canHold(v) { return isObject(v) || (typeof v === 'symbol' && Symbol.keyFor(v) === undefined); }
// OrdinaryCreateFromConstructor: newTarget.prototype, or dflt when that is
// not an object
function fromCtor(self, nt, dflt) {
  var P = nt.prototype;
  if (!isObject(P)) P = dflt;
  return getPrototypeOf(self) === P ? self : create(P);
}
var refSlots = new WeakMap(), regSlots = new WeakMap();
function WeakRef(target) {
  if (new.target === undefined) throw new TypeError("Constructor WeakRef requires 'new'");
  if (!canHold(target)) throw new TypeError('WeakRef: invalid target');
  var self = fromCtor(this, new.target, WeakRef.prototype);
  refSlots.set(self, { t: target });
  return self;
}
defineProperty(WeakRef, 'prototype', { writable: false });
hide(WeakRef.prototype, 'deref', function deref() {
  var s = isObject(this) ? refSlots.get(this) : undefined;
  if (s === undefined) throw new TypeError('WeakRef.prototype.deref called on incompatible receiver');
  return s.t;
});
tag(WeakRef.prototype, 'WeakRef');
hide(globalThis, 'WeakRef', WeakRef);
function FinalizationRegistry(cleanup) {
  if (new.target === undefined) throw new TypeError("Constructor FinalizationRegistry requires 'new'");
  if (!isCallable(cleanup)) throw new TypeError('FinalizationRegistry: cleanup must be callable');
  var self = fromCtor(this, new.target, FinalizationRegistry.prototype);
  regSlots.set(self, { cleanup: cleanup, cells: [] });
  return self;
}
defineProperty(FinalizationRegistry, 'prototype', { writable: false });
function regSlot(o, name) {
  var s = isObject(o) ? regSlots.get(o) : undefined;
  if (s === undefined) throw new TypeError('FinalizationRegistry.prototype.' + name + ' called on incompatible receiver');
  return s;
}
hide(FinalizationRegistry.prototype, 'register', function register(target, heldValue) {
  var s = regSlot(this, 'register');
  var token = arguments.length > 2 ? arguments[2] : undefined;
  if (!canHold(target)) throw new TypeError('FinalizationRegistry.prototype.register: invalid target');
  if (target === heldValue) throw new TypeError('FinalizationRegistry.prototype.register: target and holdings must not be the same');
  if (token !== undefined && !canHold(token)) throw new TypeError('FinalizationRegistry.prototype.register: invalid unregister token');
  s.cells.push({ target: target, held: heldValue, token: token });
});
hide(FinalizationRegistry.prototype, 'unregister', function unregister(unregisterToken) {
  var s = regSlot(this, 'unregister');
  if (!canHold(unregisterToken)) throw new TypeError('FinalizationRegistry.prototype.unregister: invalid unregister token');
  var removed = false, keep = [];
  for (var i = 0; i < s.cells.length; i++) {
    if (s.cells[i].token !== undefined && s.cells[i].token === unregisterToken) removed = true;
    else keep.push(s.cells[i]);
  }
  s.cells = keep;
  return removed;
});
tag(FinalizationRegistry.prototype, 'FinalizationRegistry');
hide(globalThis, 'FinalizationRegistry', FinalizationRegistry);
})();
// ---- Explicit resource management: DisposableStack, AsyncDisposableStack,
// and what `using` / `await using` declarations call (__cerxesUsing)
(function () {
var SymDispose = Symbol.dispose, SymAsyncDispose = Symbol.asyncDispose;
var PromiseC = Promise, pResolve = Promise.resolve, pThen = Promise.prototype.then;
var Suppressed = SuppressedError;
function named(f, n) { return defineProperty(f, 'name', { value: n, configurable: true }); }
hide(AsyncIteratorProto, SymAsyncDispose, named(function () {
  var self = this;
  return new PromiseC(function (resolve, reject) {
    var m;
    try { m = self.return; } catch (e) { reject(e); return; }
    if (m === undefined || m === null) { resolve(undefined); return; }
    var r;
    try { r = m.call(self); } catch (e) { reject(e); return; }
    pThen.call(pResolve.call(PromiseC, r), function () { resolve(undefined); }, reject);
  });
}, '[Symbol.asyncDispose]'));
// GetDisposeMethod: the method, or (async) the sync one wrapped
function disposeMethod(v, async) {
  var m;
  if (async) {
    m = v[SymAsyncDispose];
    if (m === undefined || m === null) {
      var sm = v[SymDispose];
      if (sm === undefined || sm === null) return undefined;
      if (!isCallable(sm)) throw new TypeError('Symbol.dispose is not a function');
      return function () {
        var o = this;
        return new PromiseC(function (resolve) { sm.call(o); resolve(undefined); });
      };
    }
  } else {
    m = v[SymDispose];
    if (m === undefined || m === null) return undefined;
  }
  if (!isCallable(m)) throw new TypeError((async ? 'Symbol.asyncDispose' : 'Symbol.dispose') + ' is not a function');
  return m;
}
// AddDisposableResource
function addResource(list, v, async) {
  if (v === null || v === undefined) {
    if (async) list.push({ v: undefined, m: undefined });
    return;
  }
  if (!isObject(v)) throw new TypeError('the value of a using declaration must be an object, null or undefined');
  var m = disposeMethod(v, async);
  if (m === undefined) throw new TypeError((async ? 'Symbol.asyncDispose' : 'Symbol.dispose') + ' is not a function');
  list.push({ v: v, m: m });
}
function suppress(err, has, e) { return has ? new Suppressed(e, err, 'An error was suppressed during disposal') : e; }
// DisposeResources (sync): throws what the block threw (has / err) or what
// disposing threw
function disposeSync(list, has, err) {
  for (var i = list.length - 1; i >= 0; i--) {
    var r = list[i];
    try { r.m.call(r.v); } catch (e) { err = suppress(err, has, e); has = true; }
  }
  list.length = 0;
  if (has) throw err;
}
// DisposeResources (async): a promise of the same
function disposeAsync(list, has, err) {
  return new PromiseC(function (resolve, reject) {
    var i = list.length;
    function step() {
      for (;;) {
        i--;
        if (i < 0) { list.length = 0; if (has) reject(err); else resolve(undefined); return; }
        var r = list[i], x;
        try { x = r.m === undefined ? undefined : r.m.call(r.v); } catch (e) { err = suppress(err, has, e); has = true; continue; }
        pThen.call(pResolve.call(PromiseC, x), step, function (e) { err = suppress(err, has, e); has = true; step(); });
        return;
      }
    }
    step();
  });
}
var usingApi = {
  add: function (list, v, async) { addResource(list, v, async); return v; },
  dispose: function (list, has, err) { disposeSync(list, has, err); },
  disposeAsync: function (list, has, err) { return disposeAsync(list, has, err); }
};
defineProperty(globalThis, '__cerxesUsing', { value: usingApi, writable: false, enumerable: false, configurable: false });

var dsSlots = new WeakMap();
function slots(o, cls, name) {
  var s = isObject(o) ? dsSlots.get(o) : undefined;
  if (s === undefined || s.cls !== cls) throw new TypeError(cls + '.prototype.' + name + ' called on incompatible receiver');
  return s;
}
function live(s, cls) { if (s.disposed) throw new ReferenceError(cls + ' already disposed'); }
function makeStack(cls) {
  var C = function () {
    if (new.target === undefined) throw new TypeError("Constructor " + cls + " requires 'new'");
    var self = this;
    // OrdinaryCreateFromConstructor: new.target's prototype, else this one
    if (getPrototypeOf(self) !== new.target.prototype) self = create(P);
    dsSlots.set(self, { cls: cls, disposed: false, list: [] });
    return self;
  };
  named(C, cls);
  defineProperty(C, 'length', { value: 0, configurable: true });
  var P = C.prototype;
  defineProperty(C, 'prototype', { writable: false });
  var async = cls === 'AsyncDisposableStack';
  hide(P, 'use', function use(value) {
    var s = slots(this, cls, 'use'); live(s, cls);
    addResource(s.list, value, async);
    return value;
  });
  hide(P, 'adopt', function adopt(value, onDispose) {
    var s = slots(this, cls, 'adopt'); live(s, cls);
    if (!isCallable(onDispose)) throw new TypeError('onDispose is not a function');
    s.list.push({ v: undefined, m: function () { return onDispose.call(undefined, value); } });
    return value;
  });
  hide(P, 'defer', function defer(onDispose) {
    var s = slots(this, cls, 'defer'); live(s, cls);
    if (!isCallable(onDispose)) throw new TypeError('onDispose is not a function');
    s.list.push({ v: undefined, m: onDispose });
    return undefined;
  });
  hide(P, 'move', function move() {
    var s = slots(this, cls, 'move'); live(s, cls);
    var n = create(P);
    dsSlots.set(n, { cls: cls, disposed: false, list: s.list });
    s.list = []; s.disposed = true;
    return n;
  });
  var d;
  if (async) {
    d = function disposeAsync() {
      var s;
      try { s = slots(this, cls, 'disposeAsync'); } catch (e) { return new PromiseC(function (_, reject) { reject(e); }); }
      if (s.disposed) return pResolve.call(PromiseC, undefined);
      s.disposed = true;
      var list = s.list; s.list = [];
      return disposeAsyncList(list);
    };
    hide(P, 'disposeAsync', d);
    hide(P, SymAsyncDispose, d);
  } else {
    d = function dispose() {
      var s = slots(this, cls, 'dispose');
      if (s.disposed) return undefined;
      s.disposed = true;
      var list = s.list; s.list = [];
      disposeSync(list, false, undefined);
      return undefined;
    };
    hide(P, 'dispose', d);
    hide(P, SymDispose, d);
  }
  defineProperty(P, 'disposed', { get: asMethod(named(function () { return slots(this, cls, 'disposed').disposed; }, 'get disposed')), enumerable: false, configurable: true });
  tag(P, cls);
  hide(globalThis, cls, C);
}
function disposeAsyncList(list) { return disposeAsync(list, false, undefined); }
makeStack('DisposableStack');
makeStack('AsyncDisposableStack');
})();

// ---- ECMAScript modules: esm.rs writes each module's script (a generator
// in a `with` over its imports); this loads, links and evaluates them, and
// answers `import()` (pcomp: __cerxesESM.dynImport). The host hands in
// the sources (__cerxesESM.source): the loader reads no files.
(function () {
var esmCompile = globalThis.__cerxesESMCompile;
delete globalThis.__cerxesESMCompile;
var PromiseC = Promise, pResolve = Promise.resolve, pThen = Promise.prototype.then;
var indirectEval = eval, jsonParse = JSON.parse, ProxyC = Proxy, hasOwnN = Object.prototype.hasOwnProperty;
var preventExt = Object.preventExtensions, rGet = Reflect.get, rHas = Reflect.has, rDel = Reflect.deleteProperty;
var rGopd = Reflect.getOwnPropertyDescriptor, rDef = Reflect.defineProperty, rKeys = Reflect.ownKeys;
var aSort = Array.prototype.sort, objKeys = Object.keys, SyntaxErrorC = SyntaxError, TypeErrorC = TypeError;
var sources = create(null), records = create(null), AMB = {}, evalMark = 0;
function own(o, k) { return hasOwnN.call(o, k); }
function promise(f) { return new PromiseC(f); }
function later(f) { pThen.call(pResolve.call(PromiseC, undefined), f); }
function normalize(p) {
  var parts = p.split('/'), out = [];
  for (var i = 0; i < parts.length; i++) {
    var s = parts[i];
    if (s === '.' || (s === '' && i > 0 && i < parts.length - 1)) continue;
    if (s === '..' && out.length > 0 && out[out.length - 1] !== '..' && out[out.length - 1] !== '') { out.length = out.length - 1; continue; }
    out[out.length] = s;
  }
  return out.join('/');
}
function resolve(spec, ref) {
  if (spec.charAt(0) === '/') return normalize(spec);
  if (spec.charAt(0) === '.' && typeof ref === 'string') {
    var i = ref.lastIndexOf('/');
    return normalize((i < 0 ? '' : ref.slice(0, i + 1)) + spec);
  }
  return spec;
}
function load(path) {
  var r = records[path];
  if (r) return r;
  var src = sources[path];
  if (src === undefined) throw new TypeErrorC("Cannot find module '" + path + "'");
  var info = jsonParse(esmCompile(src, path, false)), make, async = false;
  try { make = indirectEval(info.text); } catch (e) {
    if (!(e instanceof SyntaxErrorC)) throw e;
    var info2 = jsonParse(esmCompile(src, path, true));
    try { make = indirectEval(info2.text); } catch (e2) { throw e; }
    info = info2; async = true;
  }
  r = { path: path, info: info, make: make, async: async, status: 'new', req: create(null), deps: [], getters: null,
        gen: null, ns: null, meta: create(null), hasError: false, error: undefined, pending: null, mark: 0 };
  records[path] = r;
  var q = info.requests;
  for (var i = 0; i < q.length; i++) {
    var d = load(resolve(q[i], path));
    r.req[q[i]] = d;
    r.deps[r.deps.length] = d;
  }
  return r;
}
function resolveExport(m, name, set) {
  for (var i = 0; i < set.length; i++) if (set[i].m === m && set[i].n === name) return null;
  set[set.length] = { m: m, n: name };
  var info = m.info, k;
  for (k = 0; k < info.locals.length; k++) if (info.locals[k] === name) return { m: m, n: name, ns: false };
  for (k = 0; k < info.indirect.length; k++) {
    var e = info.indirect[k];
    if (e[0] === name) {
      var t = m.req[e[1]];
      if (e[2] === '*') return { m: t, n: '*', ns: true };
      return resolveExport(t, e[2], set);
    }
  }
  if (name === 'default') return null;
  var star = null;
  for (k = 0; k < info.stars.length; k++) {
    var res = resolveExport(m.req[info.stars[k]], name, set);
    if (res === AMB) return AMB;
    if (res !== null) {
      if (star === null) star = res;
      else if (res.m !== star.m || res.n !== star.n || res.ns !== star.ns) return AMB;
    }
  }
  return star;
}
function exportedNames(m, set) {
  for (var i = 0; i < set.length; i++) if (set[i] === m) return [];
  set[set.length] = m;
  var info = m.info, names = [], k;
  for (k = 0; k < info.locals.length; k++) names[names.length] = info.locals[k];
  for (k = 0; k < info.indirect.length; k++) names[names.length] = info.indirect[k][0];
  for (k = 0; k < info.stars.length; k++) {
    var more = exportedNames(m.req[info.stars[k]], set);
    for (var j = 0; j < more.length; j++) {
      var n = more[j], seen = n === 'default';
      for (var q = 0; !seen && q < names.length; q++) if (names[q] === n) seen = true;
      if (!seen) names[names.length] = n;
    }
  }
  return names;
}
function binder(b) {
  if (b.ns) { var t = b.m; return function () { return namespace(t); }; }
  var m = b.m, n = b.n;
  return function () { return m.getters[n](); };
}
function sameValue(a, b) { return a === b ? (a !== 0 || 1 / a === 1 / b) : (a !== a && b !== b); }
function namespace(m) {
  if (m.ns) return m.ns;
  var names = exportedNames(m, []), list = [], i;
  for (i = 0; i < names.length; i++) {
    var b = resolveExport(m, names[i], []);
    if (b !== null && b !== AMB) list[list.length] = names[i];
  }
  aSort.call(list);
  var target = create(null), binds = create(null);
  for (i = 0; i < list.length; i++) {
    defineProperty(target, list[i], { value: undefined, writable: true, enumerable: true, configurable: false });
    binds[list[i]] = binder(resolveExport(m, list[i], []));
  }
  defineProperty(target, Symbol.toStringTag, { value: 'Module', writable: false, enumerable: false, configurable: false });
  preventExt(target);
  function isName(k) { return typeof k === 'string' && own(binds, k); }
  m.ns = new ProxyC(target, {
    get: function (t, k) { if (typeof k === 'symbol') return rGet(t, k); return own(binds, k) ? binds[k]() : undefined; },
    set: function () { return false; },
    has: function (t, k) { return typeof k === 'symbol' ? rHas(t, k) : own(binds, k); },
    deleteProperty: function (t, k) { return typeof k === 'symbol' ? rDel(t, k) : !own(binds, k); },
    getOwnPropertyDescriptor: function (t, k) {
      if (typeof k === 'symbol') return rGopd(t, k);
      if (!own(binds, k)) return undefined;
      return { value: binds[k](), writable: true, enumerable: true, configurable: false };
    },
    defineProperty: function (t, k, d) {
      if (typeof k === 'symbol') return rDef(t, k, d);
      if (!own(binds, k)) return false;
      var v = binds[k]();
      if (own(d, 'configurable') && d.configurable) return false;
      if (own(d, 'enumerable') && !d.enumerable) return false;
      if (own(d, 'get') || own(d, 'set')) return false;
      if (own(d, 'writable') && !d.writable) return false;
      if (own(d, 'value')) return sameValue(d.value, v);
      return true;
    },
    ownKeys: function (t) { return rKeys(t); },
    getPrototypeOf: function () { return null; },
    setPrototypeOf: function (t, p) { return p === null; },
    isExtensible: function () { return false; },
    preventExtensions: function () { return true; }
  });
  return m.ns;
}
function noAssign() { throw new TypeErrorC('Assignment to constant variable.'); }
function instantiate(r) {
  var info = r.info, imp = create(null), k, b;
  for (k = 0; k < info.imports.length; k++) {
    var e = info.imports[k], t = r.req[e[1]];
    if (e[2] === '*') b = { m: t, n: '*', ns: true };
    else {
      b = resolveExport(t, e[2], []);
      if (b === null) throw new SyntaxErrorC("The requested module '" + e[1] + "' does not provide an export named '" + e[2] + "'");
      if (b === AMB) throw new SyntaxErrorC("The requested module '" + e[1] + "' contains conflicting star exports for name '" + e[2] + "'");
    }
    defineProperty(imp, e[0], { get: binder(b), set: noAssign, enumerable: true, configurable: false });
  }
  for (k = 0; k < info.indirect.length; k++) {
    if (info.indirect[k][2] === '*') continue;
    b = resolveExport(r, info.indirect[k][0], []);
    if (b === null) throw new SyntaxErrorC("The requested module '" + info.indirect[k][1] + "' does not provide an export named '" + info.indirect[k][2] + "'");
    if (b === AMB) throw new SyntaxErrorC("The requested module '" + info.indirect[k][1] + "' contains conflicting star exports for name '" + info.indirect[k][2] + "'");
  }
  var body = r.make(imp, r.meta, r.path, function (g) { r.getters = g; });
  r.gen = body();
  r.gen.next();
}
function linkAll(r, touched) {
  if (r.status !== 'new') return;
  r.status = 'linking';
  touched[touched.length] = r;
  for (var i = 0; i < r.deps.length; i++) linkAll(r.deps[i], touched);
  instantiate(r);
}
function link(r) {
  var touched = [];
  try { linkAll(r, touched); } catch (e) {
    for (var i = 0; i < touched.length; i++) { touched[i].status = 'new'; touched[i].gen = null; touched[i].getters = null; }
    throw e;
  }
  for (var j = 0; j < touched.length; j++) touched[j].status = 'linked';
}
function order(r, mark, out) {
  if (r.mark === mark) return;
  r.mark = mark;
  if (r.status !== 'evaluated') for (var i = 0; i < r.deps.length; i++) order(r.deps[i], mark, out);
  out[out.length] = r;
}
function setError(m, e) { m.hasError = true; m.error = e; }
function depError(m) {
  for (var i = 0; i < m.deps.length; i++) if (m.deps[i].hasError && m.deps[i] !== m) return m.deps[i];
  return null;
}
// the modules of r's graph in post order; undefined when they all ran
// synchronously, else the promise of their end
function evaluate(root) {
  var list = [], i, async = false;
  order(root, ++evalMark, list);
  for (i = 0; i < list.length; i++) if (list[i].pending || (list[i].async && list[i].status !== 'evaluated')) async = true;
  if (!async) {
    for (i = 0; i < list.length; i++) {
      var m = list[i];
      if (m.status === 'evaluated') continue;
      var bad = depError(m);
      if (bad) { m.status = 'evaluated'; setError(m, bad.error); continue; }
      m.status = 'evaluated';
      try { m.gen.next(); } catch (e) { setError(m, e); }
    }
    if (root.hasError) throw root.error;
    return undefined;
  }
  return promise(function (res, rej) {
    var at = 0;
    function step() {
      while (at < list.length) {
        var m = list[at++];
        if (m.status === 'evaluated') {
          if (m.pending) { wait(m); return; }
          continue;
        }
        var bad = depError(m);
        m.status = 'evaluated';
        if (bad) { setError(m, bad.error); continue; }
        if (m.async) {
          var pr;
          try { pr = m.gen.next(); } catch (e) { setError(m, e); continue; }
          m.pending = pr;
          wait(m);
          return;
        }
        try { m.gen.next(); } catch (e2) { setError(m, e2); }
      }
      if (root.hasError) rej(root.error); else res(undefined);
    }
    function wait(m) {
      pThen.call(m.pending, function () { m.pending = null; step(); }, function (e) { m.pending = null; setError(m, e); step(); });
    }
    step();
  });
}
var api = {
  failed: false, failure: undefined,
  source: function (path, text) { sources[path] = text; },
  named: function (v, decl) {
    if (typeof v === 'function' && (decl || !own(v, 'name') || v.name === '')) defineProperty(v, 'name', { value: 'default', configurable: true });
    return v;
  },
  runMain: function (path) {
    var r = load(path);
    link(r);
    var p = evaluate(r);
    if (p) pThen.call(p, undefined, function (e) { api.failed = true; api.failure = e; });
  },
  check: function () { if (api.failed) throw api.failure; },
  dynImport: function (spec, ref, opts) {
    var ok, fail;
    var p = promise(function (a, b) { ok = a; fail = b; });
    try {
      var s = `${spec}`;
      if (opts !== undefined) {
        if (!isObject(opts)) throw new TypeErrorC('The second argument of import() must be an object');
        var w = opts['with'];
        if (w !== undefined) {
          if (!isObject(w)) throw new TypeErrorC("The 'with' option of import() must be an object");
          var ks = objKeys(w);
          for (var i = 0; i < ks.length; i++) if (typeof w[ks[i]] !== 'string') throw new TypeErrorC('Import attribute values must be strings');
        }
      }
    } catch (e) { fail(e); return p; }
    later(function () {
      try {
        var r = load(resolve(s, ref));
        link(r);
        var q = evaluate(r);
        if (q) pThen.call(q, function () { ok(namespace(r)); }, fail);
        else ok(namespace(r));
      } catch (e) { fail(e); }
    });
    return p;
  }
};
defineProperty(globalThis, '__cerxesESM', { value: api, writable: false, enumerable: false, configurable: false });
defineProperty(globalThis, '__cerxesRef', { value: undefined, writable: true, enumerable: false, configurable: true });
})();
})();"#;
