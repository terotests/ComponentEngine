# Seeds

Extra seed programs for mutation: every `.js` file here (and in
subdirectories given with `--corpus`) is read whole. `run-local.sh` and
the nightly run also read `cerxes/tests/v8/own` (the known V8 bug tests,
10 grouped files, one test per line) when it exists.

Put a new case here as one self-contained `.js` file; its first line may
say where it comes from (`// v8: crbug 1426357 array-builtin-reentrancy`).
The 163 V8 mjsunit regressions on which CErXes failed are not committed
(V8's license and size); `/mnt/project-files/v8-bugs/cases` has them for
local runs (`--corpus /mnt/project-files/v8-bugs/cases`).

## Generator patterns from the known V8 bug classes

The generator (src/gen.rs) writes each class of the V8 bug list
(`classes.md` of the thread "Test CErXes against known V8 bugs") as a
statement kind, so the patterns are varied, not only replayed:

| V8 class | Generator |
|---|---|
| array-builtin-reentrancy | `side_stmt`: valueOf / toString / toPrimitive argument, callback, element toString or species that shrinks, grows, freezes or re-protos the array halfway through 30 builtins |
| typedarray-detach-resize | `typed_side_stmt`: resize, transfer, transferToFixedLength inside valueOf / comparator / map |
| proxy-getter-reentrancy | `reentry_stmt` 0–3, 14, 15; `--heavy`: 5000-deep Proxy chains |
| regexp-reentrancy-limits | `reentry_stmt` 4–6; `--heavy`: 3000 nested groups |
| jit-type-range-assumptions | `hot_stmt`: 70 or 160 rounds (past the JIT threshold 64), then an edge index (2**32, 2**53, -0, NaN, '1') or a shape / prototype / hole change; int32 sums crossing the boundary |
| size-limits-int-overflow | `edge_stmt`: computed -0, int32 / uint32 edges, radix conversion of big numbers; `--heavy`: padEnd(2**30) |
| parser-depth-early-errors | mutation (deep nesting); `--heavy`: 65536 parameters |
| iterator-generator-promise-gc | `reentry_stmt` 7, 8, 11–13 |
| class-private-scope-intl | `reentry_stmt` 9, 10 |
| native-recursion-depth | `--heavy`: toString / getter recursion, cyclic flat |
