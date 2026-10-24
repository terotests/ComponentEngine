// SPDX-License-Identifier: MIT
// Rows rendered with template literals and joined, the way a list view
// renders.
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  const rows = []
  for (let i = 0; i < n; i++) rows.push({ id: i, title: 'Document ' + i, size: i * 3, modified: '2026-10-' + (1 + (i % 28)) })
  const out = rows.map((r) => `<tr data-id="${r.id}"><td>${r.title}</td><td>${r.size} KB</td><td>${r.modified}</td></tr>`).join('\n')
  assert(out.split('\n').length === n, 'rows')
}
const t0 = Date.now()
run(100000)
const ms = Date.now() - t0
console.log('template\t' + ms)
