# CErXes — CEr with TypeScript and JSX

[CEr](../cer) reads JavaScript. CErXes reads TypeScript and JSX as well, and
hands CEr the same syntax tree (`cer::ast`), so CEr's compiler, VM and
built-ins run the result unchanged. Only the front-end is new: CEr's lexer
and parser, grown. CEr is unchanged apart from `Engine::run_ast` /
`run_ast_typed`, which run a tree another front-end parsed; its own
parser, and so its speed on `.js`, are the same.

```sh
cargo run --release --bin cerxes -- app.tsx   # .ts TS, .tsx both, .js/.jsx JSX
npm run cerxes:test                           # the syntax tests (tests/syntax.rs)
npm run cerxes:conformance                    # CEr's conformance probes, read as TSX
```

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
  and export lists and re-exports go. A value `import` is an error: CEr has
  no module system.

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

The default runtime (`src/prelude.rs`, three non-enumerable globals a
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
reading them as TSX: 1,667 agree with Node, the same probes as CEr.
