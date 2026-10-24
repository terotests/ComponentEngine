// SPDX-License-Identifier: MIT
// From MFilesAPI web/uix2/samples/stress-cpu-js: recursive fib, at its largest size.
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  const fib = (k) => (k < 2 ? k : fib(k - 1) + fib(k - 2))
  const want = { 10: 55, 15: 610, 20: 6765, 25: 75025 }[n]
  const r = fib(n)
  assert(r === want, 'fib(' + n + ') = ' + r)
}
const t0 = Date.now()
run(25)
const ms = Date.now() - t0
console.log('fib\t' + ms)
