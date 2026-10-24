// SPDX-License-Identifier: MIT
// From MFilesAPI web/uix2/samples/stress-cpu-js: string building: concatenation and join, at its largest size.
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  let s = ''
  for (let i = 0; i < n; i++) s += String.fromCharCode(97 + (i % 26))
  const parts = []
  for (let i = 0; i < n / 2; i++) parts.push(i)
  assert(s.length === n && parts.join(',').split(',').length === n / 2, 'lengths')
}
const t0 = Date.now()
run(200000)
const ms = Date.now() - t0
console.log('strcat\t' + ms)
