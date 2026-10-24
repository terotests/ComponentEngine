// SPDX-License-Identifier: MIT
// From MFilesAPI web/uix2/samples/stress-cpu-js: JSON stringify and parse, at its largest size.
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  const data = Array.from({ length: n }, (_, i) => ({ id: i, title: 'Document ' + i, tags: ['a', 'b'], props: { size: i * 3, ok: i % 2 === 0 } }))
  const text = JSON.stringify(data)
  const back = JSON.parse(text)
  assert(back.length === n && back[n - 1].props.size === (n - 1) * 3, 'round trip')
}
const t0 = Date.now()
run(10000)
const ms = Date.now() - t0
console.log('json\t' + ms)
