// SPDX-License-Identifier: MIT
// String building where the string is not a plain local: a captured
// variable, an object's property and a returned accumulator (how UI code
// builds HTML).
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  let html = ''
  const add = (s) => { html += s }
  for (let i = 0; i < n; i++) add('<li>' + i + '</li>')
  const o = { text: '' }
  for (let i = 0; i < n; i++) o.text += 'x'
  function build(acc, k) { for (let i = 0; i < k; i++) acc = acc + 'y'; return acc }
  const r = build('', n)
  assert(html.length > n && o.text.length === n && r.length === n, 'lengths')
}
const t0 = Date.now()
run(200000)
const ms = Date.now() - t0
console.log('strcap\t' + ms)
