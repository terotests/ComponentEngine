# Bug-class seeds from other engines (fuzzer v3)

One program per published bug (or bug class) of another JavaScript engine,
rewritten for CErXes: not the original exploit or test, but the shape that
made that engine go wrong (user code running where the engine assumed none
could, a length that changes under a builtin, a value a JIT assumed had a
type or range). Each prints what it sees, so the differential oracles (the
register VM vs the stack VM, snapshots, `--node`) compare the results, and
`cerxes-fuzz run` gives them bug-class hooks (src/hooks.rs) at
`--class P` %.

The first line says where the shape comes from:
`// <engine>: <CVE or report> <class>`. Engines: jsc (JavaScriptCore),
sm (SpiderMonkey), qjs (QuickJS), hermes, chakra (ChakraCore), v8, and
cerxes for the classes that matter because CErXes counts references and
moves its state between machines (a callback that drops the last reference
to what a builtin walks; a builtin suspended mid-way by a slice).

Where a report has no CVE number we know for certain, the line names the
class and the engines that had it instead of guessing a number.

| Seed | Source and class |
|---|---|
| `cerxes-snapshot-mid-reentrancy.js` | cerxes: re-entrant builtins suspended and resumed across slices (snapshot oracle: CErXes travelling engine) snapshot |
| `chakra-array-kind-transition.js` | chakra: native int / float / var array conversions while a builtin runs (lokihardt Chakra JIT and runtime bugs) array-kind |
| `chakra-implicit-call-in-hot-loop.js` | chakra: implicit calls (has trap, Symbol.hasInstance, getters) not checked in JIT loops (lokihardt) jit-sideeffect |
| `chakra-initproto-type-confusion.js` | chakra: CVE-2019-0567 (InitProto makes an object a prototype in the JIT, lokihardt / saelo) jit-shape |
| `chakra-length-nonwritable.js` | chakra: arrays with a non-writable length or frozen elements in push / unshift / splice / sort (Chakra, JSC) array-length |
| `chakra-stack-arguments-escape.js` | chakra: stack-allocated arguments / closure escaping from an inlined function (lokihardt Chakra bugs) jit-arguments |
| `hermes-apply-arraylike.js` | hermes: Function.prototype.apply / spread of an array-like whose length getter changes (Hermes, Chakra) size-overflow |
| `hermes-concat-spreadable.js` | hermes: Array.prototype.concat type confusion through isConcatSpreadable (Hermes advisories 2020-2021) species |
| `hermes-generator-eval-finally.js` | hermes: generators compiled lazily / from eval, resumed through finally (Hermes generator CVEs) iterator |
| `hermes-regexp-lookbehind-backref.js` | hermes: regexp engine OOB in lookbehind / backreferences / case folding (Hermes, QuickJS, V8 Irregexp) regexp |
| `jsc-arguments-elimination.js` | jsc: arguments elimination / mapped arguments aliasing (several JSC DFG bugs) jit-arguments |
| `jsc-arithnegate-cse.js` | jsc: CVE-2020-9802 (Project Zero "JITSploitation I", ArithNegate CSE / bounds check) jit-bounds |
| `jsc-array-species-from-builtin.js` | jsc: species constructors returning exotic objects to map / filter / splice (JSC, V8, Chakra) species |
| `jsc-concat-length-overflow.js` | jsc: lokihardt 2017 concat / spread length overflow (Project Zero) size-overflow |
| `jsc-forin-enumerator-cache.js` | jsc: for-in structure / enumerator cache invalidation (several JSC JIT bugs) jit-shape |
| `jsc-json-stringify-proxy-length.js` | jsc: JSON.stringify of a Proxy array whose length changes (JSC / V8 stringify bugs) proxy |
| `jsc-regexp-lastindex-jit.js` | jsc: Pwn2Own 2018 regexp lastIndex side effect in JIT (saelo) jit-sideeffect |
| `jsc-slice-valueof-shrink.js` | jsc: CVE-2016-4622 (Phrack 70 "Attacking JavaScript Engines") array-reentrancy |
| `jsc-typedarray-sort-detach.js` | jsc: TypedArray.prototype.sort comparator detaches the buffer (JSC, QuickJS, V8 had this) typedarray-detach |
| `qjs-async-generator-return.js` | qjs: async generator return / throw while awaiting (QuickJS UAF reports, V8 async generator bugs) iterator |
| `qjs-fast-array-valueof.js` | qjs: fast-array paths that keep a pointer while valueOf runs (QuickJS bug tracker, lastIndexOf / fill / copyWithin / with) array-reentrancy |
| `qjs-proxy-isarray-depth.js` | qjs: CVE-2023-31922 (QuickJS js_proxy_isArray stack overflow) native-recursion |
| `qjs-typedarray-set-detach.js` | qjs: TypedArray.prototype.set / subarray / DataView on a detached or shrunk buffer (QuickJS, JSC, V8) typedarray-detach |
| `qjs-with-unscopables.js` | qjs: with + Symbol.unscopables getter that deletes the binding (QuickJS / JSC scope bugs) scope |
| `rc-drop-receiver-in-callback.js` | cerxes: UAF class of refcounted engines (QuickJS, Hermes handle bugs) adapted to CErXes reference counting refcount |
| `rc-error-cause-cycle.js` | cerxes: cycles through Error.cause, closures and bound functions dropped mid-use (refcount cycle class) refcount |
| `rc-iterator-drops-collection.js` | cerxes: iterators whose collection or generator is dropped and collected while they run (UAF class) refcount |
| `rc-revoke-in-trap.js` | cerxes: Proxy revoked from inside its own trap (Proxy UAF class: V8, JSC, QuickJS) refcount |
| `sm-array-pop-type-confusion.js` | sm: CVE-2019-11707 (IonMonkey Array.prototype.pop type inference) jit-proto |
| `sm-generator-reentry.js` | sm: generator closed or resumed from inside itself (SpiderMonkey / Hermes generator bugs) iterator |
| `sm-object-prototype-then.js` | sm: CVE-2022-1802 class (prototype pollution reaching internal promise code), V8 await bugs iterator-promise |
| `sm-range-analysis.js` | sm: CVE-2024-29943 (Pwn2Own 2024, range analysis of bounds check) jit-bounds |
| `sm-slice-species-alias.js` | sm: CVE-2019-9810 (IonMonkey assumed Array.prototype.slice has no side effects, Pwn2Own 2019) species |
| `sm-storeelementhole-setter.js` | sm: CVE-2019-17026 (IonMonkey StoreElementHole ignored an index setter on the prototype) jit-proto |
| `v8-array-sort-comparator-shrink.js` | v8: Array.prototype.sort comparator shrinks / grows / re-protos the array (V8, JSC, QuickJS) array-reentrancy |
| `v8-reflect-construct-map-change.js` | v8: CVE-2020-6418 (TurboFan JSCreate side effect: Reflect.construct with a Proxy new.target) jit-sideeffect |

What the seeds found when first run (2026-11-15) is in `../../README.md`
(Fuzzer v3).
