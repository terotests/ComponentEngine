// SPDX-License-Identifier: MIT
// From MFilesAPI web/uix2/samples/stress-cpu-js: small objects allocated and dropped, at its largest size.
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  let keep = null
  for (let round = 0; round < 4; round++) {
    const objs = []
    for (let i = 0; i < n; i++) objs.push({ i, name: 'o' + i, child: { v: i } })
    keep = objs[n - 1]
  }
  assert(keep.child.v === n - 1, 'last')
}
const t0 = Date.now()
run(50000)
const ms = Date.now() - t0
console.log('alloc\t' + ms)
