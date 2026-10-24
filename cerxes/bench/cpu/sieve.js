// SPDX-License-Identifier: MIT
// From MFilesAPI web/uix2/samples/stress-cpu-js: sieve of Eratosthenes, at its largest size.
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  const composite = new Uint8Array(n + 1)
  let count = 0
  for (let i = 2; i <= n; i++) {
    if (composite[i]) continue
    count++
    for (let j = i * i; j <= n; j += i) composite[j] = 1
  }
  const want = { 1000: 168, 10000: 1229, 100000: 9592, 1000000: 78498 }[n]
  assert(count === want, 'primes to ' + n + ': ' + count)
}
const t0 = Date.now()
run(1000000)
const ms = Date.now() - t0
console.log('sieve\t' + ms)
