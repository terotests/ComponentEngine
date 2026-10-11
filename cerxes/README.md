# CErXes — CEr with TypeScript and JSX

[CEr](../cer) reads JavaScript. CErXes reads TypeScript and JSX as well, and
builds the same syntax tree CEr's parser builds, so CEr's compiler, VM and
built-ins run the result unchanged. Only the front-end is new: CEr's lexer
and parser, grown. CEr's own files are not changed; its speed on `.js` is
its own.

Like CEr, CErXes is a strict Rust module (Ranger's
[PLAN_RUST_SYNTAX](https://github.com/terotests/Ranger/blob/master/docs/plans/PLAN_RUST_SYNTAX.md)):
the same source builds with cargo and with `rgrc` into the Ranger targets.
`rgrc` reads the modules of one crate, not a crate's Cargo dependencies, so
CEr's back-end is in this crate as links to CEr's files (`src/ast.rs`,
`src/compiler.rs`, `src/vm.rs`, `src/value.rs`, `src/builtins*.rs`,
`src/regex.rs`, `src/num.rs`, `src/jsstr.rs`, `src/ops.rs`,
`src/prelude.rs` → `../cer/src`) rather than a dependency on the `cer`
crate. (A checkout needs symbolic links: on Windows, git's
`core.symlinks`.)

```sh
cargo run --release --bin cerxes -- app.tsx   # .ts TS, .tsx both, .js/.jsx JSX
npm run cerxes:build                          # rgrc: bin/Cerxes.cjs
npm run cerxes:test                           # tests/cases.txt on cargo and on rgrc's JS
node cerxes/bench/cases.mjs --targets=js,cpp,go --build   # … and C++ and Go
npm run cerxes:conformance                    # CEr's conformance probes, read as TSX
```

`tests/cases.txt` holds the syntax cases (a script and the value it
answers). `tests/syntax.rs` runs them on the cargo build, and CEr's own
cases (`../cer/tests/cases.txt`, plain JavaScript, values from Node) both
as JavaScript and as TSX; `bench/cases.mjs` runs the same on the Ranger
builds: JavaScript (`rgrc -es6 -nodemodule src/lib.rs`) and C++ / Go
(`bench/CerxesMain.rgr` through `rgrc -l=cpp|go`, then g++ / go). All of
them pass every case.

CI (`.github/workflows/ci.yml`) runs these, CEr's cases on CEr (cargo and
rgrc JS), and the conformance probes on CEr and CErXes with `--min` (no
fewer agreeing probes than today); its "All tests" check fails unless all
of them passed. `.github/rulesets/protect-main.json` is a ruleset that
requires that check (and a pull request) before anything reaches `main`.

```rust
let mut e = cerxes::Engine::new();          // TSX by default
e.set_syntax(true, false);                  // or: TypeScript only, JSX only…
let r = e.eval("const n: number = 1; n");
```

## TypeScript

Types are erased as they are read, and leave no nodes: annotations on
variables, parameters, returns and fields; type parameters and arguments
(`f<T>(x)`, `new Map<K, V>()`, `<T,>(x: T) => x`); `as`, `satisfies`,
`as const`, the non-null `!`; `<T>x` assertions (in `.ts`; in TSX `<T>` is
an element); type predicates; `interface`, `type`, `declare …`, overload
and `abstract` signatures, index signatures, `implements`, and imports of
types only (`import type`, `import { type A }`). The type grammar is read
the way `tsc` reads it, so `a < b > (c)` is a generic call there too.

What has a runtime meaning becomes the JavaScript `tsc` emits:

- `enum` (and `const enum`): the object with reverse mapping for numbers;
  members name each other bare in initializers (`AB = A | B`).
- `namespace` / `module` (dotted too): the object an IIFE fills; exported
  declarations are copied onto it after they run.
- constructor parameter properties (`constructor(private x: number)`):
  `this.x = x`, after `super(…)` in a derived class.
- `import A = B.C`: `var A = B.C`.
- `export` is dropped from a declaration, `export default e` evaluates `e`,
  and export lists and re-exports go.
- A value `import` reads a module the host defined with
  `defineModule("name", exports)`: `import D, { a, b as c } from "name"` is
  `const D = exports.default, a = exports.a, c = exports.b` where the
  statement stands, `import * as N` the whole object, `import "name"` only
  asks for it. An undefined module throws `Cannot find module 'x'`, naming
  the ones there are. There is no file loading: a module is what the host
  gives (a program's runtime, a library it ships).

Not there: decorators, and type checking — like `tsc --noCheck` or
Node's type stripping, CErXes runs what a type-correct program would.

## JSX

The classic transform: an element is a call of the factory with its type,
its props (`null` when it has none) and its children.

```text
<div id="a" {...p}>Hi {name}</div>  →  __jsx("div", {id: "a", ...p}, "Hi ", name)
<Card.Title />                      →  __jsx(Card.Title, null)
<>a</>                              →  __jsx(__jsxFragment, null, "a")
```

A tag starting lower-case, or holding `-` or `:`, is a string; otherwise
the value of that name. Text follows React's whitespace rule and takes HTML
character references (`&amp;`, `&#123;`, `&copy;` …). `/** @jsx h */` and
`/** @jsxFrag Frag */` name another factory and fragment.

The default runtime (`src/jsx_runtime.rs`, three non-enumerable globals a
script may replace):

- `__jsx`: a host tag becomes `{ type, props, children }` — `children`
  flat, without `null`, `undefined` or booleans. A function tag is a
  component, called at once with its props (`children` among them), as
  ComponentEngine expands components; a class with `render` is constructed
  and rendered.
- `__jsxFragment`: a fragment is the array of its children, spliced into
  its parent.
- `renderToString(node)`: markup for a tree (`className` → `class`, a
  `style` object → CSS).

## JavaScript out

`cerxes::emit` writes a script back out as plain JavaScript, so a browser's
own engine can run what CErXes runs, with no WebAssembly engine beside it:

```sh
cargo run --release --bin cerxes -- --emit app.tsx   # the JavaScript
cargo run --release --bin cerxes -- --runtime        # what it expects defined first
```

```rust
let r = cerxes::emit::emit_js_for_path(src, "app.tsx");   // r.js, or r.error
```

It is CErXes's own reading, printed (`src/emit.rs`): the tree the parser
builds is already JavaScript (types erased, JSX as `__jsx(…)` calls, `enum`
and `namespace` as objects, parameter properties as `this.x = x`, a value
`import` as a `const` read from `__cerxesImport("m")`), so the output means
what CErXes runs, with the same syntax errors. Statements stay on the lines
they were on, so an error's line in the output is the line in the `.tsx`;
comments and indentation are not kept. Class fields go back to being
fields (the parser turns them into initializer methods).

The output calls what every CErXes script has: `__jsx`, `__jsxFragment`,
`renderToString`, `defineModule` and `__cerxesImport`. `emit::runtime()`
(`--runtime`) defines them; run it once before the scripts. The
WebAssembly build has both: `cx_emit(src, len, path, len)` and
`cx_runtime()` (`playground/cerxes-wasm`).

Checked two ways: `tests/emit.rs` writes out every syntax case (and CEr's
cases, as JavaScript and as TSX) and runs the output as plain JavaScript
in a fresh engine, which must answer what the source answers; and
`bench/emit.mjs` writes out the 2,143 conformance probes with the rgrc
JavaScript build and runs the output in Node, which must answer what Node
answers for the probe itself. All 2,121 probes CErXes reads agree, except 7
that CErXes itself reads differently from Node (a comment ended by U+2028
or U+2029, an escaped `"use strict"`, a function's source text), where
the output means what CErXes read.

## How it works

- `lexer.rs`: CEr's lexer, reading one token at a time, each with its
  start. A token it cannot read becomes a `T_ERROR` token, an error only
  when the parser reaches it.
- `parser.rs`: CEr's parser over those tokens, with TypeScript's forms in
  functions, classes and expressions. It reads ahead with no cost to undo:
  `f<T>(x)` against `a < b` is tried and taken back (`types.rs`: `save` /
  `restore`), and so is an arrow's head with a return type.
- `types.rs`: the type grammar, skipped; object and tuple types as
  balanced brackets. A `>>` closing two type argument lists is split.
- `ts.rs`: `enum`, `namespace`, `declare`, `import` / `export`.
- `jsx.rs`: at a `<` that opens an element the parser drops the tokens
  read ahead, reads the element character by character from the lexer,
  and goes back to tokens for each `{…}` inside it.

## Conformance

`npm run cerxes:conformance` runs the 2,143 probes of Ranger's
`tests/runtime-conformance.test.ts` (plain JavaScript) through CErXes,
reading them as TSX: 1,938 agree with Node.
