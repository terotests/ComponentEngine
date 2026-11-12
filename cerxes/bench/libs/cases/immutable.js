import { List, Map, Set, OrderedMap, Record, Seq, Range, fromJS, is } from "immutable";
const Pt = Record({ x: 0, y: 0 }, "Pt");
export default {
  check: {
    list: () => { let l = List(); for (let i = 0; i < 2000; i++) l = l.push(i); l = l.set(1000, -1).delete(5).insert(3, "x").update(0, (v) => v + 100); return [l.size, l.get(1000), l.slice(0, 6).toJS(), l.filter((v) => typeof v === "number" && v % 333 === 0).toJS(), l.reduce((a, v) => (typeof v === "number" ? a + v : a), 0)]; },
    maps: () => { const m = fromJS({ a: { b: [1, 2, { c: 3 }] }, d: 4 }); const n = m.setIn(["a", "b", 2, "c"], 9).updateIn(["d"], (x) => x * 2).mergeDeep({ a: { e: 1 } }); return [n.toJS(), m.getIn(["a", "b", 2, "c"]), is(m, fromJS({ a: { b: [1, 2, { c: 3 }] }, d: 4 })), n.hashCode() === fromJS(n.toJS()).hashCode()]; },
    seq: () => [Range(1, Infinity).skip(10).filter((x) => x % 7 === 0).map((x) => x * x).take(5).toArray(), Seq({ a: 1, b: 2, c: 3 }).flip().toObject(), OrderedMap([["z", 1], ["a", 2]]).sortBy((v, k) => k).keySeq().toArray(), Set([3, 1, 2, 3]).union([5]).sort().toArray()],
    records: () => { const p = new Pt({ x: 1 }); const q = p.set("y", 5); return [p.toString(), q.toJS(), p.equals(new Pt({ x: 1, y: 0 })), Map({ [p]: 1 }).size]; },
  },
  bench: {
    listOps: () => { let l = List(); for (let i = 0; i < 3000; i++) l = l.push(i); for (let i = 0; i < 1000; i++) l = l.set((i * 7) % 3000, i); return l.reduce((a, b) => a + b, 0); },
    mapOps: () => { let m = Map(); for (let i = 0; i < 2000; i++) m = m.set("k" + i, i); let s = 0; for (let i = 0; i < 2000; i += 3) s += m.get("k" + i); return s + m.filter((v) => v & 1).size; },
    deep: () => fromJS({ items: Array.from({ length: 200 }, (_, i) => ({ id: i, t: [i, i + 1] })) }).updateIn(["items", 5, "t", 0], (x) => x + 1).toJS().items.length,
  },
};
