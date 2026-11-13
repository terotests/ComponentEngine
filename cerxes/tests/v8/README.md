# Known V8 bug classes against CErXes

V8's security bugs fall into a few classes that come back year after year.
Each test here takes the pattern of one class and checks that CErXes stays
safe and gives V8's answer. V8-specific exploit payloads are not copied: they
depend on V8's object layout and would not mean anything in another engine.
What carries over is the pattern that broke V8: a builtin that keeps a length
across a user callback, a JIT that trusts a type or a range it guessed, a
limit nobody checks.

```sh
node cerxes/tests/v8/run.mjs own                  # own/*.js on regs, stack, tree, snap, wjit vs Node
node cerxes/tests/v8/run.mjs own --filter=fill    # some of them
node cerxes/tests/v8/run.mjs mjsunit --v8=../v8   # V8's own regression tests (needs a V8 checkout)
```

A V8 checkout for `mjsunit` (test files only, ~18 MB):

```sh
git clone --depth 1 --filter=blob:none --sparse https://github.com/v8/v8
cd v8 && git sparse-checkout set test/mjsunit/regress
```

## Configurations

| name | what runs |
|---|---|
| `node` | the reference: Node in script mode, `--expose-gc` |
| `regs` | `target/release/cerxes`: register VM + pcomp, the product default |
| `stack` | `CER_REGS=0`: the stack VM |
| `tree` | `CER_PCOMP=0`: the tree compiler |
| `slice` | `CERXES_SLICE_MS=0.05` |
| `snap` | `fuzz/target/release/snaprun`: 0.05 ms slices, a snapshot and a restore into a new engine at every one |
| `wjit` | `bench/wjrun.mjs`: the WebAssembly build with the baseline JIT, every loop compiled at once (min 0) |

Every CErXes run is under `ulimit -v` (3 GB by default, `--mem-mb`), with a
timeout (`--timeout`, 20 s).

## Verdicts

