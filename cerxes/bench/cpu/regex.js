// SPDX-License-Identifier: MIT
// From MFilesAPI web/uix2/samples/stress-cpu-js: regular expressions over a log, at its largest size.
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  const text = 'INFO 2026-10-23 user=tero action=open id=1234 path=/a/b/c.pdf\n'.repeat(n)
  const ids = text.match(/id=\d+/g).length
  let paths = 0
  for (const m of text.matchAll(/path=(\S+)\.pdf/g)) if (m[1].length) paths++
  const replaced = text.replace(/user=\w+/g, 'user=x').length
  assert(ids === n && paths === n && replaced < text.length, ids + '/' + paths)
}
const t0 = Date.now()
run(16000)
const ms = Date.now() - t0
console.log('regex\t' + ms)
