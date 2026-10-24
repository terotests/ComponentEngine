// SPDX-License-Identifier: MIT
// From MFilesAPI web/uix2/samples/stress-cpu-js: Map and Set, at its largest size.
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  const m = new Map()
  const s = new Set()
  for (let i = 0; i < n; i++) { m.set('k' + i, i); s.add(i % 1000) }
  let sum = 0
  for (let i = 0; i < n; i++) sum += m.get('k' + i)
  m.forEach((v, k) => { if (v % 2) m.delete(k) })
  assert(m.size === n / 2 && s.size === 1000 && sum === (n * (n - 1)) / 2, m.size + '/' + s.size + '/' + sum)
}
const t0 = Date.now()
run(100000)
const ms = Date.now() - t0
console.log('mapset\t' + ms)