`same` (prints what Node prints), `differs`, and the crash classes `panic`,
`stack-overflow` (native stack, an abort), `oom` (an allocation failed under
the limit), `hang`, `wasm-trap` (the WebAssembly instance died: a panic is
`unreachable`, a deep native recursion `memory access out of bounds` or the
host's `Maximum call stack`), `crash:SIG…`.

`known.json` holds today's verdict of every own test. A run prints what
changed against it and fails when a test got a crash class it did not have.
`out/own-known.json` is written by each run; copy it over `known.json` when a
fix lands.

## own/

One file per class; every `T(...)` / `A(...)` (async) / `I(...)` (Intl) line
is one test, run as a script of its own with the rest of its file as setup.

| file | class | V8 history (examples) |
|---|---|---|
| 01-array-reentrancy | Array builtins whose valueOf / callbacks / getters change the array they walk; length limits; prototype elements in holes | Array.prototype.fill/copyWithin with a shrinking array (crbug 1426357), concat fast path (regress-599414), lastIndexOf/join/sort on a changed array |
| 02-typedarray-buffer | TypedArray / DataView / ArrayBuffer detached or resized during a call | TypedArray fill/sort/set on a detached buffer, resizable buffers (2023+), species returning a small array |
| 03-proxy-getter | Proxy invariants, getters that delete or add properties during Object.assign / entries / JSON, Map/Set iteration with mutation, deep proxy chains | Reflect.ownKeys duplicates, JSON.parse reviver (regress-729671), proxy chains (regress-7716) |
| 04-regexp | RegExp builtins that re-enter user code (lastIndex, exec, flags, species), huge quantifiers and groups, ReDoS, unicode | lastIndex valueOf recompiling the regexp, deep groups (regress-484904778) |
| 05-jit-types | values a JIT may have assumed away: -0, NaN, int32 overflow, index truncation, holes, map and prototype changes after warm-up | Math.expm1(-0) typer (crbug 880207), CVE-2021-21220 / -21224 truncation, CVE-2019-5782 arguments length, CVE-2020-6418 and CVE-2023-3079 maps and elements kinds |
| 06-limits-strings-numbers | string and number builtins at their size limits, BigInt sizes, number formatting buffers | String repeat/padStart/join overflow, toString(radix) buffers, BigInt size limit (crbug 909614) |
| 07-parser | deep nesting, long inputs, Function constructor parameter injection, early errors | parser stack overflows, `new Function("/*", "*/){")` |
| 08-iter-gen-async-gc | generators re-entered, iterator close, Promise combinators re-entered, FinalizationRegistry, collection during callbacks | Promise.all resolve element re-entrancy, async generator queue |
| 09-class-scope-intl | private fields and brand checks, super, TDZ, Function properties, Intl options | private brand on proxies and return-override objects, TDZ hole leaks |
| 10-depth | native recursion depth: every case should end in a catchable RangeError | many: V8 has a stack limit everywhere |
| 11-snapshot | CErXes only: state a snapshot taken in the middle must keep (for-in over a changing shape, suspended generators, iterators, lastIndex, sort in progress) | nearest: V8 snapshot/code-cache deserializer bugs |

After a run, `out/own/` holds every test as a script of its own; the fuzzer
can mutate them: `cerxes-fuzz run --corpus cerxes/tests/v8/out/own`.

## Results

| date | same | differs | crash classes | change |
|---|---|---|---|---|
| 2026-11-13 first run (#153) | — | — | 220 | panics, aborts on deep recursion, OOM on string/BigInt limits, wasm traps |
| 2026-11-13 #159 | 2606 | 347 | 22 | radix buffers, BigInt size limit, wasm32 index overflow, snapshot for-in, parser depth |
| 2026-11-13 #160 (on top of the 8-byte Val, #161) | 2654 | 304 | 7 (hang) | see below |

No test panics, aborts, runs out of memory or traps in WebAssembly any more; the 7 hangs are 08-for-of-next-getter-once (all five configurations) and two snap cases, listed below.

### Fixed in this round

- WebAssembly stack: `native_sp()` sees only the shadow stack, and V8's
  machine stack ran out first on deep JSON.parse / flat / join / proxy chains.
  The VM budget in wasm32 is now 128 KB and the parser's 256 KB
  (`parse_stack_budget`); every deep case ends in a RangeError. Deep `!!!…`
  is checked in `unary()` of all three parsers.
- Proxy `get` and call traps check the stack (deep proxy chains).
- Array.prototype.flat walks with its own stack (10 000 levels, then RangeError).
- `'a'+'a'+…` with 100k terms: string `+` appends in place when the left side
  is long (was quadratic, ran out of memory).
- `for (var e = of(x); …)`: a name `of` in a for head was read as for-of
  (minified Tailwind/Radix).
- toPrecision is exact (rust target), lastIndexOf clamps its start,
  Date.parse rejects year -000000, global match/matchAll/replace/split step
  over a surrogate pair after an empty match in u/v mode.
- Object.entries/values skip keys an earlier getter deleted or made
  non-enumerable.
- `arguments[i] = v` reaches a lazily mapped parameter.
- Private fields can be added to a frozen object; isFrozen/isSealed ignore them.
- A derived constructor returning `undefined` explicitly checks `this`.
- Function constructor: parameters or body that close a bracket they did not
  open (`new Function("/*", "*/){")`) are a SyntaxError.
- Error.captureStackTrace exists.

### Open, on purpose or for later

| tests | why |
|---|---|
| 01-proto-setter-push, 01-reverse-holes-proto, 01-join/with/flat-*-shrink, keyed-store-proto-setter | prototype elements seen through holes and arrays shrinking under valueOf: CErXes arrays are dense and do not look up the prototype for holes; a fix touches every array builtin |
| 01-index-2_32-2 | array index 2^32-2 is kept as a property, length stays 0 |
| 01-apply-big-arraylike, 01-spread-big, join-sparse-huge, replace-growth, string-raw limits | CErXes has higher size limits than V8 and gives the result where V8 throws RangeError |
| 02-ctor-offset-valueOf-detach, 02-slice-detach | detaching inside the constructor/slice is not checked again after valueOf |
| 03-defineProperties-getter, 03-Object.groupBy-getter | descriptor/group order when a getter changes the object |
| 03-structuredClone-getter, btoa cases | host APIs, not in the engine |
| 04 exec-override, species, named-getter, deep-groups, backref-named-dup | RegExp builtins do not call a user `exec`; species; duplicate named groups |
| 07-*-html-comment | Annex B HTML comments are not parsed |
| 07-deep-parens/array/object-1k, deep funcs/arrows, 03-json-deep-parse, 03-proxy-deep-proto | CErXes depth limits differ from V8's (lower for the parser; with 8-byte values a 5000-deep proxy chain fits where V8 throws); the answer is a RangeError or the value, never a crash |
| 07-assign-to-call | SyntaxError where V8 throws ReferenceError at run time |
| prepareStackTrace | Error.prepareStackTrace is not supported |
| 08-for-of-next-getter-once (hang) | CErXes reads `next` on every step, V8 once; the fix touches every iterator path |
| 08 async-iter-close, array-iterator-proto-next-override, deep yield*, deep await | iterator close in async loops, overridden %ArrayIteratorPrototype%.next, deep generator chains |
| global-function-over-nonconfig | global function declaration over a non-configurable property |
| 09 Intl validation, toLocaleString-reenter | Intl option validation order |
| 04-ReDoS-nested, 04-ReDoS-alt | the regex step budget from #158 stops catastrophic backtracking with a RangeError; V8 runs it to the end |
| snap many-vars-100k, many-object-keys (hang) | the harness snapshots every 0.05 ms; 100k-entry states are slow to copy that often, not a fault |
