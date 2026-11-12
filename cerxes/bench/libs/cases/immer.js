import { produce, enablePatches, enableMapSet, applyPatches, produceWithPatches, current, original, freeze, isDraft } from "immer";
enablePatches(); enableMapSet();
const base = { users: Array.from({ length: 300 }, (_, i) => ({ id: i, name: "u" + i, tags: ["a"], meta: { n: i } })), map: new Map([["a", { v: 1 }]]), set: new Set([1, 2]) };
export default {
  check: {
    basic: () => { const n = produce(base, (d) => { d.users[3].name = "x"; d.users.push({ id: 999 }); d.map.get("a").v++; d.set.add(3); }); return [n === base, n.users === base.users, n.users[4] === base.users[4], n.users[3].name, base.users[3].name, n.users.length, n.map.get("a").v, base.map.get("a").v, [...n.set], Object.isFrozen(n.users[3])]; },
    patches: () => { const [n, p, inv] = produceWithPatches({ a: [1, 2, 3], b: { c: 1 } }, (d) => { d.a.splice(1, 1); d.b.c = 2; delete d.b.x; d.d = { e: [1] }; }); return [n, p, inv, applyPatches(n, inv)]; },
    current: () => produce({ a: { b: 1 }, l: [1] }, (d) => { d.a.b = 2; const c = current(d); const o = original(d); d.r = [c.a.b, o.a.b, isDraft(d), isDraft(c)]; }).r,
    noop: () => { const n = produce(base, (d) => { d.users[0].name = "u0"; }); return n === base; },
    returnValue: () => produce({ a: 1 }, () => ({ replaced: true })),
  },
  bench: {
    updateMany: () => produce(base, (d) => { for (let i = 0; i < d.users.length; i += 3) { d.users[i].meta.n++; d.users[i].tags.push("b"); } }).users.length,
    reducerLoop: () => { let s = { todos: [], count: 0 }; for (let i = 0; i < 200; i++) s = produce(s, (d) => { d.todos.push({ id: i, done: false }); if (i % 5 === 0) d.todos[i >> 1].done = true; d.count++; }); return s.count; },
    patches: () => produceWithPatches(base, (d) => { for (let i = 0; i < 50; i++) d.users[i * 2].name += "!"; })[1].length,
  },
};
