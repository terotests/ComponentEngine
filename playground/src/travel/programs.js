// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The travelling-engine demo's JavaScript, run inside CErXes.
//
// SHARD makes shard k's rows (a seeded generator, so every host and every
// mode sees the same data). AGENT is the query: per-group count, sum and max
// of the rows from 2023 on, kept in one data root, globalThis.__carry, so
// that root is all a move has to take when the code is already there.

export const SHARD = `
function loadShard(k, n) {
  let s = k * 7919 + 1;
  const rnd = () => (s = (s * 48271) % 2147483647) / 2147483647;
  const regions = ['north', 'south', 'east', 'west', 'central'];
  const rows = new Array(n);
  for (let i = 0; i < n; i++) rows[i] = { id: k * 1e7 + i, region: regions[(rnd() * 5) | 0], customer: 'C' + ((rnd() * 10000) | 0), amount: Math.round(rnd() * 100000) / 100, year: 2020 + ((rnd() * 6) | 0) };
  return rows;
}
`;

export const AGENT = `
globalThis.__carry = globalThis.__carry || { groups: {}, hops: 0, rows: 0 };
function step(rows, key) {
  const g = __carry.groups;
  for (let i = 0; i < rows.length; i++) {
    const r = rows[i];
    if (r.year < 2023) continue;
    const k = r[key];
    const a = g[k] || (g[k] = { n: 0, sum: 0, max: 0 });
    a.n++; a.sum += r.amount; if (r.amount > a.max) a.max = r.amount;
  }
  __carry.hops++; __carry.rows += rows.length;
}
function merge(part) {
  const g = __carry.groups;
  for (const k in part.groups) {
    const p = part.groups[k];
    const a = g[k] || (g[k] = { n: 0, sum: 0, max: 0 });
    a.n += p.n; a.sum += p.sum; if (p.max > a.max) a.max = p.max;
  }
  __carry.hops++; __carry.rows += part.rows;
}
function summary() {
  const g = __carry.groups;
  let n = 0, s = 0;
  for (const k in g) { n += g[k].n; s += g[k].sum; }
  return Object.keys(g).length + ' groups, n=' + n + ', sum=' + s.toFixed(2);
}
`;

// A collection at once: gc() only asks for one, and it runs at the next safe
// point (a call or a backward jump) in the same script.
export const COLLECT = "gc(); (() => 0)()";
