// SPDX-License-Identifier: MIT
// Property reads and writes on objects of a few shapes, and method calls
// through prototypes (Richards-like).
function assert(c, m) { if (!c) throw new Error(m || 'assertion failed') }
function run(n) {
  class Node { constructor(v) { this.value = v; this.next = null; this.count = 0 } touch() { this.count++; return this.value } }
  class Leaf extends Node { constructor(v) { super(v); this.leaf = true } touch() { this.count += 2; return this.value * 2 } }
  const list = []
  for (let i = 0; i < 1000; i++) list.push(i % 3 ? new Node(i) : new Leaf(i))
  for (let i = 0; i < 999; i++) list[i].next = list[i + 1]
  let sum = 0
  for (let r = 0; r < n; r++) {
    let p = list[0]
    while (p !== null) { sum += p.touch(); p = p.next }
  }
  assert(sum > 0, String(sum))
}
const t0 = Date.now()
run(300)
const ms = Date.now() - t0
console.log('props\t' + ms)
