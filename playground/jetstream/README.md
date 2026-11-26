# JetStream 3 material

The playground's **JetStream 3** tab runs the JavaScript benchmarks of
WebKit's [JetStream 3](https://github.com/WebKit/JetStream), the ones it runs
by default, in every ticked engine.

The workloads (~80 MB) are not kept in this repository. `update.mjs` fetches
the pinned revision (`REV` in it) as a sparse checkout of only the files
needed into `jetstream/JetStream` (git ignored) and writes
`dist/jetstream/`: `manifest.json` (benchmarks, iterations, files,
preloads, and the default benchmarks left out with the reason) and the
workloads' files. `build.mjs` runs it as its `jetstream` step.

`check.mjs` runs the same benchmarks outside the browser (Node's engine or
a CErXes WASM build in a worker thread), for finding what an engine fails:

    node playground/jetstream/check.mjs --engine=node,cerxes-wasm --cap=1

Left out: the WebAssembly workloads (CErXes and the other interpreters have
no WebAssembly API) and the two that need Web Workers (JetStream itself
leaves them out in shells).

## Licences

JetStream: BSD 2-clause, Apple Inc. (copied as `dist/jetstream/LICENSE`).
Workloads keep their own open-source licences (BSD, MIT, Apache 2.0 and
similar), in the headers of their files or in the LICENSE files
`update.mjs` copies next to them (listed under `licences` in the
manifest). JetStream distributes them all for running as a benchmark, as
done here.
