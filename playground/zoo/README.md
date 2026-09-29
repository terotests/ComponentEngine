# Zoo ranking material

What the playground's **Zoo ranking** tab runs and ranks against, from
Ivan Krasilnikov's [javascript-zoo](https://github.com/ivankra/javascript-zoo)
(the project behind [zoo.js.org](https://zoo.js.org/)):

| File | What | From |
| --- | --- | --- |
| `octane/*.js` | the eight Octane v9 suites of zoo.js.org's Score, self-contained | javascript-zoo `bench/` |
| `conformance.json` | 1,370 tests: `es1`, `es3`, `es5` and the compat-table ports for ES6 and ES2016–ES2025, each with its compat-table weight | javascript-zoo `conformance/` |
| `reference.json` | each engine's published amd64 results: Octane medians, ES1–5 pass rate, ES6 and ES2016+ weighted pass rates | [javascript-zoo-data](https://github.com/ivankra/javascript-zoo-data) (branch `data`) |

`update.mjs` regenerates them from checkouts of the two repositories; its
header has the commands. The revisions used are in the `source` fields of
the two JSON files.

## Licences

- javascript-zoo and javascript-zoo-data: MIT (`LICENSE.javascript-zoo`)
- the compat-table tests: MIT, Juriy Zaytsev (`LICENSE.compat-table`)
- Octane: BSD-style, the V8 project authors (the header of each suite)
