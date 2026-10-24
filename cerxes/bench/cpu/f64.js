// SPDX-License-Identifier: MIT
// From MFilesAPI web/uix2/samples/stress-cpu-js: Float64Array math, at its largest size.
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  const a = new Float64Array(n)
  for (let i = 0; i < n; i++) a[i] = Math.sin(i) * Math.sqrt(i)
  let sum = 0
  for (let i = 0; i < n; i++) sum += a[i] * a[i]
  assert(sum > 0 && Number.isFinite(sum), String(sum))
}
const t0 = Date.now()
run(1000000)
const ms = Date.now() - t0
console.log('f64\t' + ms)
