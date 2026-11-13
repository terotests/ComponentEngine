# Findings of the first runs (2026-11-12)

Each file reproduces with `cerxes-fuzz repro <file>`, or with the CLI where
noted. All four are fixed (2026-11-13); the files stay as regression cases
and `repro` should now answer `ok`.

| File | Class | What happened | Fix |
|---|---|---|---|
| `hang-lone-backslash.js` | hang | A source text that is only `\` (or ends in `\`) never finishes: `target/release/cerxes hang-lone-backslash.js` runs forever. Slices do not help because it happens before the script runs, while it is read. | `read_ident` steps over a `\` that starts no escape (both lexers). |
| `panic-tree-compiler-vm8055.js` | panic | Tree compiler (`Engine::new_with(false)`, also the TypeScript path's fallback): index out of bounds (index -1) at vm.rs:8055. | A function in a `finally` block was compiled twice (normal and exception path) and its bindings placed twice; the compiler now reuses the proto per node. |
| `stack-overflow-map-proto.js` | stack-overflow | A Map whose `__proto__` is set to an array overflows the Rust stack and aborts the process. | JS frame limit (10000) and a native stack budget (1 MB, 512 KB on wasm): RangeError. |
| `mismatch-snapshot-regexp-source.js` (+ .txt, its seed) | mismatch-snapshot | After a snapshot and restore, a RegExp's `source` reads `undefined`. Seen with every regexp the generator writes. | The shown source is rebuilt from the regex cache key when the snapshot did not carry it. |

`--heavy` also found `oom` right away: `'ab'.repeat(2 ** 31)` and
`new Array(2 ** 32 - 1).fill(0)` aborted the process when an allocation
failed, and deep recursion (`r(1e6)`) overflowed the stack. Now a string
over 536870888 bytes and an array over 2^25 elements throw RangeError, and
recursion throws RangeError at the limits above. There is still no total
heap limit: many arrays just under the cap can exhaust memory.
