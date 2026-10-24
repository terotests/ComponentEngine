// SPDX-License-Identifier: MIT
// From MFilesAPI web/uix2/samples/stress-cpu-js: closures and method calls in a hot loop, at its largest size.
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  class Vec { constructor(x, y) { this.x = x; this.y = y } add(o) { return new Vec(this.x + o.x, this.y + o.y) } }
  let v = new Vec(0, 0)
  const step = new Vec(1, 2)
  const apply = (f) => (x) => f(x)
  const add = apply((x) => x.add(step))
  for (let i = 0; i < n; i++) v = add(v)
  assert(v.x + v.y === 3 * n, String(v.x + v.y))
}
const t0 = Date.now()
run(300000)
const ms = Date.now() - t0
console.log('closures\t' + ms)
