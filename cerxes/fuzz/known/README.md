# Findings of the first runs (2026-11-12)

Each file reproduces with `cerxes-fuzz repro <file>`, or with the CLI where
noted. When one is fixed, its file can stay here as a regression case.

| File | Class | What happens |
|---|---|---|
| `hang-lone-backslash.js` | hang | A source text that is only `\` (or ends in `\`) never finishes: `target/release/cerxes hang-lone-backslash.js` runs forever. Slices do not help because it happens before the script runs, while it is read. |
| `panic-tree-compiler-vm8055.js` | panic | Tree compiler (`Engine::new_with(false)`, also the TypeScript path's fallback): index out of bounds (index -1) at vm.rs:8055. |
| `stack-overflow-map-proto.js` | stack-overflow | A Map whose `__proto__` is set to an array overflows the Rust stack and aborts the process. |
| `mismatch-snapshot-regexp-source.js` (+ .txt, its seed) | mismatch-snapshot | After a snapshot and restore, a RegExp's `source` reads `undefined`. Seen with every regexp the generator writes. |

`--heavy` also found `oom` right away: `'ab'.repeat(2 ** 31)` and
`new Array(2 ** 32 - 1).fill(0)` abort the process when an allocation fails.
They do not throw a RangeError, because CErXes has no heap limit. Deep
recursion (`r(1e6)`) overflows the stack the same way.
