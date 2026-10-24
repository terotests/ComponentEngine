// SPDX-License-Identifier: MIT
// From MFilesAPI web/uix2/samples/stress-cpu-js: sort numbers and strings, at its largest size.
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  let seed = 1
  const rand = () => (seed = (seed * 16807) % 2147483647)
  const nums = Array.from({ length: n }, rand)
  nums.sort((a, b) => a - b)
  for (let i = 1; i < nums.length; i++) if (nums[i - 1] > nums[i]) throw new Error('numbers out of order')
  const strs = Array.from({ length: n / 5 }, () => rand().toString(36))
  strs.sort()
  for (let i = 1; i < strs.length; i++) if (strs[i - 1] > strs[i]) throw new Error('strings out of order')
}
const t0 = Date.now()
run(100000)
const ms = Date.now() - t0
console.log('sort\t' + ms)
